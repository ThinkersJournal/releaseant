//! Staging and promoting `image` artifacts (`docs/ARTIFACTS-DESIGN.md` sections 2, 4.2, 6; slice A4).
//!
//! An image is a local OCI layout directory (`dir:`), pushed BY DIGEST (nothing a user can pull by
//! name changes), read back by hashing what the registry serves and checking every child manifest and
//! the platform set, and only later given its tags (`promote_images`, after the crates are published).
//! Everything goes through [`ImageRegistry`]: [`fake::FakeImages`] for tests, [`Crane`] for the real
//! thing. The facts about `crane` that the code relies on, and what is NOT known about Docker Hub, are
//! in `docs/ARTIFACT-MEASUREMENTS.md`.
//!
//! Differences from files (`crate::stage`), on purpose: an image's identity is its digest, so a digest
//! that is already in the repository is content-addressed and harmless: it is read back and counted
//! (`AlreadyPublished`) without needing a ledger entry. The thing with a name to protect is the TAG:
//! a tag that exists at another digest is `Held`, before anything is pushed or moved.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::artifact::{Artifact, Kind};
use crate::ledger::{EventKind, Ledger, LedgerError};
use crate::registry::Clock;
use crate::stage::{Ctx, Outcome, Rec, StageReport, hash_from_sums, ours};

/// The registry, as far as image staging needs it. `repo` is `host/path` (an artifact's `target`).
pub trait ImageRegistry {
    /// The digest `reference` (a tag or a `sha256:` digest) resolves to: `Ok(None)` only when the
    /// registry definitively says it is not there.
    ///
    /// # Errors
    /// The failure text when the registry could not be asked (never read as "absent").
    fn manifest_digest(&mut self, repo: &str, reference: &str) -> Result<Option<String>, String>;
    /// Push the layout in `layout` (its manifest, children and blobs) to `repo` by `digest`, with no
    /// tag.
    ///
    /// # Errors
    /// The push's failure text; what it left behind is found by asking.
    fn push_by_digest(&mut self, layout: &Path, repo: &str, digest: &str) -> Result<(), String>;
    /// The exact bytes of the manifest stored under `digest`.
    ///
    /// # Errors
    /// The failure text.
    fn manifest_bytes(&mut self, repo: &str, digest: &str) -> Result<Vec<u8>, String>;
    /// Point `tag` at `digest` (the registry may overwrite an existing tag: callers check first).
    ///
    /// # Errors
    /// The failure text.
    fn tag(&mut self, repo: &str, digest: &str, tag: &str) -> Result<(), String>;
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for b in Sha256::digest(bytes) {
        write!(out, "{b:02x}").expect("writing to a String cannot fail");
    }
    out
}

fn valid_digest(d: &str) -> bool {
    d.strip_prefix("sha256:").is_some_and(|h| {
        h.len() == 64
            && h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// `os/arch[/variant]` of a descriptor's or a config's platform fields.
fn platform_of(v: &Value) -> Option<String> {
    let os = v.get("os")?.as_str()?;
    let arch = v.get("architecture")?.as_str()?;
    Some(match v.get("variant").and_then(Value::as_str) {
        Some(var) if !var.is_empty() => format!("{os}/{arch}/{var}"),
        _ => format!("{os}/{arch}"),
    })
}

/// What a manifest (or index) lists: the platforms and the child digests (none for a single image).
fn parse_top(bytes: &[u8]) -> Result<(BTreeSet<String>, Vec<String>), String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| format!("not JSON: {e}"))?;
    let Some(list) = v.get("manifests").and_then(Value::as_array) else {
        return Err("a single-platform manifest has no platform list".into());
    };
    let mut platforms = BTreeSet::new();
    let mut children = Vec::new();
    for m in list {
        let d = m.get("digest").and_then(Value::as_str).unwrap_or("");
        if !valid_digest(d) {
            return Err(format!("a listed manifest has a bad digest {d:?}"));
        }
        let p = m
            .get("platform")
            .and_then(platform_of)
            .ok_or_else(|| format!("listed manifest {d} has no os/architecture"))?;
        platforms.insert(p);
        children.push(d.to_owned());
    }
    Ok((platforms, children))
}

/// A local OCI layout, read and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The digest of the one top-level manifest or index.
    pub digest: String,
    /// The platforms it holds.
    pub platforms: BTreeSet<String>,
    /// The digests of its child manifests (empty for a single-platform image).
    pub children: Vec<String>,
}

fn blob(dir: &Path, digest: &str) -> Result<Vec<u8>, String> {
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or("not a sha256 digest")?;
    let bytes = fs::read(dir.join("blobs/sha256").join(hex))
        .map_err(|e| format!("blob {digest} could not be read: {e}"))?;
    let got = sha256_hex(&bytes);
    if got == hex {
        Ok(bytes)
    } else {
        Err(format!(
            "blob {digest} hashes to sha256:{got}: the layout is corrupt"
        ))
    }
}

