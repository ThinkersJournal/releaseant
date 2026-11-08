//! Staging `file` artifacts as GitHub release assets (`docs/ARTIFACTS-DESIGN.md` sections 2, 4.1, 6;
//! slice A2). Everything here goes through the [`Forge`] trait, so it is tested against a fake; no
//! real forge is touched until slice A3, which is also where the limits and the error wording of a
//! real one are measured.
//!
//! The rules, each a test:
//! 1. every file is hashed locally and must equal its declaration BEFORE anything is uploaded;
//! 2. assets go into a DRAFT release (invisible to users), never into a public one;
//! 3. an asset counts only when the bytes the forge serves back hash to the declaration;
//! 4. a name that already exists is never replaced or deleted: the same bytes with a ledger entry for
//!    this commit are `AlreadyPublished`, anything else is `Held`;
//! 5. a forge that cannot be asked is `Unknown`, never "absent" (the run stops, resumable).
//!
//! Promoting the draft (tag, undraft) is slice A6, not here.

use std::fs;
use std::path::Path;

use crate::artifact::{Artifact, Kind};
use crate::crates_io::sha256_file;
use crate::ledger::{Event, EventKind, Ledger, LedgerError};
use crate::registry::Clock;

/// One asset on a release as the forge reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetInfo {
    /// The asset's name.
    pub name: String,
    /// Its size in bytes as the forge records it.
    pub size: u64,
    /// `false` for an upload the forge has not finished (a dropped connection can leave one).
    pub complete: bool,
}

/// A release of a tag.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Release {
    /// A draft is invisible to users.
    pub draft: bool,
    /// The assets on it.
    pub assets: Vec<AssetInfo>,
}

/// The forge, as far as staging needs it.
pub trait Forge {
    /// The release for `tag`: `Ok(None)` only when the forge definitively has none.
    ///
    /// # Errors
    /// The failure text when the forge could not be asked (never read as "none").
    fn release(&mut self, tag: &str) -> Result<Option<Release>, String>;
    /// Create a draft release for `tag`.
    ///
    /// # Errors
    /// The forge's refusal.
    fn create_draft(&mut self, tag: &str) -> Result<(), String>;
    /// Upload `file` to the release of `tag` as `name`. Never replaces an existing asset.
    ///
    /// # Errors
    /// The forge's refusal or a transport failure (the asset may then be partial).
    fn upload_asset(&mut self, tag: &str, name: &str, file: &Path) -> Result<(), String>;
    /// The SHA-256 (lowercase hex) of the bytes the forge now serves for the asset.
    ///
    /// # Errors
    /// The failure text when it could not be read.
    fn served_sha256(&mut self, tag: &str, name: &str) -> Result<String, String>;
}

/// What happened to one artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Uploaded and read back: the forge serves the declared bytes.
    Visible,
    /// Already there with the declared bytes, uploaded by this tool from this commit.
    AlreadyPublished,
    /// Something is there that this run did not put there, or the release is public: the run's
    /// answer is to stop and ask a human. Nothing was changed.
    Held(String),
    /// Refused or failed, with the reason.
    Failed(String),
    /// Not attempted because another artifact failed verification first.
    Skipped(String),
}

impl Outcome {
    /// Whether the artifact is on the forge with the declared bytes.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Visible | Self::AlreadyPublished)
    }
}

/// The result of a staging pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StageReport {
    /// `(artifact name, outcome)` in manifest order, for the artifacts that were reached.
    pub outcomes: Vec<(String, Outcome)>,
    /// Why the run stopped early (a forge that could not be asked), if it did. Resumable.
    pub stopped: Option<String>,
}

impl StageReport {
    /// Whether every artifact is on the forge with the declared bytes and nothing stopped the run.
    #[must_use]
    pub fn all_ok(&self) -> bool {
        self.stopped.is_none() && self.outcomes.iter().all(|(_, o)| o.is_ok())
    }
}

/// Who is running, for the ledger.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// The run id.
    pub run_id: String,
    /// The full commit SHA.
    pub sha: String,
}

