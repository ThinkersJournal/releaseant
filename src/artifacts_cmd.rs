//! `publish --artifacts-only`: stage and confirm the declared artifacts of a commit without touching
//! the crates (slice 2a of the CLI wiring of [`crate::release::release`]).
//!
//! Also the mixed release from `publish` (slice 2b): a real `publish` of a commit that declares
//! artifacts runs [`Mixed`]'s steps around the crates' run, in the order of
//! `docs/ARTIFACTS-DESIGN.md` section 7 (stage, crates, promote, project tag, publish the draft).

use std::cell::RefCell;
use std::error::Error;
use std::path::Path;
use std::time::Duration;

use semver::Version;

use crate::artifact::{self, Artifact};
use crate::cargo::Cargo;
use crate::clock::SystemClock;
use crate::gh_forge::GhForge;
use crate::images::Crane;
use crate::ledger::Ledger;
use crate::metadata::Package;
use crate::pipeline::{CrateStepHooks, prepare};
use crate::plan::Plan;
use crate::registry::{Lookup, Registry, Upload};
use crate::release::{
    Inputs, ReleaseReport, Select, Services, promote_artifacts, publish_release,
    record_declarations, release, stage_artifacts,
};
use crate::release_repo;
use crate::run::{Mode, Options, RunContext};
use crate::stage::Ctx;
use crate::tag::{release_tag, release_version};

/// What `--artifacts-only` needs.
pub struct Request<'a> {
    /// The git repository.
    pub repo: &'a Path,
    /// The commit (full SHA).
    pub sha: &'a str,
    /// The ledger file.
    pub ledger: &'a Path,
    /// Where CI put the artifacts (`--artifacts-dir`).
    pub artifacts_dir: Option<&'a Path>,
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

/// The flags a real mixed release needs: the project tag (the draft release is published onto it)
/// and a whole plan (a partial release has neither tag nor release).
///
/// # Errors
/// The named refusal.
pub fn check_mixed_flags(tag: bool, partial: bool) -> Result<(), String> {
    if !tag {
        return Err("this commit declares [[artifact]]: a release that publishes them needs --tag (the draft release is published onto the project tag), or say --crates-only".into());
    }
    if partial {
        return Err("this commit declares [[artifact]]: a release with --only or --exclude is partial and cannot publish them; say --crates-only".into());
    }
    Ok(())
}

/// Where a commit's artifacts go.
struct Target {
    artifacts: Vec<Artifact>,
    slug: String,
    tag: String,
}

/// Read the declarations of `sha`, resolve the GitHub repository and the release tag, and say so.
/// `Ok(None)` when the commit declares no artifact.
fn resolve_target(
    cargo: &Cargo,
    repo: &Path,
    sha: &str,
    repo_override: Option<&str>,
    tag_version: Option<&Version>,
) -> Result<Option<Target>, Box<dyn Error>> {
    let id = crate::cli::run_id(sha);
    let work = crate::cli::work_dir(&id);
    let prepared = prepare(cargo, repo, sha, &work);
    let _ = std::fs::remove_dir_all(&work);
    let prepared = prepared?;
    let artifacts: Vec<Artifact> = prepared.config.artifact.clone();
    if artifacts.is_empty() {
        return Ok(None);
    }
    let slug = release_repo::resolve(
        &plan_packages(&prepared.meta.packages, &prepared.plan),
        repo_override,
    )?;
    let version = release_version(&prepared.plan, tag_version)?;
    let tag = release_tag(&version);
    println!("artifacts go to GitHub release {tag} of {slug}");
    for a in &artifacts {
        println!("{}", artifact::describe(a));
    }
    Ok(Some(Target {
        artifacts,
        slug,
        tag,
    }))
}

/// The artifact steps of a mixed release, run by [`crate::pipeline::publish`] around its crates.
pub struct Mixed {
    target: Target,
    plan: Plan,
    dir: std::path::PathBuf,
    ledger: std::path::PathBuf,
    forge: RefCell<GhForge>,
    images: RefCell<Crane>,
}