/// Read the layout in `dir`: exactly one top-level manifest, every blob it needs present and equal to
/// its name, and its platforms. A single-platform image takes its platform from its config blob.
///
/// # Errors
/// What is wrong with the layout, naming the file or blob.
pub fn read_layout(dir: &Path) -> Result<Layout, String> {
    let idx: Value = serde_json::from_slice(
        &fs::read(dir.join("index.json"))
            .map_err(|e| format!("index.json could not be read: {e}"))?,
    )
    .map_err(|e| format!("index.json is not JSON: {e}"))?;
    let list = idx
        .get("manifests")
        .and_then(Value::as_array)
        .ok_or("index.json has no manifests list")?;
    let [one] = list.as_slice() else {
        return Err(format!(
            "index.json lists {} manifests; exactly one is expected",
            list.len()
        ));
    };
    let digest = one
        .get("digest")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    if !valid_digest(&digest) {
        return Err(format!(
            "index.json's digest {digest:?} is not sha256:<64 hex>"
        ));
    }
    let top = blob(dir, &digest)?;
    if let Ok((platforms, children)) = parse_top(&top) {
        for c in &children {
            blob(dir, c)?;
        }
        return Ok(Layout {
            digest,
            platforms,
            children,
        });
    }
    // A single manifest: its platform is in its config.
    let v: Value =
        serde_json::from_slice(&top).map_err(|e| format!("manifest is not JSON: {e}"))?;
    let cfg = v
        .pointer("/config/digest")
        .and_then(Value::as_str)
        .ok_or("the top manifest is neither an index nor an image manifest")?;
    let cfg: Value = serde_json::from_slice(&blob(dir, cfg)?)
        .map_err(|e| format!("config blob is not JSON: {e}"))?;
    let p = platform_of(&cfg).ok_or("the config blob has no os/architecture")?;
    Ok(Layout {
        digest,
        platforms: BTreeSet::from([p]),
        children: Vec::new(),
    })
}

/// The `repo` and layout directory of an image artifact, and its declared digest and platforms.
struct Declared<'a> {
    a: &'a Artifact,
    dir: PathBuf,
    digest: String,
    platforms: BTreeSet<String>,
}

fn declare<'a>(a: &'a Artifact, root: &Path) -> Result<Declared<'a>, String> {
    let src = a
        .source
        .as_deref()
        .and_then(|s| s.strip_prefix("dir:"))
        .ok_or("source is not dir:<path>")?;
    let digest = match (&a.digest, &a.sums) {
        (Some(d), _) => d.clone(),
        (None, Some(s)) => {
            let text = fs::read_to_string(root.join(s))
                .map_err(|e| format!("sums file {s:?} could not be read: {e}"))?;
            format!("sha256:{}", hash_from_sums(&text, src)?)
        }
        (None, None) => return Err("no digest and no sums".into()),
    };
    Ok(Declared {
        a,
        dir: root.join(src),
        digest,
        platforms: a.platforms.iter().flatten().cloned().collect(),
    })
}

fn diff(want: &BTreeSet<String>, got: &BTreeSet<String>) -> String {
    let missing: Vec<&String> = want.difference(got).collect();
    let extra: Vec<&String> = got.difference(want).collect();
    format!("declared but absent {missing:?}, present but undeclared {extra:?}")
}

/// Read an image back from the registry: the bytes served under `digest` hash to it, the platforms
/// are the declared ones, and every child manifest resolves to its own digest.
/// `Err` means the registry could not be asked; `Ok(Err(..))` is a definite failure.
fn read_back(
    reg: &mut dyn ImageRegistry,
    repo: &str,
    digest: &str,
) -> Result<Result<(), String>, String> {
    let bytes = reg.manifest_bytes(repo, digest)?;
    let got = format!("sha256:{}", sha256_hex(&bytes));
    if got != digest {
        return Ok(Err(format!(
            "read-back differs: the registry serves {got}, declared {digest}"
        )));
    }
    // The served bytes ARE the layout's manifest (same hash), whose platforms were compared with the
    // declaration before anything was pushed, so there is nothing further to compare here: what is
    // left to check is that the registry holds every child the index names.
    let children = match parse_top(&bytes) {
        Ok((_, children)) => children,
        // A single-platform image has no children to look for.
        Err(e) if e.contains("no platform list") => return Ok(Ok(())),
        Err(e) => return Ok(Err(format!("served manifest: {e}"))),
    };
    for c in children {
        match reg.manifest_digest(repo, &c)? {
            Some(d) if d == c => {}
            Some(d) => return Ok(Err(format!("child {c} resolves to {d}"))),
            None => {
                return Ok(Err(format!(
                    "child manifest {c} is missing from the registry"
                )));
            }
        }
    }
    Ok(Ok(()))
}