/// The hash declared for `path` in the text of a `SHA256SUMS` file (`<64 hex>  <name>`, a `*` before
/// the name allowed), matching the whole path or its last component.
///
/// # Errors
/// When the file is not listed, or is listed with two different hashes.
pub fn hash_from_sums(text: &str, path: &str) -> Result<String, String> {
    let want = path.replace('\\', "/");
    let base = want.rsplit('/').next().unwrap_or(&want);
    let mut found: Vec<&str> = Vec::new();
    for line in text.lines() {
        let Some((hash, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let name = rest.trim_start().trim_start_matches('*').replace('\\', "/");
        let name = name.strip_prefix("./").unwrap_or(&name);
        let hash_ok = hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if hash_ok && (name == want || name == base) && !found.contains(&hash) {
            found.push(hash);
        }
    }
    match found.as_slice() {
        [h] => Ok((*h).to_owned()),
        [] => Err(format!("{path:?} is not listed in the sums file")),
        _ => Err(format!(
            "{path:?} is listed with different hashes in the sums file"
        )),
    }
}

/// The declared hash of a file artifact: its `sha256`, or the entry in its `sums` file under `dir`.
fn declared(a: &Artifact, dir: &Path) -> Result<String, String> {
    if let Some(h) = &a.sha256 {
        return Ok(h.clone());
    }
    let sums = a.sums.as_deref().ok_or("no sha256 and no sums")?;
    let text = fs::read_to_string(dir.join(sums))
        .map_err(|e| format!("sums file {sums:?} could not be read: {e}"))?;
    hash_from_sums(&text, a.path.as_deref().unwrap_or(""))
}

/// The asset name a file is uploaded under: the artifact's `name` (the manifest's unique key).
fn asset_name(a: &Artifact) -> &str {
    &a.name
}

pub(crate) fn ours(earlier: &[Event], ctx: &Ctx, name: &str) -> bool {
    earlier.iter().any(|e| {
        e.sha == ctx.sha
            && e.crate_name.as_deref() == Some(name)
            && matches!(
                e.event,
                EventKind::ArtifactUploadStarted | EventKind::ArtifactVisible
            )
    })
}

pub(crate) struct Rec<'a> {
    pub(crate) ledger: &'a Ledger,
    pub(crate) ctx: &'a Ctx,
}

impl Rec<'_> {
    pub(crate) fn log(
        &self,
        clock: &mut dyn Clock,
        kind: EventKind,
        name: &str,
        detail: impl Into<String>,
    ) -> Result<(), LedgerError> {
        self.ledger.append(&Event {
            ts: clock.now_utc(),
            run: self.ctx.run_id.clone(),
            sha: self.ctx.sha.clone(),
            crate_name: Some(name.to_owned()),
            version: None,
            event: kind,
            detail: detail.into(),
        })
    }
}

