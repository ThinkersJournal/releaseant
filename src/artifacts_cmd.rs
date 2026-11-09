//! `publish --artifacts-only`: stage and confirm the declared artifacts of a commit without touching
//! the crates (slice 2a of the CLI wiring of [`crate::release::release`]).
//!
//! The mixed release (crates and artifacts in the order of `docs/ARTIFACTS-DESIGN.md` section 7) is not
//! reachable from the command line yet; until it is, a real `publish` of a commit that declares
//! artifacts must say `--crates-only` or `--artifacts-only` rather than silently leave them out.

use std::error::Error;
use std::path::Path;
use std::time::Duration;

use semver::Version;

use crate::artifact::{self, Artifact};
use crate::cargo::Cargo;
use crate::clock::SystemClock;
use crate::config;
use crate::gh_forge::GhForge;
use crate::images::Crane;
use crate::ledger::Ledger;
use crate::metadata::Package;
use crate::pipeline::prepare;
use crate::plan::Plan;
use crate::registry::{Lookup, Registry, Upload};
use crate::release::{Inputs, ReleaseReport, Select, Services, release};
use crate::release_repo;
use crate::run::{Mode, Options, RunContext};
use crate::tag::{release_tag, release_version};

/// What `--artifacts-only` needs.
pub struct Request<'a> {
    /// The git repository.
    pub repo: &'a Path,
    /// The commit (full SHA).
    pub sha: &'a str,
    /// The ledger file.
    pub ledger: &'a Path,
    /// Where CI put the artifacts.
    pub artifacts_dir: &'a Path,
    /// `--artifacts-repo`.
    pub repo_override: Option<&'a str>,
    /// `--tag-version`.
    pub tag_version: Option<&'a Version>,
    /// Actually upload.
    pub execute: bool,
    /// `--max-wait`, seconds.
    pub max_wait: Option<u64>,
}

/// A crate registry that must never be asked: an artifacts-only release has no crate step.
struct NoCrates;

impl Registry for NoCrates {
    fn lookup(&mut self, _: &str, _: &Version) -> Lookup {
        Lookup::Unknown("an artifacts-only release asked the crate registry".into())
    }
    fn upload(&mut self, _: &str, _: &Version) -> Upload {
        Upload::Permanent("an artifacts-only release asked the crate registry to upload".into())
    }
    fn versions(&mut self, _: &str) -> Result<Vec<(Version, bool)>, String> {
        Err("an artifacts-only release asked the crate registry".into())
    }
}

/// The workspace packages named by the plan: whose `repository` decides where the artifacts go.
fn plan_packages<'a>(all: &'a [Package], plan: &Plan) -> Vec<&'a Package> {
    all.iter()
        .filter(|p| plan.order.iter().any(|c| c.name == p.name))
        .collect()
}

/// Run `publish --artifacts-only`. `Ok(true)` when everything asked for is done (or, without
/// `--execute`, when the plan is sound).
///
/// # Errors
/// What prevents the release from starting: no declared artifacts, an unnamed or conflicting
/// repository, an unreadable ledger, an ambiguous version.
pub fn artifacts_only(cargo: &Cargo, req: &Request<'_>) -> Result<bool, Box<dyn Error>> {
    let id = crate::cli::run_id(req.sha);
    let work = crate::cli::work_dir(&id);
    let prepared = prepare(cargo, req.repo, req.sha, &work);
    let _ = std::fs::remove_dir_all(&work);
    let prepared = prepared?;
    let artifacts: Vec<Artifact> = prepared.config.artifact.clone();
    if artifacts.is_empty() {
        return Err(
            "--artifacts-only: this commit's releaseant.toml declares no [[artifact]]".into(),
        );
    }
    let slug = release_repo::resolve(
        &plan_packages(&prepared.meta.packages, &prepared.plan),
        req.repo_override,
    )?;
    let version = release_version(&prepared.plan, req.tag_version)?;
    let tag = release_tag(&version);
    println!("artifacts go to GitHub release {tag} of {slug}");
    for a in &artifacts {
        println!("{}", artifact::describe(a));
    }
    if !req.execute {
        println!("dry run: nothing was uploaded (use --execute)");
        return Ok(true);
    }

    let ledger = Ledger::open(req.ledger)?;
    let ctx = RunContext {
        run_id: id,
        sha: req.sha.to_owned(),
    };
    let mut opts = Options {
        mode: Mode::Execute,
        ..Options::default()
    };
    if let Some(s) = req.max_wait {
        opts.max_wait = Duration::from_secs(s);
    }
    let mut forge = GhForge::new(&slug);
    let mut images = Crane::from_env();
    let mut registry = NoCrates;
    let mut clock = SystemClock;
    let report = release(
        &Inputs {
            plan: &Plan { order: Vec::new() },
            artifacts: &artifacts,
            artifacts_dir: req.artifacts_dir,
            tag: &tag,
            select: Select::ArtifactsOnly,
        },
        &mut Services {
            forge: &mut forge,
            images: &mut images,
            registry: &mut registry,
            clock: &mut clock,
        },
        &ledger,
        &ctx,
        &opts,
        &mut || Err("an artifacts-only release creates no project tag".into()),
    )?;
    print_report(&report);
    println!(
        "ledger: {}",
        req.ledger.display().to_string().replace('\\', "/")
    );
    Ok(report.is_success(Select::ArtifactsOnly, true))
}

fn print_report(rep: &ReleaseReport) {
    for (n, o) in rep.files.outcomes.iter().chain(&rep.images.outcomes) {
        println!("{n}: {o:?}");
    }
    for (n, o) in &rep.promoted.outcomes {
        println!("{n} (promoted): {o:?}");
    }
    for why in [
        &rep.files.stopped,
        &rep.images.stopped,
        &rep.promoted.stopped,
    ]
    .into_iter()
    .flatten()
    {
        println!("stopped (resumable): {why}");
    }
    if let Some(why) = &rep.stopped {
        println!("run stopped (resumable): {why}");
    }
}

/// The declared artifacts of `sha`'s `releaseant.toml`, read straight from git.
fn declared(repo: &Path, sha: &str) -> Result<Vec<Artifact>, Box<dyn Error>> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("show")
        .arg(format!("{sha}:{}", config::FILE))
        .output()?;
    if !out.status.success() {
        // No such file at that commit: nothing is declared. (A bad SHA fails later, in the pipeline.)
        return Ok(Vec::new());
    }
    Ok(config::parse(&String::from_utf8_lossy(&out.stdout))?.artifact)
}

/// A real `publish` that is neither `--crates-only` nor `--artifacts-only`, of a commit that declares
/// artifacts, would publish the crates and leave the artifacts out without a word. Refuse it.
///
/// # Errors
/// The named refusal, or an unreadable manifest.
pub fn refuse_unwired_mixed(repo: &Path, sha: &str) -> Result<(), Box<dyn Error>> {
    let n = declared(repo, sha)?.len();
    if n == 0 {
        return Ok(());
    }
    Err(format!(
        "this commit declares {n} [[artifact]]; the mixed release is not wired to `publish` yet. Say --crates-only (the crates only, artifacts left alone) or --artifacts-only --artifacts-dir <dir>"
    )
    .into())
}