/// Stage the `image` artifacts of `artifacts`: verify every layout locally (digest and platforms must
/// equal the declaration), refuse any whose tags are taken by another digest, push by digest, read
/// back. No tag is created or moved (see [`promote_images`]). `root` is the artifacts directory.
///
/// # Errors
/// [`LedgerError`] when the ledger cannot be written; trouble with the registry is in the report.
pub fn stage_images(
    reg: &mut dyn ImageRegistry,
    ledger: &Ledger,
    clock: &mut dyn Clock,
    ctx: &Ctx,
    root: &Path,
    artifacts: &[Artifact],
) -> Result<StageReport, LedgerError> {
    let rec = Rec { ledger, ctx };
    let mut report = StageReport::default();
    let images: Vec<&Artifact> = artifacts.iter().filter(|a| a.kind == Kind::Image).collect();

    // 1. Local verification of every image before the registry is touched.
    let mut ok: Vec<Declared> = Vec::new();
    let mut results: Vec<(String, Option<Outcome>)> = Vec::new();
    for a in &images {
        let checked = declare(a, root).and_then(|d| {
            let l = read_layout(&d.dir)?;
            if l.digest != d.digest {
                return Err(format!(
                    "the layout holds {}, the declaration says {}; nothing was pushed",
                    l.digest, d.digest
                ));
            }
            if l.platforms != d.platforms {
                return Err(format!(
                    "the layout's platforms differ from the declaration: {}; nothing was pushed",
                    diff(&d.platforms, &l.platforms)
                ));
            }
            Ok(d)
        });
        match checked {
            Ok(d) => {
                rec.log(
                    clock,
                    EventKind::ArtifactVerified,
                    &a.name,
                    format!("digest={}", d.digest),
                )?;
                results.push((a.name.clone(), None));
                ok.push(d);
            }
            Err(why) => {
                rec.log(clock, EventKind::ArtifactFailed, &a.name, why.clone())?;
                results.push((a.name.clone(), Some(Outcome::Failed(why))));
            }
        }
    }
    if results.iter().any(|(_, o)| o.is_some()) {
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

    let earlier = ledger.read()?;
    'image: for d in ok {
        let (name, repo) = (d.a.name.as_str(), d.a.target.as_str());
        // 2. Tags taken by another digest are refused before anything is pushed.
        for tag in d.a.tags.iter().flatten() {
            match reg.manifest_digest(repo, tag) {
                Err(why) => {
                    report.stopped = Some(format!("{name}: could not ask about tag {tag}: {why}"));
                    return Ok(report);
                }
                Ok(Some(cur)) if cur != d.digest => {
                    let why = format!(
                        "{name}: tag {tag} already points at {cur}, not the declared {}; nothing was pushed or moved",
                        d.digest
                    );
                    rec.log(clock, EventKind::ArtifactHeld, name, why.clone())?;
                    report.outcomes.push((name.to_owned(), Outcome::Held(why)));
                    continue 'image;
                }
                Ok(_) => {}
            }
        }
        // 3. Is the digest already there? Then it is read back, not pushed again.
        let present = match reg.manifest_digest(repo, &d.digest) {
            Err(why) => {
                report.stopped = Some(format!("{name}: could not ask about {}: {why}", d.digest));
                return Ok(report);
            }
            Ok(p) => p.is_some(),
        };
        if !present {
            rec.log(
                clock,
                EventKind::ArtifactUploadStarted,
                name,
                format!("digest={}", d.digest),
            )?;
            if let Err(why) = reg.push_by_digest(&d.dir, repo, &d.digest) {
                rec.log(clock, EventKind::ArtifactFailed, name, why.clone())?;
                report
                    .outcomes
                    .push((name.to_owned(), Outcome::Failed(why)));
                continue;
            }
            rec.log(clock, EventKind::ArtifactUploadFinished, name, "")?;
        }
        // 4. Read back.
        match read_back(reg, repo, &d.digest) {
            Err(why) => {
                report.stopped = Some(format!("{name}: could not be read back: {why}"));
                report
                    .outcomes
                    .push((name.to_owned(), Outcome::Failed(why)));
                return Ok(report);
            }
            Ok(Err(why)) => {
                let why = format!("{name}: {why}");
                rec.log(clock, EventKind::ArtifactFailed, name, why.clone())?;
                report
                    .outcomes
                    .push((name.to_owned(), Outcome::Failed(why)));
            }
            Ok(Ok(())) => {
                let o = if present && !ours(&earlier, ctx, name) {
                    rec.log(
                        clock,
                        EventKind::ArtifactVisible,
                        name,
                        "digest already in the repository, read back",
                    )?;
                    Outcome::AlreadyPublished
                } else if present {
                    rec.log(
                        clock,
                        EventKind::ArtifactVisible,
                        name,
                        "already there, resumed",
                    )?;
                    Outcome::AlreadyPublished
                } else {
                    rec.log(
                        clock,
                        EventKind::ArtifactVisible,
                        name,
                        format!("digest={}", d.digest),
                    )?;
                    Outcome::Visible
                };
                report.outcomes.push((name.to_owned(), o));
            }
        }
    }
    Ok(report)
}