/// Stage the `file` artifacts of `artifacts` into the draft release of `tag`, reading local files
/// from `dir`. Artifacts of other kinds are ignored here.
///
/// # Errors
/// [`LedgerError`] when the ledger cannot be written. Trouble with the forge is not an error: it is
/// in the report.
pub fn stage_files(
    forge: &mut dyn Forge,
    ledger: &Ledger,
    clock: &mut dyn Clock,
    ctx: &Ctx,
    tag: &str,
    dir: &Path,
    artifacts: &[Artifact],
) -> Result<StageReport, LedgerError> {
    let rec = Rec { ledger, ctx };
    let files: Vec<&Artifact> = artifacts.iter().filter(|a| a.kind == Kind::File).collect();
    let mut report = StageReport::default();

    // 1. Verify every local file against its declaration before anything leaves the machine.
    let mut verified: Vec<(&Artifact, String)> = Vec::new();
    let mut any_failed = false;
    let mut results: Vec<(String, Option<Outcome>)> = Vec::new();
    for a in &files {
        let checked = declared(a, dir).and_then(|want| {
            let path = a.path.as_deref().unwrap_or("");
            let got = sha256_file(&dir.join(path))
                .map_err(|e| format!("{path:?} could not be read: {e}"))?;
            if got == want {
                Ok(want)
            } else {
                Err(format!(
                    "local file hashes to {got}, the declaration says {want}; nothing was uploaded"
                ))
            }
        });
        match checked {
            Ok(h) => {
                rec.log(
                    clock,
                    EventKind::ArtifactVerified,
                    &a.name,
                    format!("sha256={h}"),
                )?;
                results.push((a.name.clone(), None));
                verified.push((a, h));
            }
            Err(why) => {
                any_failed = true;
                rec.log(clock, EventKind::ArtifactFailed, &a.name, why.clone())?;
                results.push((a.name.clone(), Some(Outcome::Failed(why))));
            }
        }
    }
    if any_failed {
        report.outcomes = results
            .into_iter()
            .map(|(n, o)| {
                (
                    n,
                    o.unwrap_or_else(|| {
                        Outcome::Skipped("another artifact failed verification".into())
                    }),
                )
            })
            .collect();
        return Ok(report);
    }
    if verified.is_empty() {
        return Ok(report);
    }

    // 2. The release must be ours to add to: absent (create a draft) or an existing draft.
    let release = match forge.release(tag) {
        Err(why) => {
            report.stopped = Some(format!("the forge could not be asked about {tag}: {why}"));
            return Ok(report);
        }
        Ok(r) => r,
    };
    let assets = match release {
        None => {
            if let Err(why) = forge.create_draft(tag) {
                let why = format!("could not create a draft release for {tag}: {why}");
                for (a, _) in &verified {
                    rec.log(clock, EventKind::ArtifactFailed, &a.name, why.clone())?;
                    report
                        .outcomes
                        .push((a.name.clone(), Outcome::Failed(why.clone())));
                }
                return Ok(report);
            }
            Vec::new()
        }
        Some(r) if !r.draft => {
            let why = format!("the release for {tag} is already public; nothing was changed");
            for (a, _) in &verified {
                rec.log(clock, EventKind::ArtifactHeld, &a.name, why.clone())?;
                report
                    .outcomes
                    .push((a.name.clone(), Outcome::Held(why.clone())));
            }
            return Ok(report);
        }
        Some(r) => r.assets,
    };

    // 3. Each artifact, in manifest order.
    let earlier = ledger.read()?;
    for (a, want) in verified {
        let name = asset_name(a);
        let outcome = match assets.iter().find(|x| x.name == name) {
            Some(x) if !x.complete => {
                let why = format!(
                    "{name}: a partial asset ({} bytes) is on the draft; it is not replaced or removed by this tool, a person decides",
                    x.size
                );
                rec.log(clock, EventKind::ArtifactHeld, &a.name, why.clone())?;
                Outcome::Held(why)
            }
            Some(_) => match forge.served_sha256(tag, name) {
                Err(why) => {
                    report.stopped = Some(format!("{name}: could not read the asset back: {why}"));
                    report.outcomes.push((a.name.clone(), Outcome::Failed(why)));
                    return Ok(report);
                }
                Ok(got) if got != want => {
                    let why = format!(
                        "{name}: already on the draft with different bytes ({got}, declared {want}); not replaced"
                    );
                    rec.log(clock, EventKind::ArtifactHeld, &a.name, why.clone())?;
                    Outcome::Held(why)
                }
                Ok(_) if ours(&earlier, ctx, name) => {
                    rec.log(
                        clock,
                        EventKind::ArtifactVisible,
                        &a.name,
                        "already there, resumed",
                    )?;
                    Outcome::AlreadyPublished
                }
                Ok(_) => {
                    let why = format!(
                        "{name}: on the draft with the declared bytes, but not uploaded by this tool from this commit"
                    );
                    rec.log(clock, EventKind::ArtifactHeld, &a.name, why.clone())?;
                    Outcome::Held(why)
                }
            },
            None => {
                let path = a.path.as_deref().unwrap_or("");
                rec.log(
                    clock,
                    EventKind::ArtifactUploadStarted,
                    &a.name,
                    format!("sha256={want}"),
                )?;
                if let Err(why) = forge.upload_asset(tag, name, &dir.join(path)) {
                    rec.log(clock, EventKind::ArtifactFailed, &a.name, why.clone())?;
                    report.outcomes.push((a.name.clone(), Outcome::Failed(why)));
                    // Whatever the failure left on the forge is found, by asking, on the next run.
                    continue;
                }
                rec.log(clock, EventKind::ArtifactUploadFinished, &a.name, "")?;
                match forge.served_sha256(tag, name) {
                    Err(why) => {
                        report.stopped = Some(format!(
                            "{name}: uploaded, but could not be read back: {why}"
                        ));
                        report.outcomes.push((a.name.clone(), Outcome::Failed(why)));
                        return Ok(report);
                    }
                    Ok(got) if got == want => {
                        rec.log(
                            clock,
                            EventKind::ArtifactVisible,
                            &a.name,
                            format!("sha256={got}"),
                        )?;
                        Outcome::Visible
                    }
                    Ok(got) => {
                        let why = format!(
                            "{name}: read-back differs: the forge serves {got}, declared {want}"
                        );
                        rec.log(clock, EventKind::ArtifactFailed, &a.name, why.clone())?;
                        Outcome::Failed(why)
                    }
                }
            }
        };
        report.outcomes.push((a.name.clone(), outcome));
    }
    Ok(report)
}

