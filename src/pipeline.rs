//! prepare -> package -> verify -> run -> consumer checks (DESIGN section 3).

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use semver::Version;

use crate::cargo::{Cargo, CargoError};
use crate::externals::{ExternalsError, MissingExternal, missing_externals};
use crate::ledger::{Event, EventKind, Ledger, LedgerError};
use crate::metadata::Metadata;
use crate::plan::{Plan, PlanError, plan};
use crate::registry::{Clock, Lookup, Registry};
use crate::run::{Mode, Options, Outcome, Report, RunContext, RunError, run};
use crate::source::{self, SourceError};
use crate::verify::{PackagedCrate, verify_set};

/// A recorded exception to a rule: this crate skips pre-upload verification, for this reason.
#[derive(Debug, Clone)]
pub struct Exception {
    /// The crate.
    pub krate: String,
    /// Why. Must not be blank.
    pub reason: String,
}

/// Why the pipeline stopped before or around the run.
#[derive(Debug)]
pub enum PipelineError {
    /// Archiving the commit failed.
    Source(SourceError),
    /// A cargo step failed.
    Cargo(CargoError),
    /// The plan could not be made.
    Plan(PlanError),
    /// The run could not start or write its record.
    Run(RunError),
    /// The ledger failed.
    Ledger(LedgerError),
    /// File system trouble.
    Io(io::Error),
    /// Pre-upload verification failed for these crates; nothing was uploaded.
    VerifyFailed(Vec<(String, String)>),
    /// An exception has no reason.
    ExceptionWithoutReason(String),
    /// An exception names a crate that is not in the plan.
    ExceptionForUnknownCrate(String),
    /// An exclusion has no reason.
    ExclusionWithoutReason(String),
    /// An exclusion names a crate that is not in the publish plan.
    ExclusionForUnknownCrate(String),
    /// The registry could not be asked about the external dependencies.
    Externals(ExternalsError),
    /// Dependencies outside the set that the registry cannot satisfy; nothing was uploaded.
    MissingExternals(Vec<MissingExternal>),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(e) => write!(f, "{e}"),
            Self::Cargo(e) => write!(f, "{e}"),
            Self::Plan(e) => write!(f, "{e}"),
            Self::Run(e) => write!(f, "{e}"),
            Self::Ledger(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "I/O: {e}"),
            Self::VerifyFailed(v) => {
                write!(f, "verification failed; nothing was uploaded:")?;
                for (n, why) in v {
                    write!(f, "\n--- {n}\n{why}")?;
                }
                Ok(())
            }
            Self::ExceptionWithoutReason(c) => {
                write!(f, "skipping verification of {c} needs a non-empty reason")
            }
            Self::ExceptionForUnknownCrate(c) => {
                write!(f, "exception names {c}, which is not in the publish plan")
            }
            Self::ExclusionWithoutReason(c) => {
                write!(f, "excluding {c} needs a non-empty reason")
            }
            Self::ExclusionForUnknownCrate(c) => {
                write!(f, "exclusion names {c}, which is not in the publish plan")
            }
            Self::Externals(e) => write!(f, "{e}"),
            Self::MissingExternals(v) => {
                write!(
                    f,
                    "dependencies outside the set are not on the registry; nothing was uploaded:"
                )?;
                for m in v {
                    write!(
                        f,
                        "
  {m}"
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for PipelineError {}

macro_rules! from_err {
    ($($t:ty => $v:ident),*) => {$(
        impl From<$t> for PipelineError { fn from(e: $t) -> Self { Self::$v(e) } }
    )*};
}
from_err!(SourceError => Source, CargoError => Cargo, PlanError => Plan, RunError => Run,
          LedgerError => Ledger, io::Error => Io, ExternalsError => Externals);

/// A commit archived and planned.
#[derive(Debug)]
pub struct Prepared {
    /// The `git archive` directory.
    pub dir: PathBuf,
    /// The publish plan.
    pub plan: Plan,
    /// The workspace metadata the plan came from.
    pub meta: Metadata,
}

/// Archive `sha` from `repo` into `work/src` and plan it.
///
/// # Errors
/// See [`PipelineError`].
pub fn prepare(
    cargo: &Cargo,
    repo: &Path,
    sha: &str,
    work: &Path,
) -> Result<Prepared, PipelineError> {
    let dir = work.join("src");
    source::archive(repo, sha, &dir)?;
    let meta = cargo.metadata(&dir)?;
    let plan = plan(&meta)?;
    Ok(Prepared { dir, plan, meta })
}

/// Package the whole plan in one cargo call.
///
/// # Errors
/// [`PipelineError::Cargo`].
pub fn package_all(
    cargo: &Cargo,
    prepared: &Prepared,
    target_dir: &Path,
) -> Result<Vec<PackagedCrate>, PipelineError> {
    let set: Vec<(String, Version)> = prepared
        .plan
        .order
        .iter()
        .map(|c| (c.name.clone(), c.version.clone()))
        .collect();
    let files = cargo.package_set(&prepared.dir, &set, target_dir)?;
    Ok(set
        .into_iter()
        .zip(files)
        .map(|((name, _), crate_file)| PackagedCrate { name, crate_file })
        .collect())
}

/// Builds a freshly created consumer project that depends on a published crate.
pub trait ConsumerCheck {
    /// `cargo add name@=version` into an empty project and `cargo check` it.
    ///
    /// # Errors
    /// Cargo's output if either step fails.
    fn check(&mut self, name: &str, version: &Version) -> Result<(), String>;
}

/// The real consumer check.
pub struct CargoAddCheck {
    /// The cargo to use.
    pub cargo: Cargo,
    /// A directory to create the throwaway project in.
    pub scratch: PathBuf,
}

impl ConsumerCheck for CargoAddCheck {
    fn check(&mut self, name: &str, version: &Version) -> Result<(), String> {
        let dir = self.scratch.join(format!("consumer-{name}-{version}"));
        let _ = fs::remove_dir_all(&dir);
        let io = |e: io::Error| e.to_string();
        fs::create_dir_all(dir.join("src")).map_err(io)?;
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"consumer-check\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[workspace]\n",
        )
        .map_err(io)?;
        fs::write(dir.join("src/lib.rs"), "").map_err(io)?;
        let spec = format!("{name}@={version}");
        for args in [vec!["add", spec.as_str()], vec!["check"]] {
            let out = self.cargo.run(&dir, &args).map_err(io)?;
            if !out.success {
                return Err(format!("`cargo {}` failed:\n{}", args.join(" "), out.text));
            }
        }
        Ok(())
    }
}

/// Inputs to [`publish`].
pub struct PublishArgs<'a> {
    /// The repository to publish from.
    pub repo: &'a Path,
    /// The full commit SHA.
    pub sha: &'a str,
    /// The ledger file.
    pub ledger: &'a Path,
    /// A scratch directory (created if missing).
    pub work: &'a Path,
    /// Run identifier for ledger events.
    pub run_id: &'a str,
    /// Run options (mode, waits).
    pub opts: Options,
    /// Recorded exceptions to verification.
    pub exceptions: Vec<Exception>,
    /// Crates left out of this run on purpose, each with its reason (recorded in the ledger).
    pub excluded: Vec<Exception>,
}

/// What a publish did.
#[derive(Debug)]
pub struct Summary {
    /// The run's per-crate outcomes.
    pub report: Report,
    /// Pre-upload verification result per verified crate.
    pub verified: Vec<(String, Result<(), String>)>,
    /// Consumer-check result per exception crate that became visible.
    pub consumer: Vec<(String, Result<(), String>)>,
    /// The plan that was run, without any excluded crates.
    pub plan: Plan,
}

impl Summary {
    /// The run succeeded and every consumer check passed.
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.report.is_success() && self.consumer.iter().all(|(_, r)| r.is_ok())
    }
}

fn record(
    ledger: &Ledger,
    clock: &mut dyn Clock,
    args: &PublishArgs<'_>,
    kind: EventKind,
    krate: Option<(&str, &Version)>,
    detail: &str,
) -> Result<(), LedgerError> {
    ledger.append(&Event {
        ts: clock.now_utc(),
        run: args.run_id.to_owned(),
        sha: args.sha.to_owned(),
        crate_name: krate.map(|(n, _)| n.to_owned()),
        version: krate.map(|(_, v)| v.to_string()),
        event: kind,
        detail: detail.to_owned(),
    })
}

/// Archive, plan, package, verify, run, and check exceptions as a consumer.
///
/// `make_registry` receives the archive directory and cargo's target directory. In dry-run mode
/// everything local still happens (packaging and verification are builds, not uploads) and the
/// registry is only asked questions.
///
/// # Errors
/// See [`PipelineError`]. A verification failure returns before the run starts.
pub fn publish(
    cargo: &Cargo,
    args: &PublishArgs<'_>,
    make_registry: impl FnOnce(&Path, &Path) -> Box<dyn Registry>,
    clock: &mut dyn Clock,
    consumer: &mut dyn ConsumerCheck,
) -> Result<Summary, PipelineError> {
    for e in &args.exceptions {
        if e.reason.trim().is_empty() {
            return Err(PipelineError::ExceptionWithoutReason(e.krate.clone()));
        }
    }
    for e in &args.excluded {
        if e.reason.trim().is_empty() {
            return Err(PipelineError::ExclusionWithoutReason(e.krate.clone()));
        }
    }
    fs::create_dir_all(args.work)?;
    let mut prepared = prepare(cargo, args.repo, args.sha, args.work)?;
    for e in &args.excluded {
        if !prepared.plan.order.iter().any(|c| c.name == e.krate) {
            return Err(PipelineError::ExclusionForUnknownCrate(e.krate.clone()));
        }
    }
    // Remember the versions before the crates leave the plan; the ledger names them.
    let excluded: Vec<(&Exception, Version)> = args
        .excluded
        .iter()
        .filter_map(|e| {
            let v = prepared.plan.order.iter().find(|c| c.name == e.krate)?;
            Some((e, v.version.clone()))
        })
        .collect();
    let names = args.excluded.iter().map(|e| e.krate.clone()).collect();
    prepared.plan = prepared.plan.without(&names);
    for e in &args.exceptions {
        if !prepared.plan.order.iter().any(|c| c.name == e.krate) {
            return Err(PipelineError::ExceptionForUnknownCrate(e.krate.clone()));
        }
    }
    let target_dir = args.work.join("target");
    let mut registry = make_registry(&prepared.dir, &target_dir);
    // Before anything is written or built: a dependency outside the set that the registry cannot
    // satisfy means the dependent cannot be published yet (it would fail at package or upload).
    let missing = missing_externals(&prepared.meta, &prepared.plan, registry.as_mut())?;
    if !missing.is_empty() {
        return Err(PipelineError::MissingExternals(missing));
    }
    let ledger = Ledger::open(args.ledger)?;
    let version_of = |name: &str| {
        prepared
            .plan
            .order
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.version.clone())
            .expect("checked above")
    };
    for (e, v) in &excluded {
        record(
            &ledger,
            clock,
            args,
            EventKind::Exception,
            Some((&e.krate, v)),
            &format!("excluded from this run: {}", e.reason),
        )?;
    }
    for e in &args.exceptions {
        let v = version_of(&e.krate);
        record(
            &ledger,
            clock,
            args,
            EventKind::Exception,
            Some((&e.krate, &v)),
            &format!("verification skipped: {}", e.reason),
        )?;
    }

