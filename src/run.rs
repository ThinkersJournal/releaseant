//! The run loop: plan in, report and ledger out (DESIGN sections 3 to 6).
//!
//! `run` is also resume. It never trusts the ledger about what is published; for each crate it asks
//! the registry, uploads only on a definitive "absent", then reads the crate back before calling it
//! done.

use std::fmt;
use std::time::Duration;

use semver::Version;

use crate::ledger::{Event, EventKind, Ledger, LedgerError};
use crate::plan::Plan;
use crate::registry::{Clock, Lookup, Registry, Upload};

/// Whether a run may upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Look up and report; upload nothing. The default.
    #[default]
    DryRun,
    /// Upload.
    Execute,
}

/// Tunables. The defaults are conservative: dry-run, one hour of total waiting.
#[derive(Debug, Clone)]
pub struct Options {
    /// Dry-run or execute.
    pub mode: Mode,
    /// Total time one run may spend waiting before it stops (resumable).
    pub max_wait: Duration,
    /// First backoff when the server gave no advice; doubles per attempt.
    pub backoff_base: Duration,
    /// Backoff ceiling.
    pub backoff_cap: Duration,
    /// Added to a server-advised wait.
    pub retry_margin: Duration,
    /// Retries for transient network errors per crate.
    pub transient_retries: u32,
    /// How many times read-back looks for a crate.
    pub readback_polls: u32,
    /// Wait between read-back looks.
    pub readback_interval: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::DryRun,
            max_wait: Duration::from_secs(3600),
            backoff_base: Duration::from_secs(30),
            backoff_cap: Duration::from_secs(600),
            retry_margin: Duration::from_secs(5),
            transient_retries: 4,
            readback_polls: 30,
            readback_interval: Duration::from_secs(10),
        }
    }
}

/// Identity of a run.
#[derive(Debug, Clone)]
pub struct RunContext {
    /// Run identifier recorded on every event.
    pub run_id: String,
    /// Full 40-hex commit SHA the archive came from.
    pub sha: String,
}

/// What became of one crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Dry-run: the registry does not have it; an execute run would upload it.
    WouldUpload,
    /// The registry already had this version.
    AlreadyPublished,
    /// Uploaded and read back, checksum agreed where one was known.
    Visible,
    /// Uploaded, but the index checksum differs from what was uploaded.
    ChecksumMismatch {
        /// SHA-256 of what was uploaded.
        uploaded: String,
        /// What the index shows.
        index: String,
    },
    /// Uploaded, but read-back never saw it.
    Missing,
    /// The registry refused, or retries ran out.
    Failed(String),
    /// Not attempted because a dependency did not succeed.
    Skipped {
        /// The first dependency that did not succeed.
        blocked_by: String,
    },
    /// The run stopped while working on this crate.
    Interrupted(String),
    /// The run stopped before reaching this crate.
    NotAttempted,
}

impl Outcome {
    /// Counts as done for the purpose of letting dependents proceed.
    fn is_good(&self) -> bool {
        matches!(
            self,
            Self::WouldUpload | Self::AlreadyPublished | Self::Visible
        )
    }
}

/// The result of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The mode the run used.
    pub mode: Mode,
    /// One outcome per planned crate, in plan order.
    pub outcomes: Vec<(String, Outcome)>,
    /// Why the whole run stopped early, if it did. A stopped run is resumable.
    pub stopped: Option<String>,
}

impl Report {
    /// True only for an executed run in which every crate is published and read back.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.mode == Mode::Execute
            && self.stopped.is_none()
            && self
                .outcomes
                .iter()
                .all(|(_, o)| matches!(o, Outcome::AlreadyPublished | Outcome::Visible))
    }
}

