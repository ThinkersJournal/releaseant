//! prepare -> package -> verify -> run -> consumer checks (DESIGN section 3).

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use semver::Version;

use crate::cargo::{Cargo, CargoError};
use crate::config::{self, Config, ConfigError};
use crate::externals::{ExternalsError, MissingExternal, missing_externals};
use crate::ledger::{Event, EventKind, Ledger, LedgerError};
use crate::metadata::Metadata;
use crate::plan::{Plan, PlanError, plan};
use crate::preflight::{self, Problem};
use crate::progress::{ProgressSink, observer};
use crate::registry::{Clock, Lookup, Registry};
use crate::run::{Mode, Options, Outcome, Report, RunContext, RunError, run};
use crate::source::{self, SourceError};
use crate::verify::{PackagedCrate, verify_set_with};

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
    /// Crates in the plan whose publish metadata is incomplete; nothing was uploaded.
    Metadata(Vec<Problem>),
    /// The commit's `releaseant.toml` cannot be used; nothing was uploaded.
    Config(ConfigError),
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
            Self::Config(e) => write!(f, "{e}"),
            Self::Metadata(v) => {
                write!(
                    f,
                    "publish metadata is incomplete for {} problem(s); nothing was uploaded. \
                     Fix these in the crates' Cargo.toml files and commit:",
                    v.len()
                )?;
                for p in v {
                    write!(f, "\n  {p}")?;
                }
                Ok(())
            }
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
          LedgerError => Ledger, io::Error => Io, ExternalsError => Externals,
          ConfigError => Config);

/// A commit archived and planned.
#[derive(Debug)]
pub struct Prepared {
    /// The `git archive` directory.
    pub dir: PathBuf,
    /// The publish plan.
    pub plan: Plan,
    /// The workspace metadata the plan came from.
    pub meta: Metadata,
    /// The commit's `releaseant.toml` (the default when it has none).
    pub config: Config,
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
    let config = config::load(&dir)?;
    Ok(Prepared {
        dir,
        plan,
        meta,
        config,
    })
}