/// An in-memory forge for tests.
pub mod fake {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    use super::{AssetInfo, Forge, Release};
    use crate::crates_io::sha256_file;

    /// Called at the start of each upload with the asset name.
    pub type UploadHook = Box<dyn FnMut(&str)>;

    /// Scriptable forge. Behaviour not scripted is the honest default: an upload becomes a complete
    /// asset that serves exactly the bytes sent, and an existing asset is never replaced.
    #[derive(Default)]
    pub struct FakeForge {
        /// Releases by tag.
        pub releases: BTreeMap<String, Release>,
        /// `(tag, asset)` to the SHA-256 of the bytes the forge serves.
        pub served: BTreeMap<(String, String), String>,
        /// Every `upload_asset` call, in order (refused ones included).
        pub upload_calls: Vec<String>,
        /// Every `create_draft` call.
        pub drafts_created: Vec<String>,
        /// Uploads of these assets fail with a transport error.
        pub fail_upload: BTreeSet<String>,
        /// A failing upload leaves a partial asset behind.
        pub partial_on_fail: bool,
        /// These assets are served with bytes that differ from what was uploaded.
        pub corrupt: BTreeSet<String>,
        /// `release` fails with this text.
        pub release_error: Option<String>,
        /// The next `served_sha256` of these assets fails once (a kill between upload and read-back).
        pub read_back_fails_once: BTreeSet<String>,
        /// Run at the start of each upload.
        pub on_upload: Option<UploadHook>,
    }

    impl FakeForge {
        /// An empty forge.
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }

        /// Put a complete asset serving `sha` on the (draft or public) release of `tag`.
        pub fn with_asset(mut self, tag: &str, draft: bool, name: &str, sha: &str) -> Self {
            let r = self.releases.entry(tag.into()).or_default();
            r.draft = draft;
            r.assets.push(AssetInfo {
                name: name.into(),
                size: 1,
                complete: true,
            });
            self.served.insert((tag.into(), name.into()), sha.into());
            self
        }

        /// Put a release with no assets on the forge.
        pub fn with_release(mut self, tag: &str, draft: bool) -> Self {
            self.releases.entry(tag.into()).or_default().draft = draft;
            self
        }