    let packaged = package_all(cargo, &prepared, &target_dir)?;
    let skip: Vec<String> = args.exceptions.iter().map(|e| e.krate.clone()).collect();
    let verified = verify_set(cargo, &packaged, &args.work.join("verify"), &skip)?;
    let failed: Vec<(String, String)> = verified
        .iter()
        .filter_map(|(n, r)| r.as_ref().err().map(|why| (n.clone(), why.clone())))
        .collect();
    if !failed.is_empty() {
        return Err(PipelineError::VerifyFailed(failed));
    }
    for (n, _) in &verified {
        let v = version_of(n);
        record(&ledger, clock, args, EventKind::Verified, Some((n, &v)), "")?;
    }

    let ctx = RunContext {
        run_id: args.run_id.to_owned(),
        sha: args.sha.to_owned(),
    };
    let report = run(
        &prepared.plan,
        registry.as_mut(),
        clock,
        &ledger,
        &ctx,
        &args.opts,
    )?;

    let mut consumer_results = Vec::new();
    if args.opts.mode == Mode::Execute {
        for e in &args.exceptions {
            let visible = report
                .outcomes
                .iter()
                .any(|(n, o)| *n == e.krate && matches!(o, Outcome::Visible));
            if visible {
                let v = version_of(&e.krate);
                let r = consumer.check(&e.krate, &v);
                let detail = match &r {
                    Ok(()) => "consumer check passed".to_owned(),
                    Err(why) => format!("consumer check FAILED: {why}"),
                };
                record(
                    &ledger,
                    clock,
                    args,
                    EventKind::Verified,
                    Some((&e.krate, &v)),
                    &detail,
                )?;
                consumer_results.push((e.krate.clone(), r));
            }
        }
    }
    Ok(Summary {
        report,
        verified,
        consumer: consumer_results,
        plan: prepared.plan,
    })
}

