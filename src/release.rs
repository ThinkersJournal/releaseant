//! A mixed release: crates and artifacts together, in the order of `docs/ARTIFACTS-DESIGN.md`
//! section 7 (slice A6, part 1: the orchestration, tested against fakes).
//!
//! ```text
//! stage artifacts (draft release, images pushed by digest, NOTHING public)
//!   -> publish crates (irreversible)
//!   -> promote images (tags)
//!   -> project tag
//!   -> publish the draft release onto that tag
//! ```
//!
//! Everything that can fail cheaply (size, auth, a held name, a layout that differs from its
//! declaration) fails BEFORE the irreversible crates step; and a step that did not finish stops the
//! sequence, so the project tag only ever exists after every crate and every artifact is confirmed
//! (DESIGN section 15, widened). A dry run touches nothing but the crates' read-only lookups.

use std::fmt;
use std::path::Path;

use crate::artifact::{self, Artifact};
use crate::images::{ImageRegistry, promote_images, stage_images};
use crate::ledger::{Event, EventKind, Ledger, LedgerError};
use crate::plan::Plan;
use crate::registry::{Clock, Registry};
use crate::run::{self, Mode, Options, RunContext, RunError};
use crate::stage::{Ctx, Forge, StageReport, stage_files};

/// The ledger subject of the artifact declarations' own `Planned` event (not a crate name: a crate
/// name cannot contain parentheses).
pub const ARTIFACTS_SUBJECT: &str = "(artifacts)";

/// What to publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Select {
    /// Crates and artifacts: the whole release.
    Both,
    /// Only the crates (today's behaviour).
    CratesOnly,
    /// Only the artifacts; no crate is published and there is no project tag.
    ArtifactsOnly,
}

/// Everything a release needs besides the services.
pub struct Inputs<'a> {
    /// The crates to publish.
    pub plan: &'a Plan,
    /// The declared artifacts.
    pub artifacts: &'a [Artifact],
    /// The directory CI put the artifacts in.
    pub artifacts_dir: &'a Path,
    /// The git tag of the release (`v<version>`), also the GitHub release's tag.
    pub tag: &'a str,
    /// Which parts to publish.
    pub select: Select,
}

/// How far a release got.
#[derive(Debug, Default)]
pub struct ReleaseReport {
    /// Staging of `file` artifacts.
    pub files: StageReport,
    /// Staging of `image` artifacts.
    pub images: StageReport,
    /// The crates' run, when it happened.
    pub crates: Option<run::Report>,
    /// Promotion of images (tags).
    pub promoted: StageReport,
    /// The project tag step ran and succeeded.
    pub tagged: bool,
    /// The draft release was published.
    pub published: bool,
    /// The first reason the sequence stopped, if it did. Resumable.
    pub stopped: Option<String>,
}

impl ReleaseReport {
    /// Whether everything that was asked for is done and confirmed.
    #[must_use]
    pub fn is_success(&self, select: Select, execute: bool) -> bool {
        execute
            && self.stopped.is_none()
            && self.files.all_ok()
            && self.images.all_ok()
            && self.promoted.all_ok()
            && match select {
                Select::ArtifactsOnly => true,
                Select::CratesOnly => self.crates.as_ref().is_some_and(run::Report::is_success),
                Select::Both => {
                    self.tagged && self.crates.as_ref().is_some_and(run::Report::is_success)
                }
            }
    }
}

/// Why a release could not be carried out.
#[derive(Debug)]
pub enum ReleaseError {
    /// The ledger could not be read or written.
    Ledger(LedgerError),
    /// The crates' run could not start.
    Run(RunError),
    /// The artifact declarations differ from the ones this commit's earlier run recorded.
    ArtifactsChanged {
        /// What the ledger recorded.
        recorded: String,
        /// What was declared now.
        now: String,
    },
}

impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ledger(e) => write!(f, "{e}"),
            Self::Run(e) => write!(f, "{e}"),
            Self::ArtifactsChanged { recorded, now } => write!(
                f,
                "the artifact declarations changed since this commit's earlier run (recorded {recorded:?}, now {now:?}); a changed declaration is a new release"
            ),
        }
    }
}

impl std::error::Error for ReleaseError {}