impl Mixed {
    /// The steps for `sha`, or `None` when it declares no artifact. Without `execute` this only
    /// reports where the artifacts would go; with it, `artifacts_dir` must be given.
    ///
    /// # Errors
    /// An unnamed or conflicting repository, an ambiguous version, a missing `--artifacts-dir`.
    pub fn prepare(cargo: &Cargo, req: &Request<'_>) -> Result<Option<Self>, Box<dyn Error>> {
        let Some(target) =
            resolve_target(cargo, req.repo, req.sha, req.repo_override, req.tag_version)?
        else {
            return Ok(None);
        };
        let dir = match req.artifacts_dir {
            Some(d) => d.to_path_buf(),
            None if req.execute => {
                return Err(
                    "this commit declares [[artifact]]: say --artifacts-dir <dir> (where CI put them), or --crates-only to publish the crates alone"
                        .into(),
                );
            }
            None => std::path::PathBuf::new(),
        };
        let forge = RefCell::new(GhForge::new(&target.slug));
        Ok(Some(Self {
            target,
            plan: Plan { order: Vec::new() },
            dir,
            ledger: req.ledger.to_path_buf(),
            forge,
            images: RefCell::new(Crane::from_env()),
        }))
    }

    /// The release tag the draft release sits on.
    #[must_use]
    pub fn tag(&self) -> &str {
        &self.target.tag
    }

    fn inputs(&self) -> Inputs<'_> {
        Inputs {
            plan: &self.plan,
            artifacts: &self.target.artifacts,
            artifacts_dir: &self.dir,
            tag: &self.target.tag,
            select: Select::Both,
        }
    }

    /// The last step: publish the draft release onto the project tag. Call only once the tag is
    /// created and read back.
    ///
    /// # Errors
    /// Why the draft release was not published.
    pub fn finish(&self) -> Result<(), String> {
        let mut rep = ReleaseReport::default();
        if publish_release(
            &self.target.artifacts,
            &mut *self.forge.borrow_mut(),
            &self.target.tag,
            &mut rep,
        ) {
            if rep.published {
                println!("published the draft release {}", self.target.tag);
            }
            Ok(())
        } else {
            Err(rep.stopped.unwrap_or_default())
        }
    }
}

impl CrateStepHooks for Mixed {
    fn before_crates(&self, ctx: &RunContext) -> Result<(), String> {
        let ledger = Ledger::open(&self.ledger).map_err(|e| e.to_string())?;
        let mut clock = SystemClock;
        record_declarations(&self.target.artifacts, &ledger, &mut clock, ctx)
            .map_err(|e| e.to_string())?;
        let mut rep = ReleaseReport::default();
        let ok = stage_artifacts(
            &self.inputs(),
            &mut *self.forge.borrow_mut(),
            &mut *self.images.borrow_mut(),
            &ledger,
            &mut clock,
            &Ctx {
                run_id: ctx.run_id.clone(),
                sha: ctx.sha.clone(),
            },
            &mut rep,
        )
        .map_err(|e| e.to_string())?;
        print_report(&rep);
        if ok {
            Ok(())
        } else {
            Err(rep.stopped.unwrap_or_default())
        }
    }

    fn after_crates(&self, ctx: &RunContext) -> Result<(), String> {
        let ledger = Ledger::open(&self.ledger).map_err(|e| e.to_string())?;
        let mut rep = ReleaseReport::default();
        let ok = promote_artifacts(
            &self.inputs(),
            &mut *self.images.borrow_mut(),
            &ledger,
            &mut SystemClock,
            &Ctx {
                run_id: ctx.run_id.clone(),
                sha: ctx.sha.clone(),
            },
            &mut rep,
        )
        .map_err(|e| e.to_string())?;
        print_report(&rep);
        if ok {
            Ok(())
        } else {
            Err(rep.stopped.unwrap_or_default())
        }
    }
}

/// Run `publish --artifacts-only`. `Ok(true)` when everything asked for is done (or, without
/// `--execute`, when the plan is sound).
///
/// # Errors
/// What prevents the release from starting: no declared artifacts, an unnamed or conflicting
/// repository, an unreadable ledger, an ambiguous version.
pub fn artifacts_only(cargo: &Cargo, req: &Request<'_>) -> Result<bool, Box<dyn Error>> {
    let id = crate::cli::run_id(req.sha);
    let Some(Target {
        artifacts,
        slug,
        tag,
    }) = resolve_target(cargo, req.repo, req.sha, req.repo_override, req.tag_version)?
    else {
        return Err(
            "--artifacts-only: this commit's releaseant.toml declares no [[artifact]]".into(),
        );
    };
    let artifacts_dir = req
        .artifacts_dir
        .ok_or("--artifacts-only needs --artifacts-dir")?;
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
            artifacts_dir,
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