/// Give each staged image its tags (the user-visible commit point), after everything else in the plan
/// is confirmed. Every tag is checked first: one that points at another digest holds the whole image
/// and nothing is moved. Each tag is read back after it is set.
///
/// # Errors
/// [`LedgerError`] when the ledger cannot be written.
pub fn promote_images(
    reg: &mut dyn ImageRegistry,
    ledger: &Ledger,
    clock: &mut dyn Clock,
    ctx: &Ctx,
    root: &Path,
    artifacts: &[Artifact],
) -> Result<StageReport, LedgerError> {
    let rec = Rec { ledger, ctx };
    let mut report = StageReport::default();
    for a in artifacts.iter().filter(|a| a.kind == Kind::Image) {
        let name = a.name.as_str();
        let repo = a.target.as_str();
        let d = match declare(a, root) {
            Ok(d) => d,
            Err(why) => {
                rec.log(clock, EventKind::ArtifactFailed, name, why.clone())?;
                report
                    .outcomes
                    .push((name.to_owned(), Outcome::Failed(why)));
                continue;
            }
        };
        match reg.manifest_digest(repo, &d.digest) {
            Err(why) => {
                report.stopped = Some(format!("{name}: could not ask about {}: {why}", d.digest));
                return Ok(report);
            }
            Ok(None) => {
                let why = format!(
                    "{name}: {} is not in the repository; stage it first",
                    d.digest
                );
                rec.log(clock, EventKind::ArtifactFailed, name, why.clone())?;
                report
                    .outcomes
                    .push((name.to_owned(), Outcome::Failed(why)));
                continue;
            }
            Ok(Some(_)) => {}
        }
        // Pass 1: look at every tag; move nothing if any is taken.
        let mut todo = Vec::new();
        for tag in a.tags.iter().flatten() {
            match reg.manifest_digest(repo, tag) {
                Err(why) => {
                    report.stopped = Some(format!("{name}: could not ask about tag {tag}: {why}"));
                    return Ok(report);
                }
                Ok(Some(cur)) if cur == d.digest => {}
                Ok(Some(cur)) => {
                    let why = format!(
                        "{name}: tag {tag} already points at {cur}, not the declared {}; no tag was moved",
                        d.digest
                    );
                    rec.log(clock, EventKind::ArtifactHeld, name, why.clone())?;
                    report.outcomes.push((name.to_owned(), Outcome::Held(why)));
                    todo.clear();
                    break;
                }
                Ok(None) => todo.push(tag.clone()),
            }
        }
        if report
            .outcomes
            .last()
            .is_some_and(|(n, o)| n == name && matches!(o, Outcome::Held(_)))
        {
            continue;
        }
        // Pass 2: tag, then read each tag back.
        let all_there = todo.is_empty();
        let mut failed = None;
        for tag in &todo {
            if let Err(why) = reg.tag(repo, &d.digest, tag) {
                failed = Some(format!("{name}: tag {tag} could not be set: {why}"));
                break;
            }
            match reg.manifest_digest(repo, tag) {
                Err(why) => {
                    report.stopped =
                        Some(format!("{name}: tag {tag} could not be read back: {why}"));
                    return Ok(report);
                }
                Ok(Some(cur)) if cur == d.digest => {}
                Ok(other) => {
                    failed = Some(format!(
                        "{name}: tag {tag} reads back as {other:?}, not {}",
                        d.digest
                    ));
                    break;
                }
            }
        }
        let outcome = if let Some(why) = failed {
            rec.log(clock, EventKind::ArtifactFailed, name, why.clone())?;
            Outcome::Failed(why)
        } else {
            rec.log(
                clock,
                EventKind::ArtifactPromoted,
                name,
                format!(
                    "tags={}",
                    a.tags
                        .iter()
                        .flatten()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            )?;
            if all_there {
                Outcome::AlreadyPublished
            } else {
                Outcome::Visible
            }
        };
        report.outcomes.push((name.to_owned(), outcome));
    }
    Ok(report)
}

/// What `crane digest` said, classified: the digest, definitively absent, or could not be asked.
///
/// The error codes are from the real output in `docs/ARTIFACT-MEASUREMENTS.md` rows 5 and 6; a
/// message that carries neither is "could not ask", which fails safe.
///
/// # Errors
/// The last line of crane's stderr, or a digest that is not `sha256:<64 hex>`.
pub fn classify_digest(
    success: bool,
    stdout: &str,
    stderr: &str,
) -> Result<Option<String>, String> {
    if success {
        let d = stdout.trim();
        return if valid_digest(d) {
            Ok(Some(d.to_owned()))
        } else {
            Err(format!("crane printed {d:?}, not a sha256 digest"))
        };
    }
    if stderr.contains("MANIFEST_UNKNOWN") || stderr.contains("NAME_UNKNOWN") {
        return Ok(None);
    }
    Err(stderr
        .lines()
        .last()
        .unwrap_or("crane failed with no message")
        .trim()
        .to_owned())
}

/// The real registry client: shells out to `crane`.
#[derive(Debug, Clone)]
pub struct Crane {
    /// The `crane` executable.
    pub exe: PathBuf,
}

impl Crane {
    fn run(&self, args: &[&str]) -> Result<std::process::Output, String> {
        Command::new(&self.exe)
            .args(args)
            .output()
            .map_err(|e| format!("{} could not be run: {e}", self.exe.display()))
    }

    fn fail(out: &std::process::Output) -> String {
        String::from_utf8_lossy(&out.stderr)
            .lines()
            .last()
            .unwrap_or("crane failed with no message")
            .trim()
            .to_owned()
    }
}

impl Crane {
    /// The executable named by `$CRANE`, else `crane` on the PATH.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            exe: std::env::var_os("CRANE").map_or_else(|| PathBuf::from("crane"), PathBuf::from),
        }
    }

    /// Whether `crane` can be run, and its version line.
    ///
    /// # Errors
    /// The reason it cannot: not found, or it ran and failed.
    pub fn version(&self) -> Result<String, String> {
        let out = self.run(&["version"])?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        } else {
            Err(Self::fail(&out))
        }
    }
}