impl From<LedgerError> for ReleaseError {
    fn from(e: LedgerError) -> Self {
        Self::Ledger(e)
    }
}

impl From<RunError> for ReleaseError {
    fn from(e: RunError) -> Self {
        Self::Run(e)
    }
}

/// The services a release talks to.
pub struct Services<'a> {
    /// The forge holding the release assets.
    pub forge: &'a mut dyn Forge,
    /// The image registry.
    pub images: &'a mut dyn ImageRegistry,
    /// The crate registry.
    pub registry: &'a mut dyn Registry,
    /// Sleeps and timestamps.
    pub clock: &'a mut dyn Clock,
}

/// Fix the artifact declarations for this commit: the first run records them, a later run of the
/// same commit must declare the same ones.
///
/// # Errors
/// [`ReleaseError::ArtifactsChanged`], or the ledger failing.
pub fn record_declarations(
    artifacts: &[Artifact],
    ledger: &Ledger,
    clock: &mut dyn Clock,
    ctx: &RunContext,
) -> Result<(), ReleaseError> {
    let now = artifact::fingerprint(artifacts);
    let earlier = ledger.read()?;
    let recorded = earlier.iter().rev().find(|e| {
        e.event == EventKind::Planned
            && e.crate_name.as_deref() == Some(ARTIFACTS_SUBJECT)
            && e.sha == ctx.sha
    });
    match recorded {
        Some(e) if e.detail != now => Err(ReleaseError::ArtifactsChanged {
            recorded: e.detail.clone(),
            now,
        }),
        Some(_) => Ok(()),
        None => Ok(ledger.append(&Event {
            ts: clock.now_utc(),
            run: ctx.run_id.clone(),
            sha: ctx.sha.clone(),
            crate_name: Some(ARTIFACTS_SUBJECT.to_owned()),
            version: None,
            event: EventKind::Planned,
            detail: now,
        })?),
    }
}

/// Step 1: stage artifacts (reversible, so everything that can fail cheaply fails here). Returns
/// whether every artifact is confirmed on its target; when not, `rep.stopped` says so.
///
/// # Errors
/// The ledger failing.
pub fn stage_artifacts(
    inp: &Inputs<'_>,
    forge: &mut dyn Forge,
    images: &mut dyn ImageRegistry,
    ledger: &Ledger,
    clock: &mut dyn Clock,
    sctx: &Ctx,
    rep: &mut ReleaseReport,
) -> Result<bool, LedgerError> {
    rep.files = stage_files(
        forge,
        ledger,
        clock,
        sctx,
        inp.tag,
        inp.artifacts_dir,
        inp.artifacts,
    )?;
    // A file that failed stops the staging here: nothing of the other kind is pushed either.
    if rep.files.all_ok() {
        rep.images = stage_images(
            images,
            ledger,
            clock,
            sctx,
            inp.artifacts_dir,
            inp.artifacts,
        )?;
    }
    if !rep.files.all_ok() || !rep.images.all_ok() {
        rep.stopped = Some(
            "an artifact is not confirmed on its target; no crate was published and nothing was tagged"
                .into(),
        );
        return Ok(false);
    }
    Ok(true)
}

/// Step 3: promote images, the user-visible commit point of an image. Returns whether every image
/// is tagged; when not, `rep.stopped` says so.
///
/// # Errors
/// The ledger failing.
pub fn promote_artifacts(
    inp: &Inputs<'_>,
    images: &mut dyn ImageRegistry,
    ledger: &Ledger,
    clock: &mut dyn Clock,
    sctx: &Ctx,
    rep: &mut ReleaseReport,
) -> Result<bool, LedgerError> {
    rep.promoted = promote_images(
        images,
        ledger,
        clock,
        sctx,
        inp.artifacts_dir,
        inp.artifacts,
    )?;
    if !rep.promoted.all_ok() {
        rep.stopped = Some("an image could not be tagged; no project tag was created".into());
        return Ok(false);
    }
    Ok(true)
}

/// Step 5: publish the draft release onto the project tag. A declaration without a `file` artifact
/// has no draft, and this does nothing. Returns whether the release is published (or none was due).
pub fn publish_release(
    artifacts: &[Artifact],
    forge: &mut dyn Forge,
    tag: &str,
    rep: &mut ReleaseReport,
) -> bool {
    if !artifacts.iter().any(|a| a.kind == artifact::Kind::File) {
        return true;
    }
    if let Err(why) = forge.publish_draft(tag) {
        rep.stopped = Some(format!(
            "the tag exists but the draft release was not published: {why}"
        ));
        return false;
    }
    rep.published = true;
    true
}