        /// Put an incomplete asset on the draft of `tag`.
        pub fn with_partial(mut self, tag: &str, name: &str) -> Self {
            let r = self.releases.entry(tag.into()).or_default();
            r.draft = true;
            r.assets.push(AssetInfo {
                name: name.into(),
                size: 7,
                complete: false,
            });
            self
        }
    }

    impl Forge for FakeForge {
        fn release(&mut self, tag: &str) -> Result<Option<Release>, String> {
            if let Some(e) = &self.release_error {
                return Err(e.clone());
            }
            Ok(self.releases.get(tag).cloned())
        }

        fn create_draft(&mut self, tag: &str) -> Result<(), String> {
            self.drafts_created.push(tag.into());
            self.releases.insert(
                tag.into(),
                Release {
                    draft: true,
                    assets: Vec::new(),
                },
            );
            Ok(())
        }

        fn upload_asset(&mut self, tag: &str, name: &str, file: &Path) -> Result<(), String> {
            self.upload_calls.push(name.into());
            if let Some(h) = self.on_upload.as_mut() {
                h(name);
            }
            let r = self.releases.get_mut(tag).ok_or("no such release")?;
            if r.assets.iter().any(|a| a.name == name) {
                return Err(format!("{name}: already exists"));
            }
            if self.fail_upload.contains(name) {
                if self.partial_on_fail {
                    r.assets.push(AssetInfo {
                        name: name.into(),
                        size: 3,
                        complete: false,
                    });
                }
                return Err("connection reset".into());
            }
            let sha = if self.corrupt.contains(name) {
                "f".repeat(64)
            } else {
                sha256_file(file).map_err(|e| e.to_string())?
            };
            r.assets.push(AssetInfo {
                name: name.into(),
                size: 1,
                complete: true,
            });
            self.served.insert((tag.into(), name.into()), sha);
            Ok(())
        }

        fn served_sha256(&mut self, tag: &str, name: &str) -> Result<String, String> {
            if self.read_back_fails_once.remove(name) {
                return Err("timed out".into());
            }
            self.served
                .get(&(tag.to_owned(), name.to_owned()))
                .cloned()
                .ok_or_else(|| format!("{name}: no such asset"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeForge;
    use super::*;
    use crate::registry::fake::FakeClock;

    const SHA: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";

    fn ctx(sha: &str) -> Ctx {
        Ctx {
            run_id: "r1".into(),
            sha: sha.into(),
        }
    }

    /// A directory with `dist/a.msi` ("alpha") and `dist/b.tar` ("beta"), and its artifacts.
    struct Fix {
        dir: tempfile::TempDir,
        arts: Vec<Artifact>,
        ha: String,
        hb: String,
    }

    fn fix() -> Fix {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("dist")).unwrap();
        fs::write(dir.path().join("dist/a.msi"), "alpha").unwrap();
        fs::write(dir.path().join("dist/b.tar"), "beta").unwrap();
        let ha = sha256_file(&dir.path().join("dist/a.msi")).unwrap();
        let hb = sha256_file(&dir.path().join("dist/b.tar")).unwrap();
        let mk = |name: &str, path: &str, h: &str| Artifact {
            name: name.into(),
            kind: Kind::File,
            path: Some(path.into()),
            source: None,
            sha256: Some(h.into()),
            digest: None,
            sums: None,
            target: crate::artifact::TARGET_GITHUB_RELEASE.into(),
            tags: None,
            platforms: None,
        };
        let arts = vec![
            mk("a.msi", "dist/a.msi", &ha),
            mk("b.tar", "dist/b.tar", &hb),
        ];
        Fix { dir, arts, ha, hb }
    }

    fn run(forge: &mut FakeForge, ledger: &Ledger, f: &Fix, sha: &str) -> StageReport {
        stage_files(
            forge,
            ledger,
            &mut FakeClock::default(),
            &ctx(sha),
            "v1.0.0",
            f.dir.path(),
            &f.arts,
        )
        .unwrap()
    }

    fn ledger(dir: &Path) -> Ledger {
        Ledger::open(dir.join("ledger.jsonl")).unwrap()
    }

    fn kinds(l: &Ledger) -> Vec<(String, EventKind)> {
        l.read()
            .unwrap()
            .into_iter()
            .map(|e| (e.crate_name.unwrap_or_default(), e.event))
            .collect()
    }

    // The control for everything below: nothing on the forge, everything declared correctly.
    #[test]
    fn a_clean_run_creates_a_draft_uploads_and_reads_back_every_file() {
        let f = fix();
        let l = ledger(f.dir.path());
        let mut forge = FakeForge::new();
        let r = run(&mut forge, &l, &f, SHA);
        assert!(r.all_ok(), "{r:?}");
        assert_eq!(
            r.outcomes
                .iter()
                .map(|(_, o)| o.clone())
                .collect::<Vec<_>>(),
            vec![Outcome::Visible; 2]
        );
        assert_eq!(forge.drafts_created, ["v1.0.0"]);
        assert!(
            forge.releases["v1.0.0"].draft,
            "staged into a draft, never a public release"
        );
        assert_eq!(forge.upload_calls, ["a.msi", "b.tar"]);
        assert_eq!(
            kinds(&l)
                .iter()
                .filter(|(n, _)| n == "a.msi")
                .map(|(_, k)| *k)
                .collect::<Vec<_>>(),
            [
                EventKind::ArtifactVerified,
                EventKind::ArtifactUploadStarted,
                EventKind::ArtifactUploadFinished,
                EventKind::ArtifactVisible
            ]
        );
    }

    #[test]
    fn upload_started_is_in_the_ledger_before_the_upload_happens() {
        let f = fix();
        let l = ledger(f.dir.path());
        let path = l.path().to_owned();
        let mut forge = FakeForge::new();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let s2 = seen.clone();
        forge.on_upload = Some(Box::new(move |name| {
            let text = fs::read_to_string(&path).unwrap_or_default();
            s2.borrow_mut()
                .push((name.to_owned(), text.contains("artifact_upload_started")));
        }));
        run(&mut forge, &l, &f, SHA);
        assert_eq!(
            *seen.borrow(),
            [("a.msi".to_owned(), true), ("b.tar".to_owned(), true)]
        );
    }

    #[test]
    fn a_local_file_that_differs_from_its_declaration_stops_everything_before_any_upload() {
        let mut f = fix();
        f.arts[1].sha256 = Some("0".repeat(64));
        let l = ledger(f.dir.path());
        let mut forge = FakeForge::new();
        let r = run(&mut forge, &l, &f, SHA);
        assert!(!r.all_ok());
        assert!(
            matches!(&r.outcomes[0].1, Outcome::Skipped(_)),
            "the good one waits: {r:?}"
        );
        assert!(
            matches!(&r.outcomes[1].1, Outcome::Failed(m) if m.contains("nothing was uploaded")),
            "{r:?}"
        );
        assert!(
            forge.upload_calls.is_empty() && forge.drafts_created.is_empty(),
            "the forge was not touched"
        );
        // A missing file is a failure too, not a panic.
        let mut f2 = fix();
        f2.arts[0].path = Some("dist/missing.msi".into());
        let r = run(&mut FakeForge::new(), &ledger(f2.dir.path()), &f2, SHA);
        assert!(
            matches!(&r.outcomes[0].1, Outcome::Failed(m) if m.contains("could not be read")),
            "{r:?}"
        );
    }

    #[test]
    fn bytes_served_back_that_differ_are_not_visible() {
        let f = fix();
        let l = ledger(f.dir.path());
        let mut forge = FakeForge::new();
        forge.corrupt.insert("a.msi".into());
        let r = run(&mut forge, &l, &f, SHA);
        assert!(
            matches!(&r.outcomes[0].1, Outcome::Failed(m) if m.contains("read-back differs")),
            "{r:?}"
        );
        assert_eq!(
            r.outcomes[1].1,
            Outcome::Visible,
            "the other file is unaffected"
        );
        assert!(!r.all_ok());
        assert!(!kinds(&l).contains(&("a.msi".into(), EventKind::ArtifactVisible)));
    }

    #[test]
    fn a_name_that_exists_is_held_unless_the_ledger_says_this_commit_put_it_there() {
        let f = fix();
        // Different bytes: held, never replaced.
        let l = ledger(f.dir.path());
        let mut forge = FakeForge::new().with_asset("v1.0.0", true, "a.msi", &"9".repeat(64));
        let r = run(&mut forge, &l, &f, SHA);
        assert!(
            matches!(&r.outcomes[0].1, Outcome::Held(m) if m.contains("different bytes")),
            "{r:?}"
        );
        assert!(
            !forge.upload_calls.contains(&"a.msi".to_owned()),
            "never replaced"
        );
        // Same bytes, no ledger entry (a lost ledger, or someone else's upload): held (DESIGN section 17).
        let l2 = ledger(tempfile::tempdir().unwrap().keep().as_path());
        let mut forge = FakeForge::new().with_asset("v1.0.0", true, "a.msi", &f.ha);
        let r = run(&mut forge, &l2, &f, SHA);
        assert!(
            matches!(&r.outcomes[0].1, Outcome::Held(m) if m.contains("not uploaded by this tool")),
            "{r:?}"
        );
        // Same bytes and this commit's ledger entry (the control): already published, no upload.
        let l3 = ledger(tempfile::tempdir().unwrap().keep().as_path());
        let mut first = FakeForge::new();
        assert!(run(&mut first, &l3, &f, SHA).all_ok());
        let uploads = first.upload_calls.len();
        let r = run(&mut first, &l3, &f, SHA);
        assert_eq!(
            r.outcomes
                .iter()
                .map(|(_, o)| o.clone())
                .collect::<Vec<_>>(),
            vec![Outcome::AlreadyPublished; 2]
        );
        assert_eq!(
            first.upload_calls.len(),
            uploads,
            "a resume uploads nothing"
        );
        // A ledger recorded at another commit proves nothing about this one.
        let r = run(&mut first, &l3, &f, &"b".repeat(40));
        assert!(matches!(&r.outcomes[0].1, Outcome::Held(_)), "{r:?}");
    }

    #[test]
    fn a_public_release_is_never_added_to() {
        let f = fix();
        let l = ledger(f.dir.path());
        let mut forge = FakeForge::new().with_release("v1.0.0", false);
        let r = run(&mut forge, &l, &f, SHA);
        assert!(
            r.outcomes
                .iter()
                .all(|(_, o)| matches!(o, Outcome::Held(m) if m.contains("already public"))),
            "{r:?}"
        );
        assert!(forge.upload_calls.is_empty());
    }

    #[test]
    fn a_partial_asset_is_held_not_replaced_or_deleted() {
        let f = fix();
        let l = ledger(f.dir.path());
        let mut forge = FakeForge::new().with_partial("v1.0.0", "a.msi");
        let r = run(&mut forge, &l, &f, SHA);
        assert!(
            matches!(&r.outcomes[0].1, Outcome::Held(m) if m.contains("partial")),
            "{r:?}"
        );
        assert_eq!(
            r.outcomes[1].1,
            Outcome::Visible,
            "the other file still stages"
        );
        assert_eq!(forge.upload_calls, ["b.tar"]);
        assert!(
            forge.releases["v1.0.0"]
                .assets
                .iter()
                .any(|a| a.name == "a.msi" && !a.complete),
            "left alone"
        );
    }

    #[test]
    fn a_failed_upload_leaves_a_partial_that_the_next_run_finds_by_asking() {
        let f = fix();
        let l = ledger(f.dir.path());
        let mut forge = FakeForge::new();
        forge.fail_upload.insert("a.msi".into());
        forge.partial_on_fail = true;
        let r = run(&mut forge, &l, &f, SHA);
        assert!(
            matches!(&r.outcomes[0].1, Outcome::Failed(m) if m.contains("connection reset")),
            "{r:?}"
        );
        assert_eq!(r.outcomes[1].1, Outcome::Visible);
        forge.fail_upload.clear();
        let r = run(&mut forge, &l, &f, SHA);
        assert!(
            matches!(&r.outcomes[0].1, Outcome::Held(m) if m.contains("partial")),
            "{r:?}"
        );
        assert_eq!(r.outcomes[1].1, Outcome::AlreadyPublished);
    }

    #[test]
    fn a_forge_that_cannot_be_asked_stops_the_run_and_uploads_nothing() {
        let f = fix();
        let l = ledger(f.dir.path());
        let mut forge = FakeForge::new();
        forge.release_error = Some("HTTP 502".into());
        let r = run(&mut forge, &l, &f, SHA);
        assert!(
            r.stopped.as_deref().is_some_and(|m| m.contains("502")),
            "{r:?}"
        );
        assert!(
            forge.upload_calls.is_empty() && forge.drafts_created.is_empty(),
            "never read as absent"
        );
    }

    // DESIGN section 6: a kill between upload and read-back is resolved by asking the forge.
    #[test]
    fn a_kill_between_upload_and_read_back_resumes_without_a_second_upload() {
        let f = fix();
        let l = ledger(f.dir.path());
        let mut forge = FakeForge::new();
        forge.read_back_fails_once.insert("a.msi".into());
        let r = run(&mut forge, &l, &f, SHA);
        assert!(
            r.stopped
                .as_deref()
                .is_some_and(|m| m.contains("could not be read back")),
            "{r:?}"
        );
        assert_eq!(
            forge.upload_calls,
            ["a.msi"],
            "the run stopped after the first upload"
        );
        assert!(kinds(&l).contains(&("a.msi".into(), EventKind::ArtifactUploadFinished)));
        assert!(!kinds(&l).contains(&("a.msi".into(), EventKind::ArtifactVisible)));
        // Resume: a.msi is found by asking (complete, equal hash, ledger says ours), b.tar is uploaded.
        let r = run(&mut forge, &l, &f, SHA);
        assert!(r.all_ok(), "{r:?}");
        assert_eq!(r.outcomes[0].1, Outcome::AlreadyPublished);
        assert_eq!(r.outcomes[1].1, Outcome::Visible);
        assert_eq!(
            forge.upload_calls,
            ["a.msi", "b.tar"],
            "a.msi was not uploaded twice"
        );
    }

    #[test]
    fn the_hash_can_come_from_a_sums_file_and_a_bad_entry_is_a_failure() {
        let mut f = fix();
        let sums = format!("{}  dist/a.msi\n{} *b.tar\n", f.ha, f.hb);
        fs::write(f.dir.path().join("SHA256SUMS"), &sums).unwrap();
        for a in &mut f.arts {
            a.sha256 = None;
            a.sums = Some("SHA256SUMS".into());
        }
        let r = run(&mut FakeForge::new(), &ledger(f.dir.path()), &f, SHA);
        assert!(r.all_ok(), "{r:?}");
        // Not listed.
        fs::write(
            f.dir.path().join("SHA256SUMS"),
            format!("{}  dist/a.msi\n", f.ha),
        )
        .unwrap();
        let r = run(
            &mut FakeForge::new(),
            &ledger(tempfile::tempdir().unwrap().keep().as_path()),
            &f,
            SHA,
        );
        assert!(
            matches!(&r.outcomes[1].1, Outcome::Failed(m) if m.contains("not listed")),
            "{r:?}"
        );
        // A sums file that disagrees with the file on disk.
        fs::write(
            f.dir.path().join("SHA256SUMS"),
            format!("{}  dist/a.msi\n{}  b.tar\n", "0".repeat(64), f.hb),
        )
        .unwrap();
        let r = run(
            &mut FakeForge::new(),
            &ledger(tempfile::tempdir().unwrap().keep().as_path()),
            &f,
            SHA,
        );
        assert!(
            matches!(&r.outcomes[0].1, Outcome::Failed(m) if m.contains("declaration says")),
            "{r:?}"
        );
    }

    #[test]
    fn sums_parsing_matches_path_or_basename_and_refuses_conflicts() {
        let h1 = "1".repeat(64);
        let h2 = "2".repeat(64);
        assert_eq!(
            hash_from_sums(&format!("{h1}  dist/x\n"), "dist/x"),
            Ok(h1.clone())
        );
        assert_eq!(
            hash_from_sums(&format!("{h1} *./x\n"), "dist/x"),
            Ok(h1.clone()),
            "basename, binary marker, ./"
        );
        assert_eq!(
            hash_from_sums(&format!("{h1}  x\n{h1}  x\n"), "x"),
            Ok(h1.clone()),
            "the same hash twice is fine"
        );
        assert!(
            hash_from_sums(&format!("{h1}  x\n{h2}  x\n"), "x")
                .unwrap_err()
                .contains("different hashes")
        );
        assert!(hash_from_sums("", "x").unwrap_err().contains("not listed"));
        assert!(
            hash_from_sums("short  x\n", "x").is_err(),
            "a malformed hash is not an entry"
        );
    }

    #[test]
    fn images_are_not_this_stages_business() {
        let mut f = fix();
        f.arts = vec![Artifact {
            name: "img".into(),
            kind: Kind::Image,
            path: None,
            source: Some("dir:x".into()),
            sha256: None,
            digest: Some(format!("sha256:{}", "1".repeat(64))),
            sums: None,
            target: "docker.io/a/b".into(),
            tags: Some(vec!["1".into()]),
            platforms: Some(vec!["linux/amd64".into()]),
        }];
        let mut forge = FakeForge::new();
        let r = run(&mut forge, &ledger(f.dir.path()), &f, SHA);
        assert_eq!(r, StageReport::default());
        assert!(
            forge.drafts_created.is_empty(),
            "no draft for nothing to stage"
        );
    }
}