/// Refuse a plan whose crates are missing metadata the registry refuses to publish without, naming
/// every error of every crate; the quality fields it accepts without come back as warnings, and
/// `require` (the command line's) together with the commit's `releaseant.toml` turn the named ones
/// into errors (see [`preflight`]). Run it while the archive directory exists: it also checks that
/// each readme and licence file is there.
///
/// # Errors
/// [`PipelineError::Metadata`] with all the errors.
pub fn check_metadata(
    prepared: &Prepared,
    require: &BTreeSet<String>,
) -> Result<Vec<Problem>, PipelineError> {
    let mut require = require.clone();
    require.extend(prepared.config.required());
    let report = preflight::problems(&prepared.meta, &prepared.plan, &require);
    if report.errors.is_empty() {
        Ok(report.warnings)
    } else {
        Err(PipelineError::Metadata(report.errors))
    }
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

/// Shows metadata warnings to the person running the command.
pub type WarningSink<'a> = &'a dyn Fn(&[Problem]);

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
    /// Metadata fields that are only warnings unless named here (see [`preflight::REQUIRABLE`]).
    pub require_metadata: BTreeSet<String>,
    /// Called with the metadata warnings as soon as they are known, before anything is built or
    /// uploaded, so a run that fails later has still shown them.
    pub on_warnings: Option<WarningSink<'a>>,
    /// Called with one line of text per step as the run goes (packaging, each verification, and
    /// a line for every ledger event), so a long run is not silent.
    pub on_progress: Option<ProgressSink>,
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
    /// Metadata problems that did not stop the run (quality fields the registry accepts without).
    pub warnings: Vec<Problem>,
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
    // Before anything else: a crate the registry would refuse at upload leaves a half-published set.
    let warnings = check_metadata(&prepared, &args.require_metadata)?;
    if let Some(show) = args.on_warnings {
        show(&warnings);
    }
    let target_dir = args.work.join("target");
    let mut registry = make_registry(&prepared.dir, &target_dir);
    // Before anything is written or built: a dependency outside the set that the registry cannot
    // satisfy means the dependent cannot be published yet (it would fail at package or upload).
    let missing = missing_externals(&prepared.meta, &prepared.plan, registry.as_mut())?;
    if !missing.is_empty() {
        return Err(PipelineError::MissingExternals(missing));
    }
    let mut ledger = Ledger::open(args.ledger)?;
    if let Some(sink) = &args.on_progress {
        ledger = ledger.observed_by(observer(sink.clone()));
    }
    let say = |line: String| {
        if let Some(sink) = &args.on_progress {
            sink(&line);
        }
    };
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

    say(format!("packaging {} crates", prepared.plan.order.len()));
    let packaged = package_all(cargo, &prepared, &target_dir)?;
    let skip: Vec<String> = args.exceptions.iter().map(|e| e.krate.clone()).collect();
    let verified = verify_set_with(
        cargo,
        &packaged,
        &args.work.join("verify"),
        &skip,
        &|name, i, total| say(format!("verifying {name} ({i} of {total})")),
    )?;
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
        warnings,
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

    /// A crate manifest with complete publish metadata (the readme file is the caller's to write).
    fn manifest(name: &str, deps: &str) -> String {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.1\"\nedition = \"2021\"\ndescription = \"probe\"\nlicense = \"MIT\"\nrepository = \"https://example.invalid/probe\"\nreadme = \"README.md\"\nkeywords = [\"probe\"]\ncategories = [\"development-tools\"]\nrust-version = \"1.70\"\n{deps}\n"
        )
    }

    /// A repo whose `b` depends on `a` (so the plan is a, b); `bad_b` makes `b` build only from the tree.
    fn repo(p: &str, bad_b: bool) -> (tempfile::TempDir, String) {
        repo_with(p, bad_b, |_, manifest| manifest)
    }

    /// [`repo`], with each crate's manifest passed through `tweak(crate name, manifest)` first.
    fn repo_with(
        p: &str,
        bad_b: bool,
        tweak: impl Fn(&str, String) -> String,
    ) -> (tempfile::TempDir, String) {
        repo_full(p, bad_b, tweak, &[])
    }

    /// [`repo_with`], also committing `extra` files at `(path, text)` (for example `releaseant.toml`).
    fn repo_full(
        p: &str,
        bad_b: bool,
        tweak: impl Fn(&str, String) -> String,
        extra: &[(&str, &str)],
    ) -> (tempfile::TempDir, String) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        let (a, b) = (format!("{p}-a"), format!("{p}-b"));
        write(
            &r.join("Cargo.toml"),
            &format!("[workspace]\nresolver = \"2\"\nmembers = [\"{a}\", \"{b}\"]\n"),
        );
        write(&r.join(&a).join("Cargo.toml"), &tweak(&a, manifest(&a, "")));
        write(&r.join(&a).join("src/lib.rs"), "");
        write(&r.join(&a).join("README.md"), "probe");
        write(
            &r.join(&b).join("Cargo.toml"),
            &tweak(
                &b,
                manifest(
                    &b,
                    &format!("[dependencies]\n{a} = {{ path = \"../{a}\", version = \"0.0.1\" }}"),
                ),
            ),
        );
        write(&r.join(&b).join("README.md"), "probe");
        write(&r.join("outside.txt"), "outside the crate");
        for (path, text) in extra {
            write(&r.join(path), text);
        }
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
            require_metadata: BTreeSet::new(),
            on_warnings: None,
            on_progress: None,
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

    // Every ledger event of the run produced one progress line (compared by kind, not a bare
    // count), and the package/verify stages announced themselves; with no sink the run is unchanged.
    #[test]
    fn progress_lines_mirror_the_ledger_and_cover_package_and_verify() {
        let p = pid();
        let (repo, sha) = repo(&p, false);
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let lines: std::rc::Rc<std::cell::RefCell<Vec<String>>> = std::rc::Rc::default();
        let l2 = std::rc::Rc::clone(&lines);
        let mut a = args(repo.path(), &sha, &ledger, &work, Mode::Execute, vec![]);
        a.on_progress = Some(std::rc::Rc::new(move |l: &str| {
            l2.borrow_mut().push(l.to_owned());
        }));
        let s = publish(
            &Cargo::from_env(),
            &a,
            |_, _| Box::new(FakeRegistry::new()),
            &mut FakeClock::default(),
            &mut NoConsumer,
        )
        .unwrap();
        assert!(s.is_success(), "{s:?}");
        let got = lines.borrow().clone();
        let (an, bn) = (format!("{p}-a"), format!("{p}-b"));
        assert_eq!(got[0], "packaging 2 crates", "{got:?}");
        assert_eq!(got[1], format!("verifying {an} (1 of 2)"));
        assert_eq!(got[2], format!("verifying {bn} (2 of 2)"));
        let events = Ledger::open(&ledger).unwrap().read().unwrap();
        let from_ledger: Vec<String> = events
            .iter()
            .map(|e| crate::progress::describe(e, None))
            .collect();
        let tail: Vec<String> = got[3..]
            .iter()
            .map(|l| {
                // upload durations are the only part the ledger cannot reproduce
                l.split(" (").next().unwrap().to_owned()
            })
            .collect();
        let want: Vec<String> = from_ledger
            .iter()
            .map(|l| l.split(" (").next().unwrap().to_owned())
            .collect();
        assert_eq!(tail, want, "one line per ledger event, in order");
        assert!(got.iter().any(|l| l == &format!("uploading {an} 0.0.1")));
        assert!(
            got.iter()
                .any(|l| l == &format!("{bn} 0.0.1: read back, visible"))
        );
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
        write(&r.join(&a).join("README.md"), "probe");
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

    // The mutation that started ROADMAP R6: delete `description` from one crate of a set (and the licence
    // from another) and the whole run stops before anything is written, naming every problem. The
    // control is `publishes_in_dependency_order_from_the_archive_with_real_cargo`, the same fixture
    // untouched.
    #[test]
    fn missing_metadata_stops_the_run_before_anything_is_written_and_names_every_problem() {
        let p = pid();
        let (a, b) = (format!("{p}-a"), format!("{p}-b"));
        let (repo, sha) = repo_with(&p, false, |name, manifest| {
            if name.ends_with("-a") {
                manifest.replace("description = \"probe\"\n", "")
            } else {
                manifest.replace("license = \"MIT\"\n", "")
            }
        });
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let uploads = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let u2 = std::rc::Rc::clone(&uploads);
        let err = publish(
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
        .unwrap_err();
        let text = err.to_string();
        match err {
            PipelineError::Metadata(ps) => {
                let got: Vec<(&str, &str)> =
                    ps.iter().map(|p| (p.krate.as_str(), p.field)).collect();
                assert_eq!(got, [(a.as_str(), "description"), (b.as_str(), "license")]);
            }
            other => panic!("expected Metadata, got {other}"),
        }
        assert!(text.contains("nothing was uploaded"), "{text}");
        assert!(text.contains("description = "), "says what to add: {text}");
        assert!(uploads.borrow().is_empty());
        assert!(!ledger.exists(), "no ledger was created");
        assert!(!work.join("target").exists(), "nothing was packaged");
    }

    // The warnings reach the caller before the first build, not only in the summary at the end: the
    // callback sees the scratch `target` directory absent, and a run that fails afterwards (the unknown
    // exception crate) has still shown them.
    #[test]
    fn warnings_are_shown_before_anything_is_built() {
        let p = pid();
        let (repo, sha) = repo_with(&p, false, |_, manifest| {
            manifest.replace(
                "keywords = [\"probe\"]
",
                "",
            )
        });
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let seen = std::cell::RefCell::new(Vec::new());
        let show = |ws: &[Problem]| {
            seen.borrow_mut()
                .push((ws.len(), work.join("target").exists()))
        };
        let mut a = args(repo.path(), &sha, &ledger, &work, Mode::DryRun, vec![]);
        a.on_warnings = Some(&show);
        let s = publish(
            &Cargo::from_env(),
            &a,
            |_, _| Box::new(FakeRegistry::new()),
            &mut FakeClock::default(),
            &mut NoConsumer,
        )
        .unwrap();
        assert_eq!(*seen.borrow(), [(s.warnings.len(), false)]);
        assert!(s.warnings.len() >= 2, "the control: warnings exist");
    }

    // Quality fields the registry accepts without (here keywords and rust-version) are warnings: the run
    // goes ahead and the summary lists them. Required, the same crates stop the run before a ledger exists.
    #[test]
    fn missing_quality_fields_are_warnings_unless_required() {
        let p = pid();
        let (a, b) = (format!("{p}-a"), format!("{p}-b"));
        let (repo, sha) = repo_with(&p, false, |_, manifest| {
            manifest
                .replace("keywords = [\"probe\"]\n", "")
                .replace("rust-version = \"1.70\"\n", "")
        });
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        let run = |ledger: &Path, work: &Path, require: &[&str]| {
            let mut a = args(repo.path(), &sha, ledger, work, Mode::DryRun, vec![]);
            a.require_metadata = require.iter().map(|s| (*s).to_owned()).collect();
            publish(
                &Cargo::from_env(),
                &a,
                |_, _| Box::new(FakeRegistry::new()),
                &mut FakeClock::default(),
                &mut NoConsumer,
            )
        };

        let s = run(&ledger, &work, &[]).unwrap();
        let got: Vec<(&str, &str)> = s
            .warnings
            .iter()
            .map(|w| (w.krate.as_str(), w.field))
            .collect();
        assert_eq!(
            got,
            [
                (a.as_str(), "keywords"),
                (a.as_str(), "rust-version"),
                (b.as_str(), "keywords"),
                (b.as_str(), "rust-version")
            ]
        );
        assert_eq!(s.report.outcomes.len(), 2, "both crates were planned");

        let (ledger2, work2) = (tmp.path().join("l2.jsonl"), tmp.path().join("w2"));
        match run(&ledger2, &work2, &["keywords"]) {
            Err(PipelineError::Metadata(ps)) => {
                let got: Vec<(&str, &str)> =
                    ps.iter().map(|p| (p.krate.as_str(), p.field)).collect();
                assert_eq!(got, [(a.as_str(), "keywords"), (b.as_str(), "keywords")]);
                assert!(ps[0].detail.contains("--require-metadata"), "{}", ps[0]);
            }
            other => panic!("expected Metadata, got {other:?}"),
        }
        assert!(!ledger2.exists(), "nothing was written");
    }

    /// A dry-run publish of `repo` at `sha` with `require` as the command line's `--require-metadata`.
    fn publish_dry(
        repo: &Path,
        sha: &str,
        ledger: &Path,
        work: &Path,
        require: &[&str],
    ) -> Result<Summary, PipelineError> {
        let mut a = args(repo, sha, ledger, work, Mode::DryRun, vec![]);
        a.require_metadata = require.iter().map(|s| (*s).to_owned()).collect();
        publish(
            &Cargo::from_env(),
            &a,
            |_, _| Box::new(FakeRegistry::new()),
            &mut FakeClock::default(),
            &mut NoConsumer,
        )
    }

    fn problem_fields(err: PipelineError) -> (Vec<(String, &'static str)>, String) {
        let text = err.to_string();
        match err {
            PipelineError::Metadata(ps) => (
                ps.iter().map(|p| (p.krate.clone(), p.field)).collect(),
                text,
            ),
            other => panic!("expected Metadata, got {other}"),
        }
    }

    // A `releaseant.toml` at the root of the commit makes chosen warnings errors, and the command line's
    // flag adds to it. The control is `missing_quality_fields_are_warnings_unless_required`: the same
    // crates without the file only warn.
    #[test]
    fn a_releaseant_toml_in_the_commit_makes_chosen_warnings_errors_and_the_flag_adds_to_it() {
        let p = pid();
        let (a, b) = (format!("{p}-a"), format!("{p}-b"));
        let (repo, sha) = repo_full(
            &p,
            false,
            |_, manifest| {
                manifest
                    .replace("keywords = [\"probe\"]\n", "")
                    .replace("rust-version = \"1.70\"\n", "")
            },
            &[("releaseant.toml", "[metadata]\nrequire = [\"keywords\"]\n")],
        );
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));

        let err = publish_dry(repo.path(), &sha, &ledger, &work, &[]).unwrap_err();
        let (fields, text) = problem_fields(err);
        assert_eq!(
            fields,
            [(a.clone(), "keywords"), (b.clone(), "keywords")],
            "keywords is required by the file; rust-version is still only a warning"
        );
        assert!(
            text.contains("releaseant.toml"),
            "says where it came from: {text}"
        );
        assert!(!ledger.exists(), "nothing was written");

        // The flag adds to the file, never replaces it.
        let (ledger2, work2) = (tmp.path().join("l2.jsonl"), tmp.path().join("w2"));
        let err = publish_dry(repo.path(), &sha, &ledger2, &work2, &["rust-version"]).unwrap_err();
        let (fields, _) = problem_fields(err);
        assert_eq!(
            fields,
            [
                (a.clone(), "keywords"),
                (a, "rust-version"),
                (b.clone(), "keywords"),
                (b, "rust-version")
            ]
        );
    }

    #[test]
    fn a_malformed_releaseant_toml_stops_the_run_before_anything_is_written() {
        let p = pid();
        let (repo, sha) = repo_full(
            &p,
            false,
            |_, manifest| manifest,
            &[("releaseant.toml", "[metadata]\nrequired = [\"readme\"]\n")],
        );
        let tmp = tempfile::tempdir().unwrap();
        let (ledger, work) = (tmp.path().join("l.jsonl"), tmp.path().join("w"));
        match publish_dry(repo.path(), &sha, &ledger, &work, &[]) {
            Err(PipelineError::Config(e)) => {
                assert!(e.to_string().starts_with("releaseant.toml:"), "{e}");
                assert!(e.to_string().contains("required"), "{e}");
            }
            other => panic!("expected Config, got {other:?}"),
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