/// Ask the registry about every crate in the plan (no uploads, no ledger).
pub fn readback(prepared: &Prepared, registry: &mut dyn Registry) -> Vec<(String, Lookup)> {
    prepared
        .plan
        .order
        .iter()
        .map(|c| (c.name.clone(), registry.lookup(&c.name, &c.version)))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::registry::fake::{FakeClock, FakeRegistry};

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn manifest(name: &str, deps: &str) -> String {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.1\"\nedition = \"2021\"\ndescription = \"probe\"\nlicense = \"MIT\"\n{deps}\n"
        )
    }

    /// A repo whose `b` depends on `a` (so the plan is a, b); `bad_b` makes `b` build only from the tree.
    fn repo(p: &str, bad_b: bool) -> (tempfile::TempDir, String) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        let (a, b) = (format!("{p}-a"), format!("{p}-b"));
        write(
            &r.join("Cargo.toml"),
            &format!("[workspace]\nresolver = \"2\"\nmembers = [\"{a}\", \"{b}\"]\n"),
        );
        write(&r.join(&a).join("Cargo.toml"), &manifest(&a, ""));
        write(&r.join(&a).join("src/lib.rs"), "");
        write(
            &r.join(&b).join("Cargo.toml"),
            &manifest(
                &b,
                &format!("[dependencies]\n{a} = {{ path = \"../{a}\", version = \"0.0.1\" }}"),
            ),
        );
        write(&r.join("outside.txt"), "outside the crate");
        write(
            &r.join(&b).join("src/lib.rs"),
            if bad_b {
                "pub const D: &str = include_str!(\"../../outside.txt\");\n"
            } else {
                ""
            },
        );
        git(r, &["init", "-q"]);
        git(r, &["add", "-A"]);
        git(r, &["commit", "-q", "-m", "one"]);
        let sha = git(r, &["rev-parse", "HEAD"]);
        (d, sha)
    }

    struct NoConsumer;
    impl ConsumerCheck for NoConsumer {
        fn check(&mut self, _: &str, _: &Version) -> Result<(), String> {
            panic!("no exceptions, so no consumer check")
        }
    }

    struct Scripted(Result<(), String>, Vec<String>);
    impl ConsumerCheck for Scripted {
        fn check(&mut self, name: &str, _: &Version) -> Result<(), String> {
            self.1.push(name.to_owned());
            self.0.clone()
        }
    }

    fn args<'a>(
        repo: &'a Path,
        sha: &'a str,
        ledger: &'a Path,
        work: &'a Path,
        mode: Mode,
        exceptions: Vec<Exception>,
    ) -> PublishArgs<'a> {
        PublishArgs {
            repo,
            sha,
            ledger,
            work,
            run_id: "t",
            opts: Options {
                mode,
                ..Options::default()
            },
            exceptions,
            excluded: vec![],
        }
    }

    fn excluding<'a>(mut a: PublishArgs<'a>, krate: &str, reason: &str) -> PublishArgs<'a> {
        a.excluded.push(Exception {
            krate: krate.to_owned(),
            reason: reason.to_owned(),
        });
        a
    }

    fn pid() -> String {
        format!("zzrlpipe{}", std::process::id())
    }

    #[test]
    fn publishes_in_dependency_order_from_the_archive_with_real_cargo() {
        let p = pid();
        let (repo, sha) = repo(&p, false);
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let uploads = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let u2 = std::rc::Rc::clone(&uploads);
        let s = publish(
            &Cargo::from_env(),
            &args(repo.path(), &sha, &ledger, &work, Mode::Execute, vec![]),
            |_, _| {
                let mut r = FakeRegistry::new();
                r.on_upload = Some(Box::new(move |n, _| u2.borrow_mut().push(n.to_owned())));
                Box::new(r)
            },
            &mut FakeClock::default(),
            &mut NoConsumer,
        )
        .unwrap();
        assert!(s.is_success(), "{s:?}");
        assert_eq!(*uploads.borrow(), [format!("{p}-a"), format!("{p}-b")]);
        assert_eq!(s.verified.len(), 2);
    }

    // The verification gate: nothing is uploaded and the ledger has no upload event.
    #[test]
    fn a_crate_that_only_builds_from_the_tree_blocks_every_upload() {
        let p = pid();
        let (repo, sha) = repo(&p, true);
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let err = publish(
            &Cargo::from_env(),
            &args(repo.path(), &sha, &ledger, &work, Mode::Execute, vec![]),
            |_, _| Box::new(FakeRegistry::new()),
            &mut FakeClock::default(),
            &mut NoConsumer,
        )
        .unwrap_err();
        match err {
            PipelineError::VerifyFailed(v) => assert_eq!(v[0].0, format!("{p}-b")),
            other => panic!("{other}"),
        }
        let text = fs::read_to_string(&ledger).unwrap();
        assert!(!text.contains("upload_started"), "{text}");
    }

    // A dependency outside the set that the registry lacks blocks everything before any write.
    #[test]
    fn unsatisfied_external_dependency_blocks_the_publish_before_any_write() {
        let p = pid();
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        let a = format!("{p}-a");
        write(
            &r.join("Cargo.toml"),
            &format!(
                "[workspace]
resolver = \"2\"
members = [\"{a}\"]
"
            ),
        );
        write(
            &r.join(&a).join("Cargo.toml"),
            &manifest(
                &a,
                "[dependencies]
zzrl-not-yet-published = \"0.23.0\"",
            ),
        );
        write(&r.join(&a).join("src/lib.rs"), "");
        git(r, &["init", "-q"]);
        git(r, &["add", "-A"]);
        git(r, &["commit", "-q", "-m", "one"]);
        let sha = git(r, &["rev-parse", "HEAD"]);
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let err = publish(
            &Cargo::from_env(),
            &args(r, &sha, &ledger, &work, Mode::Execute, vec![]),
            |_, _| Box::new(FakeRegistry::new()),
            &mut FakeClock::default(),
            &mut NoConsumer,
        )
        .unwrap_err();
        match err {
            PipelineError::MissingExternals(m) => {
                assert_eq!(m.len(), 1);
                assert_eq!(m[0].dependency, "zzrl-not-yet-published");
            }
            other => panic!("{other}"),
        }
        assert!(!ledger.exists(), "no ledger was created");
        assert!(!work.join("target").exists(), "nothing was packaged");
    }

    // T10: an exception needs a reason, is recorded, and the consumer check decides the outcome.
    #[test]
    fn exceptions_need_a_reason_are_recorded_and_are_consumer_checked() {
        let p = pid();
        let (repo, sha) = repo(&p, true);
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let b = format!("{p}-b");
        let go = |reason: &str, consumer: &mut dyn ConsumerCheck| {
            publish(
                &Cargo::from_env(),
                &args(
                    repo.path(),
                    &sha,
                    &ledger,
                    &work,
                    Mode::Execute,
                    vec![Exception {
                        krate: b.clone(),
                        reason: reason.to_owned(),
                    }],
                ),
                |_, _| Box::new(FakeRegistry::new()),
                &mut FakeClock::default(),
                consumer,
            )
        };
        assert!(matches!(
            go("  ", &mut NoConsumer),
            Err(PipelineError::ExceptionWithoutReason(_))
        ));
        let _ = fs::remove_dir_all(&work);

        // With a reason, the tree-only crate is allowed through (that is what an exception is) ...
        let mut ok = Scripted(Ok(()), vec![]);
        let s = go("needs the full CUDA environment", &mut ok).unwrap();
        assert!(s.is_success(), "{s:?}");
        assert_eq!(ok.1, std::slice::from_ref(&b));
        let text = fs::read_to_string(&ledger).unwrap();
        assert!(text.contains("verification skipped: needs the full CUDA environment"));

        // ... and a failing consumer check makes the whole publish unsuccessful.
        let _ = fs::remove_dir_all(&work);
        let _ = fs::remove_file(&ledger);
        let mut bad = Scripted(Err("does not build for a consumer".into()), vec![]);
        let s = go("needs the full CUDA environment", &mut bad).unwrap();
        assert!(s.report.is_success(), "the upload itself went fine");
        assert!(!s.is_success(), "but the consumer check failed");
    }

    #[test]
    fn exception_for_a_crate_not_in_the_plan_is_refused() {
        let p = pid();
        let (repo, sha) = repo(&p, false);
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let err = publish(
            &Cargo::from_env(),
            &args(
                repo.path(),
                &sha,
                &ledger,
                &work,
                Mode::DryRun,
                vec![Exception {
                    krate: "nope".into(),
                    reason: "x".into(),
                }],
            ),
            |_, _| Box::new(FakeRegistry::new()),
            &mut FakeClock::default(),
            &mut NoConsumer,
        )
        .unwrap_err();
        assert!(matches!(err, PipelineError::ExceptionForUnknownCrate(_)));
    }

    // An exclusion leaves the crate out of the whole run (nothing packaged, verified or uploaded for
    // it) and the ledger says who was left out and why. The first publish test is the negative control:
    // without the exclusion both crates go up.
    #[test]
    fn an_excluded_crate_is_left_out_and_the_ledger_records_why() {
        let p = pid();
        let (repo, sha) = repo(&p, false);
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let uploads = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let u2 = std::rc::Rc::clone(&uploads);
        let b = format!("{p}-b");
        let s = publish(
            &Cargo::from_env(),
            &excluding(
                args(repo.path(), &sha, &ledger, &work, Mode::Execute, vec![]),
                &b,
                "waits for the fuel release",
            ),
            |_, _| {
                let mut r = FakeRegistry::new();
                r.on_upload = Some(Box::new(move |n, _| u2.borrow_mut().push(n.to_owned())));
                Box::new(r)
            },
            &mut FakeClock::default(),
            &mut NoConsumer,
        )
        .unwrap();
        assert!(s.is_success(), "{s:?}");
        assert_eq!(*uploads.borrow(), [format!("{p}-a")]);
        assert_eq!(s.verified.len(), 1, "the excluded crate is not verified");
        assert!(s.report.outcomes.iter().all(|(n, _)| *n != b));
        let text = fs::read_to_string(&ledger).unwrap();
        assert!(
            text.contains("excluded from this run: waits for the fuel release"),
            "{text}"
        );
        assert!(text.contains(&b));
    }

    #[test]
    fn an_exclusion_needs_a_reason_and_a_crate_in_the_plan() {
        let p = pid();
        let (repo, sha) = repo(&p, false);
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let go = |krate: &str, reason: &str| {
            publish(
                &Cargo::from_env(),
                &excluding(
                    args(repo.path(), &sha, &ledger, &work, Mode::DryRun, vec![]),
                    krate,
                    reason,
                ),
                |_, _| Box::new(FakeRegistry::new()),
                &mut FakeClock::default(),
                &mut NoConsumer,
            )
            .unwrap_err()
        };
        assert!(matches!(
            go(&format!("{p}-b"), "  "),
            PipelineError::ExclusionWithoutReason(_)
        ));
        assert!(matches!(
            go("nope", "x"),
            PipelineError::ExclusionForUnknownCrate(_)
        ));
        assert!(!ledger.exists(), "refused before anything was written");
    }

    // Excluding a crate that another planned crate needs does not make it go away: the dependent
    // still needs it on the registry, and the run stops before writing or building if it is not.
    #[test]
    fn excluding_a_dependency_that_is_not_on_the_registry_stops_the_run() {
        let p = pid();
        let (repo, sha) = repo(&p, false);
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let err = publish(
            &Cargo::from_env(),
            &excluding(
                args(repo.path(), &sha, &ledger, &work, Mode::Execute, vec![]),
                &format!("{p}-a"),
                "held back",
            ),
            |_, _| Box::new(FakeRegistry::new()),
            &mut FakeClock::default(),
            &mut NoConsumer,
        )
        .unwrap_err();
        assert!(matches!(err, PipelineError::MissingExternals(_)), "{err}");
        assert!(!ledger.exists());
    }

    // Real consumer check against a crate that is on crates.io (needs network).
    #[test]
    fn cargo_add_check_passes_for_a_published_crate_and_fails_for_a_missing_version() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = CargoAddCheck {
            cargo: Cargo::from_env(),
            scratch: tmp.path().to_path_buf(),
        };
        assert_eq!(c.check("semver", &"1.0.0".parse().unwrap()), Ok(()));
        assert!(c.check("semver", &"99.0.0".parse().unwrap()).is_err());
    }
}