impl ImageRegistry for Crane {
    fn manifest_digest(&mut self, repo: &str, reference: &str) -> Result<Option<String>, String> {
        let sep = if reference.starts_with("sha256:") {
            '@'
        } else {
            ':'
        };
        let out = self.run(&["digest", &format!("{repo}{sep}{reference}")])?;
        classify_digest(
            out.status.success(),
            &String::from_utf8_lossy(&out.stdout),
            &String::from_utf8_lossy(&out.stderr),
        )
    }

    fn push_by_digest(&mut self, layout: &Path, repo: &str, digest: &str) -> Result<(), String> {
        let out = self.run(&[
            "push",
            &layout.to_string_lossy(),
            &format!("{repo}@{digest}"),
        ])?;
        if out.status.success() {
            Ok(())
        } else {
            Err(Self::fail(&out))
        }
    }

    fn manifest_bytes(&mut self, repo: &str, digest: &str) -> Result<Vec<u8>, String> {
        let out = self.run(&["manifest", &format!("{repo}@{digest}")])?;
        if out.status.success() {
            Ok(out.stdout)
        } else {
            Err(Self::fail(&out))
        }
    }

    fn tag(&mut self, repo: &str, digest: &str, tag: &str) -> Result<(), String> {
        let out = self.run(&["tag", &format!("{repo}@{digest}"), tag])?;
        if out.status.success() {
            Ok(())
        } else {
            Err(Self::fail(&out))
        }
    }
}

/// An in-memory registry for tests.
pub mod fake {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::{ImageRegistry, read_layout};

    /// Behaviour not scripted is the honest default, as measured on `registry:2`: tags are mutable
    /// (a `tag` call overwrites), a push by digest creates no tag.
    #[derive(Default)]
    pub struct FakeImages {
        /// `(repo, tag or digest)` to the digest it resolves to.
        pub refs: BTreeMap<(String, String), String>,
        /// `(repo, digest)` to the manifest bytes.
        pub bytes: BTreeMap<(String, String), Vec<u8>>,
        /// Every `push_by_digest`, as `repo@digest`.
        pub push_calls: Vec<String>,
        /// Every `tag` call, as `(repo, tag)`.
        pub tag_calls: Vec<(String, String)>,
        /// `push_by_digest` fails with this text, leaving nothing.
        pub push_error: Option<String>,
        /// Served manifest bytes differ from what was pushed.
        pub corrupt: bool,
        /// The first child manifest is not stored by a push.
        pub drop_child: bool,
        /// Every question fails with this text.
        pub ask_error: Option<String>,
        /// The next `manifest_bytes` fails once (a kill between push and read-back).
        pub bytes_fail_once: bool,
        /// A racing writer: right after a successful push, `(tag, digest)` is made to point there.
        pub race_tag: Option<(String, String)>,
    }

    impl FakeImages {
        /// An empty registry.
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }

        /// Make `tag` of `repo` point at `digest`.
        pub fn with_tag(mut self, repo: &str, tag: &str, digest: &str) -> Self {
            self.refs.insert((repo.into(), tag.into()), digest.into());
            self
        }
    }

    impl ImageRegistry for FakeImages {
        fn manifest_digest(
            &mut self,
            repo: &str,
            reference: &str,
        ) -> Result<Option<String>, String> {
            if let Some(e) = &self.ask_error {
                return Err(e.clone());
            }
            Ok(self
                .refs
                .get(&(repo.to_owned(), reference.to_owned()))
                .cloned())
        }

        fn push_by_digest(
            &mut self,
            layout: &Path,
            repo: &str,
            digest: &str,
        ) -> Result<(), String> {
            self.push_calls.push(format!("{repo}@{digest}"));
            if let Some(e) = &self.push_error {
                return Err(e.clone());
            }
            let l = read_layout(layout)?;
            let hex = |d: &str| d.trim_start_matches("sha256:").to_owned();
            let read = |d: &str| {
                std::fs::read(layout.join("blobs/sha256").join(hex(d))).map_err(|e| e.to_string())
            };
            self.bytes
                .insert((repo.into(), digest.into()), read(digest)?);
            self.refs
                .insert((repo.into(), digest.into()), digest.into());
            for (i, c) in l.children.iter().enumerate() {
                if self.drop_child && i == 0 {
                    continue;
                }
                self.bytes.insert((repo.into(), c.clone()), read(c)?);
                self.refs.insert((repo.into(), c.clone()), c.clone());
            }
            if let Some((tag, other)) = &self.race_tag {
                self.refs.insert((repo.into(), tag.clone()), other.clone());
            }
            Ok(())
        }

        fn manifest_bytes(&mut self, repo: &str, digest: &str) -> Result<Vec<u8>, String> {
            if std::mem::take(&mut self.bytes_fail_once) {
                return Err("timed out".into());
            }
            let mut b = self
                .bytes
                .get(&(repo.to_owned(), digest.to_owned()))
                .cloned()
                .ok_or_else(|| format!("{digest}: no such manifest"))?;
            if self.corrupt {
                b.push(b' ');
            }
            Ok(b)
        }

        fn tag(&mut self, repo: &str, digest: &str, tag: &str) -> Result<(), String> {
            self.tag_calls.push((repo.into(), tag.into()));
            self.refs.insert((repo.into(), tag.into()), digest.into());
            Ok(())
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::fake::FakeImages;
    use super::*;
    use crate::registry::fake::FakeClock;

    const SHA: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    const REPO: &str = "docker.io/thinkersjournal/demo";

    pub(crate) fn put(dir: &Path, bytes: &[u8]) -> String {
        let h = sha256_hex(bytes);
        fs::create_dir_all(dir.join("blobs/sha256")).unwrap();
        fs::write(dir.join("blobs/sha256").join(&h), bytes).unwrap();
        format!("sha256:{h}")
    }

    /// A layout under `root/dist/img` with an index over the given platforms; returns its digest.
    pub(crate) fn layout(root: &Path, platforms: &[(&str, &str)]) -> String {
        let dir = root.join("dist/img");
        let mut kids = Vec::new();
        for (os, arch) in platforms {
            let cfg = put(
                &dir,
                format!("{{\"os\":\"{os}\",\"architecture\":\"{arch}\"}}").as_bytes(),
            );
            let m = format!(
                "{{\"schemaVersion\":2,\"mediaType\":\"application/vnd.oci.image.manifest.v1+json\",\"config\":{{\"mediaType\":\"application/vnd.oci.image.config.v1+json\",\"digest\":\"{cfg}\",\"size\":1}},\"layers\":[]}}"
            );
            let d = put(&dir, m.as_bytes());
            kids.push(format!(
                "{{\"mediaType\":\"application/vnd.oci.image.manifest.v1+json\",\"digest\":\"{d}\",\"size\":1,\"platform\":{{\"os\":\"{os}\",\"architecture\":\"{arch}\"}}}}"
            ));
        }
        let idx = format!(
            "{{\"schemaVersion\":2,\"mediaType\":\"application/vnd.oci.image.index.v1+json\",\"manifests\":[{}]}}",
            kids.join(",")
        );
        let top = put(&dir, idx.as_bytes());
        fs::write(dir.join("oci-layout"), "{\"imageLayoutVersion\":\"1.0.0\"}").unwrap();
        fs::write(
            dir.join("index.json"),
            format!("{{\"schemaVersion\":2,\"manifests\":[{{\"mediaType\":\"application/vnd.oci.image.index.v1+json\",\"digest\":\"{top}\",\"size\":1}}]}}"),
        )
        .unwrap();
        top
    }

    struct Fix {
        root: tempfile::TempDir,
        digest: String,
        arts: Vec<Artifact>,
    }

    fn fix() -> Fix {
        let root = tempfile::tempdir().unwrap();
        let digest = layout(root.path(), &[("linux", "amd64"), ("linux", "arm64")]);
        let arts = vec![Artifact {
            name: "img".into(),
            kind: Kind::Image,
            path: None,
            source: Some("dir:dist/img".into()),
            sha256: None,
            digest: Some(digest.clone()),
            sums: None,
            target: REPO.into(),
            tags: Some(vec!["1.0.0".into(), "latest".into()]),
            platforms: Some(vec!["linux/amd64".into(), "linux/arm64".into()]),
        }];
        Fix { root, digest, arts }
    }

    fn ledger(dir: &Path) -> Ledger {
        Ledger::open(dir.join("ledger.jsonl")).unwrap()
    }

    fn stage(reg: &mut FakeImages, l: &Ledger, f: &Fix, sha: &str) -> StageReport {
        let ctx = Ctx {
            run_id: "r".into(),
            sha: sha.into(),
        };
        stage_images(
            reg,
            l,
            &mut FakeClock::default(),
            &ctx,
            f.root.path(),
            &f.arts,
        )
        .unwrap()
    }

    fn promote(reg: &mut FakeImages, l: &Ledger, f: &Fix) -> StageReport {
        let ctx = Ctx {
            run_id: "r".into(),
            sha: SHA.into(),
        };
        promote_images(
            reg,
            l,
            &mut FakeClock::default(),
            &ctx,
            f.root.path(),
            &f.arts,
        )
        .unwrap()
    }

    fn only(r: &StageReport) -> Outcome {
        assert_eq!(r.outcomes.len(), 1, "{r:?}");
        r.outcomes[0].1.clone()
    }

    #[test]
    fn a_layout_is_read_with_its_platforms_and_children() {
        let f = fix();
        let l = read_layout(&f.root.path().join("dist/img")).unwrap();
        assert_eq!(l.digest, f.digest);
        assert_eq!(
            l.platforms,
            BTreeSet::from(["linux/amd64".to_owned(), "linux/arm64".to_owned()])
        );
        assert_eq!(l.children.len(), 2);
    }

    #[test]
    fn a_single_platform_image_takes_its_platform_from_its_config() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("dist/img");
        let cfg = put(
            &dir,
            br#"{"os":"linux","architecture":"arm","variant":"v7"}"#,
        );
        let m = format!(
            "{{\"schemaVersion\":2,\"config\":{{\"digest\":\"{cfg}\",\"size\":1}},\"layers\":[]}}"
        );
        let top = put(&dir, m.as_bytes());
        fs::write(
            dir.join("index.json"),
            format!("{{\"manifests\":[{{\"digest\":\"{top}\"}}]}}"),
        )
        .unwrap();
        let l = read_layout(&dir).unwrap();
        assert_eq!(l.platforms, BTreeSet::from(["linux/arm/v7".to_owned()]));
        assert!(l.children.is_empty());
    }

    #[test]
    fn a_broken_layout_is_an_error_naming_what_is_wrong() {
        let f = fix();
        let dir = f.root.path().join("dist/img");
        // A blob whose bytes are not its name.
        let hex = f.digest.trim_start_matches("sha256:");
        fs::write(dir.join("blobs/sha256").join(hex), "tampered").unwrap();
        assert!(read_layout(&dir).unwrap_err().contains("layout is corrupt"));
        // No index.json at all.
        assert!(
            read_layout(&f.root.path().join("nowhere"))
                .unwrap_err()
                .contains("index.json")
        );
    }

    // The control for the rest: a clean registry, a correct declaration.
    #[test]
    fn staging_pushes_by_digest_reads_back_and_creates_no_tag() {
        let f = fix();
        let l = ledger(f.root.path());
        let mut reg = FakeImages::new();
        let r = stage(&mut reg, &l, &f, SHA);
        assert_eq!(only(&r), Outcome::Visible, "{r:?}");
        assert_eq!(reg.push_calls, [format!("{REPO}@{}", f.digest)]);
        assert!(reg.tag_calls.is_empty());
        assert!(
            !reg.refs
                .contains_key(&(REPO.to_owned(), "1.0.0".to_owned())),
            "nothing is pullable by name yet"
        );
        assert!(reg.refs.contains_key(&(REPO.to_owned(), f.digest.clone())));
    }

    #[test]
    fn promotion_tags_after_staging_and_reads_each_tag_back() {
        let f = fix();
        let l = ledger(f.root.path());
        let mut reg = FakeImages::new();
        assert!(stage(&mut reg, &l, &f, SHA).all_ok());
        let r = promote(&mut reg, &l, &f);
        assert_eq!(only(&r), Outcome::Visible, "{r:?}");
        assert_eq!(reg.tag_calls.len(), 2);
        for t in ["1.0.0", "latest"] {
            assert_eq!(reg.refs[&(REPO.to_owned(), t.to_owned())], f.digest);
        }
        let kinds: Vec<EventKind> = l.read().unwrap().iter().map(|e| e.event).collect();
        assert_eq!(kinds.last(), Some(&EventKind::ArtifactPromoted));
        // Running it again moves nothing.
        let r = promote(&mut reg, &l, &f);
        assert_eq!(only(&r), Outcome::AlreadyPublished);
        assert_eq!(reg.tag_calls.len(), 2);
    }

    #[test]
    fn a_layout_that_differs_from_its_declaration_touches_nothing() {
        let mut f = fix();
        f.arts[0].digest = Some(format!("sha256:{}", "0".repeat(64)));
        let mut reg = FakeImages::new();
        reg.ask_error = Some("must not be asked".into());
        let r = stage(&mut reg, &ledger(f.root.path()), &f, SHA);
        assert!(
            matches!(only(&r), Outcome::Failed(m) if m.contains("nothing was pushed")),
            "{r:?}"
        );
        assert!(
            reg.push_calls.is_empty() && r.stopped.is_none(),
            "the registry was never asked"
        );
        // Platforms that differ from the declaration.
        let mut f = fix();
        f.arts[0].platforms = Some(vec!["linux/amd64".into(), "linux/s390x".into()]);
        let mut reg2 = FakeImages::new();
        let r = stage(&mut reg2, &ledger(f.root.path()), &f, SHA);
        assert!(
            matches!(only(&r), Outcome::Failed(m) if m.contains("s390x") && m.contains("arm64")),
            "{r:?}"
        );
        assert!(
            reg2.push_calls.is_empty(),
            "refused locally, before any push"
        );
    }

    #[test]
    fn a_tag_taken_by_another_digest_holds_the_image_before_any_push_or_move() {
        let f = fix();
        let other = format!("sha256:{}", "9".repeat(64));
        let mut reg = FakeImages::new().with_tag(REPO, "latest", &other);
        let r = stage(&mut reg, &ledger(f.root.path()), &f, SHA);
        assert!(
            matches!(only(&r), Outcome::Held(m) if m.contains("latest") && m.contains("nothing was pushed")),
            "{r:?}"
        );
        assert!(reg.push_calls.is_empty());
        // Promotion also holds, and moves no tag, not even the free one.
        let mut reg = FakeImages::new();
        let l = ledger(tempfile::tempdir().unwrap().keep().as_path());
        assert!(stage(&mut reg, &l, &f, SHA).all_ok());
        reg.refs
            .insert((REPO.to_owned(), "latest".into()), other.clone());
        let r = promote(&mut reg, &l, &f);
        assert!(
            matches!(only(&r), Outcome::Held(m) if m.contains("no tag was moved")),
            "{r:?}"
        );
        assert!(reg.tag_calls.is_empty(), "1.0.0 stays free too");
        assert_eq!(
            reg.refs[&(REPO.to_owned(), "latest".to_owned())],
            other,
            "left alone"
        );
    }

    #[test]
    fn promotion_of_an_unstaged_image_fails() {
        let f = fix();
        let mut reg = FakeImages::new();
        let r = promote(&mut reg, &ledger(f.root.path()), &f);
        assert!(
            matches!(only(&r), Outcome::Failed(m) if m.contains("stage it first")),
            "{r:?}"
        );
        assert!(reg.tag_calls.is_empty());
    }

    #[test]
    fn served_bytes_that_differ_and_a_missing_child_are_not_visible() {
        let f = fix();
        let mut reg = FakeImages::new();
        reg.corrupt = true;
        let r = stage(&mut reg, &ledger(f.root.path()), &f, SHA);
        assert!(
            matches!(only(&r), Outcome::Failed(m) if m.contains("read-back differs")),
            "{r:?}"
        );
        let mut reg = FakeImages::new();
        reg.drop_child = true;
        let r = stage(
            &mut reg,
            &ledger(tempfile::tempdir().unwrap().keep().as_path()),
            &f,
            SHA,
        );
        assert!(
            matches!(only(&r), Outcome::Failed(m) if m.contains("missing from the registry")),
            "{r:?}"
        );
    }

    #[test]
    fn a_registry_that_cannot_be_asked_stops_the_run_and_pushes_nothing() {
        let f = fix();
        let mut reg = FakeImages::new();
        reg.ask_error = Some("dial tcp: connection refused".into());
        let r = stage(&mut reg, &ledger(f.root.path()), &f, SHA);
        assert!(
            r.stopped
                .as_deref()
                .is_some_and(|m| m.contains("connection refused")),
            "{r:?}"
        );
        assert!(reg.push_calls.is_empty(), "never read as absent");
    }

    #[test]
    fn a_failed_push_is_a_failure_and_the_next_run_pushes_again() {
        let f = fix();
        let l = ledger(f.root.path());
        let mut reg = FakeImages::new();
        reg.push_error = Some("blob upload unknown".into());
        let r = stage(&mut reg, &l, &f, SHA);
        assert!(
            matches!(only(&r), Outcome::Failed(m) if m.contains("blob upload")),
            "{r:?}"
        );
        reg.push_error = None;
        assert_eq!(only(&stage(&mut reg, &l, &f, SHA)), Outcome::Visible);
        assert_eq!(reg.push_calls.len(), 2);
    }

    // DESIGN section 6: a kill between push and read-back is resolved by asking the registry.
    #[test]
    fn a_kill_between_push_and_read_back_resumes_without_a_second_push() {
        let f = fix();
        let l = ledger(f.root.path());
        let mut reg = FakeImages::new();
        reg.bytes_fail_once = true;
        let r = stage(&mut reg, &l, &f, SHA);
        assert!(
            r.stopped
                .as_deref()
                .is_some_and(|m| m.contains("could not be read back")),
            "{r:?}"
        );
        assert_eq!(reg.push_calls.len(), 1);
        let r = stage(&mut reg, &l, &f, SHA);
        assert_eq!(only(&r), Outcome::AlreadyPublished, "{r:?}");
        assert_eq!(
            reg.push_calls.len(),
            1,
            "the digest was found by asking, not pushed again"
        );
    }

    #[test]
    fn classify_digest_reads_the_measured_crane_outputs() {
        let d = format!("sha256:{}", "d".repeat(64));
        assert_eq!(
            classify_digest(true, &format!("{d}\n"), ""),
            Ok(Some(d.clone()))
        );
        // Row 5: absent tag and absent repository (real wording).
        let absent = "2026/11/08 12:20:18 HEAD request failed, falling back on GET: HEAD http://127.0.0.1:5055/v2/team/app/manifests/9.9.9: unexpected status code 404 Not Found (HEAD responses have no body, use GET for details)\nError: GET http://127.0.0.1:5055/v2/team/app/manifests/9.9.9: MANIFEST_UNKNOWN: manifest unknown; map[Tag:9.9.9]\n";
        assert_eq!(classify_digest(false, "", absent), Ok(None));
        let norepo = "Error: reading tags for 127.0.0.1:5055/digestonly/app: GET http://127.0.0.1:5055/v2/digestonly/app/tags/list?n=1000: NAME_UNKNOWN: repository name not known to registry; map[name:digestonly/app]\n";
        assert_eq!(classify_digest(false, "", norepo), Ok(None));
        // Row 6: nothing listening is NOT absent.
        let down = "Error: Get \"https://127.0.0.1:5999/v2/\": dial tcp 127.0.0.1:5999: connectex: No connection could be made because the target machine actively refused it.\n";
        assert!(
            classify_digest(false, "", down)
                .unwrap_err()
                .contains("actively refused")
        );
        // An unrecognised failure and a malformed success both fail safe.
        assert!(
            classify_digest(false, "", "Error: UNAUTHORIZED: authentication required\n").is_err()
        );
        assert!(classify_digest(true, "oops", "").is_err());
    }

    #[test]
    fn a_crane_that_cannot_run_is_named_not_assumed() {
        let c = Crane {
            exe: PathBuf::from("C:/no/such/dir/crane-that-is-not-there"),
        };
        let why = c
            .version()
            .expect_err("a missing executable must not be reported as present");
        assert!(why.contains("could not be run"), "{why}");
    }
}