/// Why a run could not start or write its record.
#[derive(Debug)]
pub enum RunError {
    /// The SHA is not 40 lowercase hex digits.
    BadSha(String),
    /// The ledger has a run at this SHA with a different plan.
    PlanChanged {
        /// Fingerprint recorded by the earlier run.
        recorded: String,
        /// Fingerprint of the plan now.
        now: String,
    },
    /// The ledger failed.
    Ledger(LedgerError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSha(s) => write!(f, "not a full commit SHA: {s:?}"),
            Self::PlanChanged { recorded, now } => write!(
                f,
                "the ledger has a run at this commit with a different plan ({recorded} vs {now})"
            ),
            Self::Ledger(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RunError {}

impl From<LedgerError> for RunError {
    fn from(e: LedgerError) -> Self {
        Self::Ledger(e)
    }
}

struct Recorder<'a> {
    ledger: &'a Ledger,
    ctx: &'a RunContext,
}

impl Recorder<'_> {
    fn log(
        &self,
        clock: &mut dyn Clock,
        kind: EventKind,
        krate: Option<(&str, &Version)>,
        detail: impl Into<String>,
    ) -> Result<(), RunError> {
        self.ledger.append(&Event {
            ts: clock.now_utc(),
            run: self.ctx.run_id.clone(),
            sha: self.ctx.sha.clone(),
            crate_name: krate.map(|(n, _)| n.to_owned()),
            version: krate.map(|(_, v)| v.to_string()),
            event: kind,
            detail: detail.into(),
        })?;
        Ok(())
    }
}

fn valid_sha(s: &str) -> bool {
    s.len() == 40
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Run (or resume, or dry-run) the plan.
///
/// # Errors
/// [`RunError`] when the run cannot start or cannot write its ledger. Registry trouble is not an
/// error: it ends the run early and is reported in [`Report::stopped`].
pub fn run(
    plan: &Plan,
    registry: &mut dyn Registry,
    clock: &mut dyn Clock,
    ledger: &Ledger,
    ctx: &RunContext,
    opts: &Options,
) -> Result<Report, RunError> {
    if !valid_sha(&ctx.sha) {
        return Err(RunError::BadSha(ctx.sha.clone()));
    }
    let fingerprint = plan.fingerprint();
    let earlier = ledger.read()?;
    let last_plan = earlier
        .iter()
        .rev()
        .find(|e| e.event == EventKind::Planned && e.crate_name.is_none() && e.sha == ctx.sha);
    if let Some(e) = last_plan
        && e.detail != fingerprint
    {
        return Err(RunError::PlanChanged {
            recorded: e.detail.clone(),
            now: fingerprint,
        });
    }

    let rec = Recorder { ledger, ctx };
    rec.log(clock, EventKind::Planned, None, fingerprint)?;

    let mut outcomes: Vec<(String, Outcome)> = Vec::with_capacity(plan.order.len());
    let mut stopped: Option<String> = None;
    let mut waited = Duration::ZERO;

    for c in &plan.order {
        if stopped.is_some() {
            outcomes.push((c.name.clone(), Outcome::NotAttempted));
            continue;
        }
        let blocker = c
            .depends_on
            .iter()
            .find(|d| !outcomes.iter().any(|(n, o)| n == *d && o.is_good()));
        if let Some(b) = blocker {
            rec.log(
                clock,
                EventKind::Skipped,
                Some((&c.name, &c.version)),
                format!("blocked by {b}"),
            )?;
            outcomes.push((
                c.name.clone(),
                Outcome::Skipped {
                    blocked_by: b.clone(),
                },
            ));
            continue;
        }

        let outcome = match registry.lookup(&c.name, &c.version) {
            Lookup::Unknown(why) => {
                let reason = format!("registry lookup failed for {}: {why}", c.name);
                stopped = Some(reason.clone());
                Outcome::Interrupted(reason)
            }
            Lookup::Present { .. } => {
                rec.log(
                    clock,
                    EventKind::Classified,
                    Some((&c.name, &c.version)),
                    "already-published",
                )?;
                Outcome::AlreadyPublished
            }
            Lookup::Absent if opts.mode == Mode::DryRun => Outcome::WouldUpload,
            Lookup::Absent => publish_one(
                &rec,
                registry,
                clock,
                opts,
                &c.name,
                &c.version,
                &mut waited,
                &mut stopped,
            )?,
        };
        outcomes.push((c.name.clone(), outcome));
    }

    rec.log(
        clock,
        EventKind::Finished,
        None,
        stopped.clone().unwrap_or_else(|| "complete".to_owned()),
    )?;
    Ok(Report {
        mode: opts.mode,
        outcomes,
        stopped,
    })
}

fn backoff(opts: &Options, step: u32) -> Duration {
    let factor = 2u32.saturating_pow(step.min(20));
    opts.backoff_base
        .saturating_mul(factor)
        .min(opts.backoff_cap)
}

/// Spend `d` of the run's wait budget; `false` when that would exceed it.
fn wait(clock: &mut dyn Clock, opts: &Options, waited: &mut Duration, d: Duration) -> bool {
    if *waited + d > opts.max_wait {
        return false;
    }
    clock.sleep(d);
    *waited += d;
    true
}

#[allow(clippy::too_many_arguments)]
fn publish_one(
    rec: &Recorder<'_>,
    registry: &mut dyn Registry,
    clock: &mut dyn Clock,
    opts: &Options,
    name: &str,
    version: &Version,
    waited: &mut Duration,
    stopped: &mut Option<String>,
) -> Result<Outcome, RunError> {
    let who = Some((name, version));
    let mut step = 0u32;
    let mut transient = 0u32;
    let uploaded: Option<String> = loop {
        // The event is on disk before the upload starts.
        rec.log(clock, EventKind::UploadStarted, who, "")?;
        let result = registry.upload(name, version);
        rec.log(clock, EventKind::UploadFinished, who, format!("{result:?}"))?;
        let pause = match result {
            Upload::Accepted { checksum } => break Some(checksum),
            Upload::AlreadyPublished => break None,
            Upload::RateLimited { retry_after } => {
                Some(retry_after.map_or_else(|| backoff(opts, step), |r| r + opts.retry_margin))
            }
            Upload::DependencyNotVisible => Some(backoff(opts, step)),
            Upload::Transient(why) => {
                transient += 1;
                if transient > opts.transient_retries {
                    return Ok(Outcome::Failed(format!(
                        "transient errors persisted: {why}"
                    )));
                }
                Some(backoff(opts, step))
            }
            Upload::Permanent(why) => {
                rec.log(
                    clock,
                    EventKind::Classified,
                    who,
                    format!("permanent: {why}"),
                )?;
                return Ok(Outcome::Failed(why));
            }
            Upload::Unknown(text) => {
                let reason = format!("unrecognised upload result for {name}: {text}");
                rec.log(
                    clock,
                    EventKind::Classified,
                    who,
                    format!("unknown: {text}"),
                )?;
                *stopped = Some(reason.clone());
                return Ok(Outcome::Interrupted(reason));
            }
        };
        let d = pause.expect("every non-terminal arm sets a pause");
        rec.log(clock, EventKind::Waiting, who, format!("{d:?}"))?;
        if !wait(clock, opts, waited, d) {
            let reason = format!("max-wait exceeded while publishing {name}");
            *stopped = Some(reason.clone());
            return Ok(Outcome::Interrupted(reason));
        }
        step += 1;
    };
    read_back(rec, registry, clock, opts, name, version, uploaded)
}

fn read_back(
    rec: &Recorder<'_>,
    registry: &mut dyn Registry,
    clock: &mut dyn Clock,
    opts: &Options,
    name: &str,
    version: &Version,
    uploaded: Option<String>,
) -> Result<Outcome, RunError> {
    let who = Some((name, version));
    for poll in 0..opts.readback_polls {
        if let Lookup::Present { checksum } = registry.lookup(name, version) {
            if let Some(mine) = uploaded
                && mine != checksum
            {
                rec.log(
                    clock,
                    EventKind::Classified,
                    who,
                    format!("checksum mismatch: uploaded {mine}, index {checksum}"),
                )?;
                return Ok(Outcome::ChecksumMismatch {
                    uploaded: mine,
                    index: checksum,
                });
            }
            rec.log(clock, EventKind::Visible, who, "")?;
            return Ok(Outcome::Visible);
        }
        if poll + 1 < opts.readback_polls {
            clock.sleep(opts.readback_interval);
        }
    }
    rec.log(clock, EventKind::Missing, who, "")?;
    Ok(Outcome::Missing)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::rc::Rc;

    use super::*;
    use crate::plan::PlannedCrate;
    use crate::registry::fake::{FakeClock, FakeRegistry};

    fn pc(name: &str, deps: &[&str]) -> PlannedCrate {
        PlannedCrate {
            name: name.into(),
            version: "1.0.0".parse().unwrap(),
            depends_on: deps
                .iter()
                .map(|d| (*d).to_owned())
                .collect::<BTreeSet<_>>(),
        }
    }

    fn chain() -> Plan {
        Plan {
            order: vec![pc("a", &[]), pc("b", &["a"]), pc("c", &["b"])],
        }
    }

    fn ctx() -> RunContext {
        RunContext {
            run_id: "r1".into(),
            sha: "0123456789abcdef0123456789abcdef01234567".into(),
        }
    }

    fn exec() -> Options {
        Options {
            mode: Mode::Execute,
            ..Options::default()
        }
    }

    struct Env {
        _dir: tempfile::TempDir,
        path: PathBuf,
        ledger: Ledger,
    }

    fn env() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let ledger = Ledger::open(&path).unwrap();
        Env {
            _dir: dir,
            path,
            ledger,
        }
    }

    fn outcome<'a>(r: &'a Report, n: &str) -> &'a Outcome {
        &r.outcomes.iter().find(|(x, _)| x == n).unwrap().1
    }

    #[test]
    fn happy_path_publishes_in_order_and_reads_back() {
        let e = env();
        let mut reg = FakeRegistry::new();
        let mut clock = FakeClock::default();
        let r = run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert!(r.is_success(), "{r:?}");
        assert_eq!(
            reg.uploads.iter().map(|u| u.0.as_str()).collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    // T6: 429 N times, then OK; the run does not abort and waits the advised time.
    #[test]
    fn rate_limit_waits_the_advised_time_then_publishes() {
        let e = env();
        let mut reg = FakeRegistry::new();
        reg.upload_script.insert(
            "b".into(),
            [
                Upload::RateLimited {
                    retry_after: Some(Duration::from_secs(60)),
                },
                Upload::RateLimited { retry_after: None },
            ]
            .into(),
        );
        let mut clock = FakeClock::default();
        let r = run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert!(r.is_success(), "{r:?}");
        assert_eq!(
            clock.sleeps,
            [Duration::from_secs(65), Duration::from_secs(60)],
            "advised 60s + 5s margin, then the unadvised backoff at step 1 (30s base doubled)"
        );
    }

    #[test]
    fn max_wait_stops_the_run_resumably_and_later_crates_are_not_attempted() {
        let e = env();
        let mut reg = FakeRegistry::new();
        reg.upload_script.insert(
            "b".into(),
            std::iter::repeat_n(
                Upload::RateLimited {
                    retry_after: Some(Duration::from_secs(1000)),
                },
                10,
            )
            .collect(),
        );
        let mut clock = FakeClock::default();
        let r = run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert!(r.stopped.as_deref().unwrap().contains("max-wait"));
        assert_eq!(outcome(&r, "a"), &Outcome::Visible);
        assert!(matches!(outcome(&r, "b"), Outcome::Interrupted(_)));
        assert_eq!(outcome(&r, "c"), &Outcome::NotAttempted);
        assert!(!r.is_success());
    }

    // T7: an interrupted run resumes; every crate is uploaded exactly once overall.
    #[test]
    fn resume_uploads_only_what_is_missing_and_each_crate_once() {
        let e = env();
        let mut reg = FakeRegistry::new();
        reg.upload_script
            .insert("b".into(), [Upload::Unknown("boom".into())].into());
        let mut clock = FakeClock::default();
        let first = run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert!(first.stopped.is_some());
        assert_eq!(reg.uploads.len(), 1); // only "a"

        let second = run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert!(second.is_success(), "{second:?}");
        assert_eq!(outcome(&second, "a"), &Outcome::AlreadyPublished);
        let mut names: Vec<_> = reg.uploads.iter().map(|u| u.0.clone()).collect();
        names.sort();
        assert_eq!(names, ["a", "b", "c"], "each crate uploaded exactly once");
    }

    // T8: a failed lookup is never read as "not published".
    #[test]
    fn lookup_failure_stops_the_run_with_zero_uploads() {
        let e = env();
        let mut reg = FakeRegistry::new();
        reg.lookup_script
            .insert("a".into(), [Lookup::Unknown("timeout".into())].into());
        let mut clock = FakeClock::default();
        let r = run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert!(r.stopped.as_deref().unwrap().contains("lookup failed"));
        assert!(reg.upload_calls.is_empty());
    }

    #[test]
    fn unrecognised_upload_result_stops_the_run_and_is_not_retried() {
        let e = env();
        let mut reg = FakeRegistry::new();
        reg.upload_script.insert(
            "a".into(),
            [Upload::Unknown("a generated rate of sorts".into())].into(),
        );
        let mut clock = FakeClock::default();
        let r = run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert!(r.stopped.is_some());
        assert_eq!(reg.upload_calls, ["a"], "one call, no retry");
        assert!(clock.sleeps.is_empty());
    }

    #[test]
    fn permanent_failure_skips_dependents_but_independent_crates_continue() {
        let e = env();
        let plan = Plan {
            order: vec![pc("a", &[]), pc("b", &["a"]), pc("z", &[])],
        };
        let mut reg = FakeRegistry::new();
        reg.upload_script
            .insert("a".into(), [Upload::Permanent("name taken".into())].into());
        let mut clock = FakeClock::default();
        let r = run(&plan, &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert_eq!(outcome(&r, "a"), &Outcome::Failed("name taken".into()));
        assert_eq!(
            outcome(&r, "b"),
            &Outcome::Skipped {
                blocked_by: "a".into()
            }
        );
        assert_eq!(outcome(&r, "z"), &Outcome::Visible);
        assert!(r.stopped.is_none());
        assert!(!r.is_success());
    }

    #[test]
    fn transient_errors_retry_a_bounded_number_of_times() {
        let e = env();
        let mut reg = FakeRegistry::new();
        reg.upload_script.insert(
            "a".into(),
            std::iter::repeat_n(Upload::Transient("dns".into()), 10).collect(),
        );
        let mut clock = FakeClock::default();
        let r = run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert!(matches!(outcome(&r, "a"), Outcome::Failed(_)));
        assert_eq!(reg.upload_calls.len(), 5, "1 try + 4 retries");
    }

    // T13: index lag is waited out; permanent absence is reported as Missing, never success.
    #[test]
    fn readback_waits_for_index_lag() {
        let e = env();
        let mut reg = FakeRegistry::new();
        reg.lookup_lag = 3;
        let mut clock = FakeClock::default();
        let r = run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert!(r.is_success(), "{r:?}");
        assert!(!clock.sleeps.is_empty(), "read-back had to wait");
    }

    #[test]
    fn crate_that_never_appears_is_missing_and_blocks_dependents() {
        let e = env();
        let mut reg = FakeRegistry::new();
        reg.never_visible.insert("a".into());
        let mut clock = FakeClock::default();
        let r = run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert_eq!(outcome(&r, "a"), &Outcome::Missing);
        assert!(matches!(outcome(&r, "b"), Outcome::Skipped { .. }));
        assert!(!r.is_success());
    }

    // T14: the index checksum must equal what was uploaded.
    #[test]
    fn checksum_mismatch_is_reported_not_success() {
        let e = env();
        let mut reg = FakeRegistry::new();
        reg.index_checksum_override
            .insert("a".into(), "other".into());
        let mut clock = FakeClock::default();
        let r = run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert!(matches!(outcome(&r, "a"), Outcome::ChecksumMismatch { .. }));
        assert!(!r.is_success());
    }

    // T17: dry-run (the default) uploads nothing and writes only the planned/finished events.
    #[test]
    fn default_options_are_dry_run_and_upload_nothing() {
        assert_eq!(Options::default().mode, Mode::DryRun);
        let e = env();
        let mut reg = FakeRegistry::new().with_published("a", "1.0.0");
        let mut clock = FakeClock::default();
        let r = run(
            &chain(),
            &mut reg,
            &mut clock,
            &e.ledger,
            &ctx(),
            &Options::default(),
        )
        .unwrap();
        assert!(reg.upload_calls.is_empty());
        assert_eq!(outcome(&r, "a"), &Outcome::AlreadyPublished);
        assert_eq!(outcome(&r, "b"), &Outcome::WouldUpload);
        assert!(!r.is_success(), "a dry run is never a success");
        let kinds: BTreeSet<_> = e
            .ledger
            .read()
            .unwrap()
            .iter()
            .map(|x| format!("{:?}", x.event))
            .collect();
        assert!(!kinds.contains("UploadStarted"), "{kinds:?}");
    }

    // T16: the upload_started event is on disk before the upload runs.
    #[test]
    fn upload_started_is_in_the_ledger_before_the_upload_happens() {
        let e = env();
        let seen: Rc<RefCell<Vec<bool>>> = Rc::default();
        let (seen2, path) = (Rc::clone(&seen), e.path.clone());
        let mut reg = FakeRegistry::new();
        reg.on_upload = Some(Box::new(move |name, _| {
            let text = std::fs::read_to_string(&path).unwrap();
            let want = format!(r#""crate":"{name}""#);
            seen2.borrow_mut().push(
                text.lines()
                    .any(|l| l.contains(&want) && l.contains("upload_started")),
            );
        }));
        let mut clock = FakeClock::default();
        run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        assert_eq!(*seen.borrow(), [true, true, true]);
    }

    #[test]
    fn bad_sha_and_changed_plan_at_same_sha_are_refused_but_new_sha_is_a_new_run() {
        let e = env();
        let mut reg = FakeRegistry::new();
        let mut clock = FakeClock::default();
        let mut bad = ctx();
        bad.sha = "abc123".into();
        assert!(matches!(
            run(&chain(), &mut reg, &mut clock, &e.ledger, &bad, &exec()),
            Err(RunError::BadSha(_))
        ));

        run(&chain(), &mut reg, &mut clock, &e.ledger, &ctx(), &exec()).unwrap();
        let changed = Plan {
            order: vec![pc("a", &[]), pc("b", &["a"])],
        };
        assert!(matches!(
            run(&changed, &mut reg, &mut clock, &e.ledger, &ctx(), &exec()),
            Err(RunError::PlanChanged { .. })
        ));
        let mut newer = ctx();
        newer.sha = "f".repeat(40);
        assert!(run(&changed, &mut reg, &mut clock, &e.ledger, &newer, &exec()).is_ok());
    }
}