/// Carry out a release in the order above.
///
/// `tag_step` creates the project tag (annotated, on the commit, pushed and read back, as
/// `crate::tag` does); it is called only when every crate and every artifact is confirmed and the
/// selection is `Both`, and the draft release is published only after it succeeds.
///
/// # Errors
/// [`ReleaseError`] for what prevents the release from starting or its record from being written.
/// A step that fails is not an error: it stops the sequence and is named in
/// [`ReleaseReport::stopped`].
pub fn release(
    inp: &Inputs<'_>,
    svc: &mut Services<'_>,
    ledger: &Ledger,
    ctx: &RunContext,
    opts: &Options,
    tag_step: &mut dyn FnMut() -> Result<(), String>,
) -> Result<ReleaseReport, ReleaseError> {
    let execute = opts.mode == Mode::Execute;
    let use_artifacts = inp.select != Select::CratesOnly && !inp.artifacts.is_empty();
    let use_crates = inp.select != Select::ArtifactsOnly;
    let mut rep = ReleaseReport::default();

    // The declarations are part of the plan: an earlier run of this commit fixed them.
    if use_artifacts && execute {
        record_declarations(inp.artifacts, ledger, svc.clock, ctx)?;
    }

    // A dry run uploads nothing, stages nothing, tags nothing.
    if !execute {
        if use_crates {
            rep.crates = Some(run::run(
                inp.plan,
                svc.registry,
                svc.clock,
                ledger,
                ctx,
                opts,
            )?);
        }
        return Ok(rep);
    }

    let sctx = Ctx {
        run_id: ctx.run_id.clone(),
        sha: ctx.sha.clone(),
    };

    // 1. Stage artifacts: reversible, so everything that can fail cheaply fails here.
    if use_artifacts
        && !stage_artifacts(
            inp, svc.forge, svc.images, ledger, svc.clock, &sctx, &mut rep,
        )?
    {
        return Ok(rep);
    }

    // 2. The crates: the irreversible step.
    if use_crates {
        let r = run::run(inp.plan, svc.registry, svc.clock, ledger, ctx, opts)?;
        let ok = r.is_success();
        if !ok {
            rep.stopped = Some(r.stopped.clone().unwrap_or_else(|| {
                "not every crate is published and read back; artifacts stay staged and untagged"
                    .into()
            }));
        }
        rep.crates = Some(r);
        if !ok {
            return Ok(rep);
        }
    }

    // 3. Promote images: the user-visible commit point of an image.
    if use_artifacts && !promote_artifacts(inp, svc.images, ledger, svc.clock, &sctx, &mut rep)? {
        return Ok(rep);
    }

    // 4. The project tag, only for the whole release; 5. then the release onto it.
    if inp.select == Select::Both {
        if let Err(why) = tag_step() {
            rep.stopped = Some(format!("the project tag was not created: {why}"));
            return Ok(rep);
        }
        rep.tagged = true;
        if use_artifacts && !publish_release(inp.artifacts, svc.forge, inp.tag, &mut rep) {
            return Ok(rep);
        }
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::rc::Rc;

    use semver::Version;

    use super::*;
    use crate::artifact::Kind;
    use crate::crates_io::sha256_file;
    use crate::images::fake::FakeImages;
    use crate::images::tests::layout;
    use crate::plan::PlannedCrate;
    use crate::registry::fake::{FakeClock, FakeRegistry};
    use crate::stage::fake::FakeForge;

    const SHA: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    const TAG: &str = "v0.1.0";
    const REPO: &str = "docker.io/thinkersjournal/demo";

    struct Fix {
        dir: tempfile::TempDir,
        plan: Plan,
        arts: Vec<Artifact>,
        digest: String,
    }

    fn fix() -> Fix {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("dist")).unwrap();
        fs::write(dir.path().join("dist/x.msi"), "installer").unwrap();
        let h = sha256_file(&dir.path().join("dist/x.msi")).unwrap();
        let digest = layout(dir.path(), &[("linux", "amd64"), ("linux", "arm64")]);
        let file = Artifact {
            name: "x.msi".into(),
            kind: Kind::File,
            path: Some("dist/x.msi".into()),
            source: None,
            sha256: Some(h),
            digest: None,
            sums: None,
            target: crate::artifact::TARGET_GITHUB_RELEASE.into(),
            tags: None,
            platforms: None,
        };
        let image = Artifact {
            name: "img".into(),
            kind: Kind::Image,
            path: None,
            source: Some("dir:dist/img".into()),
            sha256: None,
            digest: Some(digest.clone()),
            sums: None,
            target: REPO.into(),
            tags: Some(vec!["0.1.0".into()]),
            platforms: Some(vec!["linux/amd64".into(), "linux/arm64".into()]),
        };
        let plan = Plan {
            order: vec![PlannedCrate {
                name: "demo".into(),
                version: Version::new(0, 1, 0),
                depends_on: Default::default(),
            }],
        };
        Fix {
            dir,
            plan,
            arts: vec![file, image],
            digest,
        }
    }

    struct World {
        forge: FakeForge,
        images: FakeImages,
        registry: FakeRegistry,
        clock: FakeClock,
        order: Rc<RefCell<Vec<String>>>,
    }

    fn world() -> World {
        let forge = FakeForge::new();
        let order = forge.order.clone();
        World {
            forge,
            images: FakeImages::new(),
            registry: FakeRegistry::new(),
            clock: FakeClock::default(),
            order,
        }
    }

    fn go(
        f: &Fix,
        w: &mut World,
        select: Select,
        mode: Mode,
        ledger: &Ledger,
        tag_ok: bool,
        arts: Option<&[Artifact]>,
    ) -> Result<ReleaseReport, ReleaseError> {
        let inp = Inputs {
            plan: &f.plan,
            artifacts: arts.unwrap_or(&f.arts),
            artifacts_dir: f.dir.path(),
            tag: TAG,
            select,
        };
        let mut svc = Services {
            forge: &mut w.forge,
            images: &mut w.images,
            registry: &mut w.registry,
            clock: &mut w.clock,
        };
        let ctx = RunContext {
            run_id: "r".into(),
            sha: SHA.into(),
        };
        let opts = Options {
            mode,
            ..Options::default()
        };
        let order = w.order.clone();
        let mut tag_step = move || {
            order.borrow_mut().push("tag".into());
            if tag_ok {
                Ok(())
            } else {
                Err("push refused".into())
            }
        };
        release(&inp, &mut svc, ledger, &ctx, &opts, &mut tag_step)
    }

    fn ledger(f: &Fix) -> Ledger {
        Ledger::open(f.dir.path().join("ledger.jsonl")).unwrap()
    }

    fn kinds(l: &Ledger) -> Vec<(String, EventKind)> {
        l.read()
            .unwrap()
            .into_iter()
            .map(|e| (e.crate_name.unwrap_or_default(), e.event))
            .collect()
    }

    fn pos(k: &[(String, EventKind)], name: &str, kind: EventKind) -> usize {
        k.iter()
            .position(|(n, e)| n == name && *e == kind)
            .unwrap_or_else(|| panic!("no {kind:?} for {name} in {k:?}"))
    }

    // The control for the rest: everything succeeds, in the designed order.
    #[test]
    fn a_whole_release_runs_in_the_designed_order_and_ends_published() {
        let f = fix();
        let l = ledger(&f);
        let mut w = world();
        let r = go(&f, &mut w, Select::Both, Mode::Execute, &l, true, None).unwrap();
        assert!(r.is_success(Select::Both, true), "{r:?}");
        assert!(r.tagged && r.published);
        let k = kinds(&l);
        let file_up = pos(&k, "x.msi", EventKind::ArtifactUploadStarted);
        let img_up = pos(&k, "img", EventKind::ArtifactUploadStarted);
        let crate_up = pos(&k, "demo", EventKind::UploadStarted);
        let promoted = pos(&k, "img", EventKind::ArtifactPromoted);
        assert!(
            file_up < crate_up && img_up < crate_up,
            "artifacts are staged before the crates: {k:?}"
        );
        assert!(crate_up < promoted, "tags come after the crates: {k:?}");
        assert_eq!(
            *w.order.borrow(),
            ["tag", "publish_draft"],
            "annotated tag first, then the release"
        );
        assert!(!w.forge.releases[TAG].draft, "published");
        // The image had no tag until after the crates.
        assert_eq!(
            w.images.refs[&(REPO.to_owned(), "0.1.0".to_owned())],
            f.digest
        );
    }

    // The A6 negative control (design section 9): a held artifact means no crate and no tag.
    #[test]
    fn a_held_artifact_stops_before_any_crate_is_published_and_nothing_is_tagged() {
        let f = fix();
        let l = ledger(&f);
        let mut w = world();
        w.images = FakeImages::new().with_tag(REPO, "0.1.0", &format!("sha256:{}", "9".repeat(64)));
        let r = go(&f, &mut w, Select::Both, Mode::Execute, &l, true, None).unwrap();
        assert!(!r.is_success(Select::Both, true));
        assert!(
            r.stopped
                .as_deref()
                .is_some_and(|m| m.contains("no crate was published")),
            "{r:?}"
        );
        assert!(
            r.crates.is_none() && w.registry.uploads.is_empty(),
            "the irreversible step was not taken"
        );
        assert!(
            !r.tagged && w.order.borrow().is_empty(),
            "no tag, no publish"
        );
        assert!(
            w.forge.releases[TAG].draft,
            "the staged release stays a draft"
        );
    }

    #[test]
    fn a_failed_file_stage_also_protects_the_crates() {
        let mut f = fix();
        f.arts[0].sha256 = Some("0".repeat(64));
        let l = ledger(&f);
        let mut w = world();
        let r = go(&f, &mut w, Select::Both, Mode::Execute, &l, true, None).unwrap();
        assert!(r.crates.is_none() && w.registry.uploads.is_empty());
        assert!(
            w.forge.upload_calls.is_empty() && w.images.push_calls.is_empty(),
            "verification is all-or-nothing"
        );
    }

    #[test]
    fn crates_that_do_not_all_land_leave_the_artifacts_staged_untagged_and_unpublished() {
        let f = fix();
        let l = ledger(&f);
        let mut w = world();
        w.registry.never_visible.insert("demo".into());
        let r = go(&f, &mut w, Select::Both, Mode::Execute, &l, true, None).unwrap();
        assert!(!r.is_success(Select::Both, true));
        assert!(r.stopped.is_some());
        assert!(w.images.tag_calls.is_empty(), "the image has no tag");
        assert!(!r.tagged && w.forge.publish_calls.is_empty() && w.forge.releases[TAG].draft);
        // Staged work is still there to resume from.
        assert_eq!(w.forge.upload_calls, ["x.msi"]);
        assert_eq!(w.images.push_calls.len(), 1);
    }

    #[test]
    fn an_image_that_cannot_be_tagged_stops_before_the_project_tag() {
        let f = fix();
        let l = ledger(&f);
        let mut w = world();
        // A writer takes the tag after our pre-check and push, before promotion.
        w.images.race_tag = Some(("0.1.0".into(), format!("sha256:{}", "7".repeat(64))));
        let r = go(&f, &mut w, Select::Both, Mode::Execute, &l, true, None).unwrap();
        assert!(!r.is_success(Select::Both, true), "{r:?}");
        assert!(
            r.stopped
                .as_deref()
                .is_some_and(|m| m.contains("could not be tagged")),
            "{r:?}"
        );
        assert!(
            !w.registry.uploads.is_empty(),
            "the crates were already published: that step is irreversible"
        );
        assert!(
            !r.tagged && w.order.borrow().is_empty() && w.forge.publish_calls.is_empty(),
            "no project tag over a held image"
        );
        assert!(w.images.tag_calls.is_empty(), "the taken tag was not moved");
    }

    #[test]
    fn a_failing_tag_step_leaves_the_release_a_draft() {
        let f = fix();
        let l = ledger(&f);
        let mut w = world();
        let r = go(&f, &mut w, Select::Both, Mode::Execute, &l, false, None).unwrap();
        assert!(
            r.stopped
                .as_deref()
                .is_some_and(|m| m.contains("push refused")),
            "{r:?}"
        );
        assert!(!r.tagged && w.forge.publish_calls.is_empty() && w.forge.releases[TAG].draft);
    }

    #[test]
    fn a_release_that_cannot_be_published_says_the_tag_exists() {
        let f = fix();
        let l = ledger(&f);
        let mut w = world();
        w.forge.publish_error = Some("422".into());
        let r = go(&f, &mut w, Select::Both, Mode::Execute, &l, true, None).unwrap();
        assert!(r.tagged && !r.published);
        assert!(
            r.stopped
                .as_deref()
                .is_some_and(|m| m.contains("tag exists")),
            "{r:?}"
        );
    }

    #[test]
    fn crates_only_is_todays_behaviour_and_never_touches_artifacts_or_the_project_tag() {
        let f = fix();
        let l = ledger(&f);
        let mut w = world();
        let r = go(
            &f,
            &mut w,
            Select::CratesOnly,
            Mode::Execute,
            &l,
            true,
            None,
        )
        .unwrap();
        assert!(r.is_success(Select::CratesOnly, true), "{r:?}");
        assert!(w.forge.upload_calls.is_empty() && w.images.push_calls.is_empty());
        assert!(
            w.order.borrow().is_empty(),
            "the caller tags crates itself in this mode"
        );
    }

    #[test]
    fn artifacts_only_publishes_no_crate_and_creates_no_project_tag() {
        let f = fix();
        let l = ledger(&f);
        let mut w = world();
        let r = go(
            &f,
            &mut w,
            Select::ArtifactsOnly,
            Mode::Execute,
            &l,
            true,
            None,
        )
        .unwrap();
        assert!(r.is_success(Select::ArtifactsOnly, true), "{r:?}");
        assert!(r.crates.is_none() && w.registry.uploads.is_empty());
        assert!(
            w.order.borrow().is_empty(),
            "no project tag, so the release stays a draft"
        );
        assert!(w.forge.releases[TAG].draft);
        assert_eq!(
            w.images.refs[&(REPO.to_owned(), "0.1.0".to_owned())],
            f.digest,
            "images are promoted"
        );
    }

    #[test]
    fn a_dry_run_touches_nothing() {
        let f = fix();
        let l = ledger(&f);
        let mut w = world();
        let r = go(&f, &mut w, Select::Both, Mode::DryRun, &l, true, None).unwrap();
        assert!(!r.is_success(Select::Both, false));
        assert!(w.forge.drafts_created.is_empty() && w.forge.upload_calls.is_empty());
        assert!(w.images.push_calls.is_empty() && w.registry.uploads.is_empty());
        assert!(w.order.borrow().is_empty());
        assert!(
            r.crates.is_some(),
            "the crates' read-only lookups still run"
        );
    }

    // The artifact declarations are part of the plan hash: a changed one is refused on resume.
    #[test]
    fn a_changed_artifact_declaration_is_refused_for_the_same_commit() {
        let f = fix();
        let l = ledger(&f);
        let mut w = world();
        // First run stops (the crate never appears) so a resume is meaningful.
        w.registry.never_visible.insert("demo".into());
        go(&f, &mut w, Select::Both, Mode::Execute, &l, true, None).unwrap();
        let mut changed = f.arts.clone();
        changed[1].tags = Some(vec!["0.1.1".into()]);
        let e = go(
            &f,
            &mut w,
            Select::Both,
            Mode::Execute,
            &l,
            true,
            Some(&changed),
        )
        .unwrap_err();
        assert!(matches!(e, ReleaseError::ArtifactsChanged { .. }), "{e}");
        // The control: the same declarations are accepted.
        assert!(go(&f, &mut w, Select::Both, Mode::Execute, &l, true, None).is_ok());
    }

    #[test]
    fn a_release_without_artifacts_is_exactly_a_crates_release() {
        let f = fix();
        let l = ledger(&f);
        let mut w = world();
        let r = go(&f, &mut w, Select::Both, Mode::Execute, &l, true, Some(&[])).unwrap();
        assert!(r.is_success(Select::Both, true), "{r:?}");
        assert!(
            r.tagged && !r.published,
            "no release to publish when there are no files"
        );
        assert!(w.forge.drafts_created.is_empty());
        assert!(
            !kinds(&l).iter().any(|(n, _)| n == ARTIFACTS_SUBJECT),
            "no artifact plan event"
        );
    }
}
