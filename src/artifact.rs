//! `[[artifact]]` in `releaseant.toml`: installers and container images that are published beside the
//! crates (`docs/ARTIFACTS-DESIGN.md`, slice A1). This module only reads, validates and names them;
//! nothing here touches a forge or a registry, and nothing here builds or signs anything.
//!
//! ```toml
//! [[artifact]]
//! name   = "humboldt-installer"
//! kind   = "file"
//! path   = "dist/humboldt.msi"
//! sha256 = "<64 lowercase hex>"          # or: sums = "dist/SHA256SUMS"
//! target = "github-release"
//!
//! [[artifact]]
//! name      = "fuel-image"
//! kind      = "image"
//! source    = "dir:dist/fuel-oci"           # an OCI image layout directory
//! digest    = "sha256:<64 lowercase hex>"  # or: sums = "dist/SHA256SUMS"
//! target    = "docker.io/thinkersjournal/fuel"
//! tags      = ["0.3.1"]
//! platforms = ["linux/amd64", "linux/arm64"]
//! ```
//!
//! Every problem of every artifact is reported at once, and a field that does not belong to the kind
//! is an error (a `tags` list on a file would otherwise be silently ignored).

use std::collections::BTreeSet;
use std::fmt;

use serde::Deserialize;

/// The only target a `file` artifact has so far.
pub const TARGET_GITHUB_RELEASE: &str = "github-release";

/// What kind of thing is published.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A file uploaded as a GitHub release asset.
    File,
    /// An OCI image pushed to a registry.
    Image,
}

impl Kind {
    /// The word used in the manifest and in output.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Image => "image",
        }
    }
}

/// One `[[artifact]]` table as written. Unknown keys are errors.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    /// Unique name in the plan; the ledger key.
    pub name: String,
    /// `file` or `image`.
    pub kind: Kind,
    /// file: the path of the file, relative to the artifacts directory.
    pub path: Option<String>,
    /// image: `dir:<path>`, a local OCI image layout (`oci-layout`, `index.json`, `blobs/`). An
    /// OCI archive tarball is not accepted: `crane` cannot push one (`docs/ARTIFACT-MEASUREMENTS.md` row 8).
    pub source: Option<String>,
    /// file: the declared SHA-256, 64 lowercase hex digits.
    pub sha256: Option<String>,
    /// image: the declared manifest or index digest, `sha256:` and 64 lowercase hex digits.
    pub digest: Option<String>,
    /// A `SHA256SUMS` file, relative to the artifacts directory, that CI wrote in the same run; the
    /// declared hash is read from it later. Exactly one of this and `sha256`/`digest`.
    pub sums: Option<String>,
    /// file: `github-release`; image: `host/path` with no tag or digest.
    pub target: String,
    /// image: tags to point at the digest once everything is confirmed.
    pub tags: Option<Vec<String>>,
    /// image: the platforms the index must hold, `os/arch` or `os/arch/variant`.
    pub platforms: Option<Vec<String>>,
}

/// Every problem found in a set of artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactError(pub Vec<String>);

impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} artifact problem(s)", self.0.len())?;
        for p in &self.0 {
            write!(f, "\n  {p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ArtifactError {}

fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A relative path that stays inside its directory (no drive, no root, no `..`).
fn relative_inside(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with(['/', '\\'])
        && !p.contains(':')
        && !p.contains('\0')
        && p.split(['/', '\\']).all(|c| c != ".." && !c.is_empty())
}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 100
        && n.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// The OCI distribution spec's tag grammar: `[A-Za-z0-9_][A-Za-z0-9._-]{0,127}`.
fn valid_tag(t: &str) -> bool {
    let b = t.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && (b[0].is_ascii_alphanumeric() || b[0] == b'_')
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

fn valid_platform(p: &str) -> bool {
    let parts: Vec<&str> = p.split('/').collect();
    (2..=3).contains(&parts.len())
        && parts.iter().all(|s| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// `host/path[/path...]`, lowercase, no scheme, no tag, no digest.
fn valid_image_target(t: &str) -> bool {
    if t.contains("://") || t.contains('@') || t.is_empty() {
        return false;
    }
    let parts: Vec<&str> = t.split('/').collect();
    if parts.len() < 2 {
        return false;
    }
    // A colon is allowed only as a host port, i.e. in the first component.
    if parts[1..].iter().any(|c| c.contains(':')) {
        return false;
    }
    parts.iter().all(|c| {
        !c.is_empty()
            && c.bytes().all(|b| {
                b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || matches!(b, b'.' | b'-' | b'_' | b':')
            })
    })
}

/// Validate a set of artifacts, listing every problem of every one.
///
/// # Errors
/// [`ArtifactError`] with one line per problem.
pub fn validate(artifacts: &[Artifact]) -> Result<(), ArtifactError> {
    let mut errs = Vec::new();
    let mut seen = BTreeSet::new();
    for a in artifacts {
        let who = format!("artifact {:?}", a.name);
        let mut bad = |m: String| errs.push(format!("{who}: {m}"));
        if !valid_name(&a.name) {
            bad("name must be 1 to 100 characters of letters, digits, `.`, `_`, `-`".into());
        }
        if !seen.insert(a.name.as_str()) {
            bad("name is used by another artifact".into());
        }
        let wrong = |present: &[(&str, bool)], kind: Kind, bad: &mut dyn FnMut(String)| {
            for (field, here) in present {
                if *here {
                    bad(format!(
                        "`{field}` does not belong to kind {:?}",
                        kind.word()
                    ));
                }
            }
        };
        match a.kind {
            Kind::File => {
                wrong(
                    &[
                        ("source", a.source.is_some()),
                        ("digest", a.digest.is_some()),
                        ("tags", a.tags.is_some()),
                        ("platforms", a.platforms.is_some()),
                    ],
                    a.kind,
                    &mut bad,
                );
                match &a.path {
                    None => bad("kind file needs `path`".into()),
                    Some(p) if !relative_inside(p) => {
                        bad(format!(
                            "path {p:?} must be relative and stay inside the artifacts directory"
                        ));
                    }
                    Some(_) => {}
                }
                match (&a.sha256, &a.sums) {
                    (Some(_), Some(_)) => bad("give `sha256` or `sums`, not both".into()),
                    (None, None) => bad("kind file needs `sha256` or `sums`".into()),
                    (Some(h), None) if !is_hex64(h) => {
                        bad("sha256 must be 64 lowercase hex digits".into());
                    }
                    _ => {}
                }
                if a.target != TARGET_GITHUB_RELEASE {
                    bad(format!(
                        "target {:?} is not supported for a file (only {TARGET_GITHUB_RELEASE:?})",
                        a.target
                    ));
                }
            }
            Kind::Image => {
                wrong(
                    &[("path", a.path.is_some()), ("sha256", a.sha256.is_some())],
                    a.kind,
                    &mut bad,
                );
                match &a.source {
                    None => bad("kind image needs `source`".into()),
                    Some(s) if s.starts_with("oci-archive:") => bad(format!(
                        "source {s:?}: an OCI tar is not accepted (crane cannot push one); give `dir:<relative path>`, an OCI image layout directory (docs/ARTIFACTS-DESIGN.md section 4.2)"
                    )),
                    Some(s) => match s.strip_prefix("dir:") {
                        Some(p) if relative_inside(p) => {}
                        _ => bad(format!(
                            "source {s:?} must be `dir:<relative path>` (an OCI image layout directory)"
                        )),
                    },
                }
                match (&a.digest, &a.sums) {
                    (Some(_), Some(_)) => bad("give `digest` or `sums`, not both".into()),
                    (None, None) => bad("kind image needs `digest` or `sums`".into()),
                    (Some(d), None) if !d.strip_prefix("sha256:").is_some_and(is_hex64) => {
                        bad("digest must be `sha256:` and 64 lowercase hex digits".into());
                    }
                    _ => {}
                }
                if !valid_image_target(&a.target) {
                    bad(format!(
                        "target {:?} must be `host/path` in lowercase, with no scheme, tag or digest",
                        a.target
                    ));
                }
                match &a.tags {
                    None => bad("kind image needs `tags`".into()),
                    Some(t) if t.is_empty() => bad("`tags` is empty".into()),
                    Some(t) => {
                        let mut tags = BTreeSet::new();
                        for tag in t {
                            if !valid_tag(tag) {
                                bad(format!("tag {tag:?} is not a valid OCI tag"));
                            }
                            if !tags.insert(tag.as_str()) {
                                bad(format!("tag {tag:?} is listed twice"));
                            }
                        }
                    }
                }
                match &a.platforms {
                    None => bad("kind image needs `platforms`".into()),
                    Some(p) if p.is_empty() => bad("`platforms` is empty".into()),
                    Some(p) => {
                        let mut set = BTreeSet::new();
                        for plat in p {
                            if !valid_platform(plat) {
                                bad(format!(
                                    "platform {plat:?} must be `os/arch` or `os/arch/variant`"
                                ));
                            }
                            if !set.insert(plat.as_str()) {
                                bad(format!("platform {plat:?} is listed twice"));
                            }
                        }
                    }
                }
            }
        }
        // A sums file path is a path inside the artifacts directory too.
        if let Some(s) = a.sums.as_ref().filter(|s| !relative_inside(s)) {
            bad(format!(
                "sums {s:?} must be relative and stay inside the artifacts directory"
            ));
        }
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(ArtifactError(errs))
    }
}

/// A stable text identity of the declarations, in manifest order, for the plan hash: empty when
/// there are none, so a project without artifacts keeps the fingerprint it had before this existed
/// and an old ledger still resumes. Any change to a declared name, kind, hash, target, tag or
/// platform changes it.
#[must_use]
pub fn fingerprint(artifacts: &[Artifact]) -> String {
    artifacts
        .iter()
        .map(|a| {
            let item = match a.kind {
                Kind::File => a.path.as_deref().unwrap_or(""),
                Kind::Image => a.source.as_deref().unwrap_or(""),
            };
            let hash = a.sha256.as_deref().or(a.digest.as_deref()).map_or_else(
                || format!("sums={}", a.sums.as_deref().unwrap_or("")),
                str::to_owned,
            );
            format!(
                "{}:{}={}@{}>{}#{}#{}",
                a.kind.word(),
                a.name,
                item,
                hash,
                a.target,
                a.tags.as_deref().unwrap_or(&[]).join(","),
                a.platforms.as_deref().unwrap_or(&[]).join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// The plan hash of crates and artifacts together: the crates' fingerprint unchanged when there are
/// no artifacts, else with `|artifacts:` and the artifacts' fingerprint appended.
#[must_use]
pub fn combined_fingerprint(crates: &str, artifacts: &[Artifact]) -> String {
    if artifacts.is_empty() {
        crates.to_owned()
    } else {
        format!("{crates}|artifacts:{}", fingerprint(artifacts))
    }
}

/// One line per artifact for `plan`'s output.
#[must_use]
pub fn describe(a: &Artifact) -> String {
    match a.kind {
        Kind::File => format!(
            "artifact {} (file {} -> {}, {})",
            a.name,
            a.path.as_deref().unwrap_or("?"),
            a.target,
            hash_word(a)
        ),
        Kind::Image => format!(
            "artifact {} (image {} -> {} tags {} platforms {}, {})",
            a.name,
            a.source.as_deref().unwrap_or("?"),
            a.target,
            a.tags.as_deref().unwrap_or(&[]).join(","),
            a.platforms.as_deref().unwrap_or(&[]).join(","),
            hash_word(a)
        ),
    }
}

fn hash_word(a: &Artifact) -> String {
    match (&a.sha256, &a.digest, &a.sums) {
        (Some(h), _, _) => format!("sha256 {h}"),
        (_, Some(d), _) => format!("digest {d}"),
        (_, _, Some(s)) => format!("hash from {s} (read at run time)"),
        _ => "no hash".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn file() -> Artifact {
        Artifact {
            name: "installer".into(),
            kind: Kind::File,
            path: Some("dist/x.msi".into()),
            source: None,
            sha256: Some(H.into()),
            digest: None,
            sums: None,
            target: TARGET_GITHUB_RELEASE.into(),
            tags: None,
            platforms: None,
        }
    }

    fn image() -> Artifact {
        Artifact {
            name: "img".into(),
            kind: Kind::Image,
            path: None,
            source: Some("dir:dist/i-oci".into()),
            sha256: None,
            digest: Some(format!("sha256:{H}")),
            sums: None,
            target: "docker.io/thinkersjournal/fuel".into(),
            tags: Some(vec!["0.3.1".into()]),
            platforms: Some(vec!["linux/amd64".into(), "linux/arm64".into()]),
        }
    }

    fn problems(a: &[Artifact]) -> Vec<String> {
        validate(a).err().map(|e| e.0).unwrap_or_default()
    }

    fn has(a: &[Artifact], needle: &str) -> bool {
        problems(a).iter().any(|p| p.contains(needle))
    }

    // The control: both well-formed examples pass, so every refusal below is the check's doing.
    #[test]
    fn well_formed_artifacts_pass() {
        assert_eq!(validate(&[file(), image()]), Ok(()));
        assert_eq!(validate(&[]), Ok(()));
    }

    #[test]
    fn a_bad_hash_is_refused_for_both_kinds() {
        let mut f = file();
        f.sha256 = Some(H[..63].into());
        assert!(has(&[f], "64 lowercase hex"));
        let mut f = file();
        f.sha256 = Some(H.to_uppercase());
        assert!(
            has(&[f], "64 lowercase hex"),
            "upper case is not the declared form"
        );
        let mut i = image();
        i.digest = Some(H.into());
        assert!(has(&[i], "`sha256:`"));
    }

    #[test]
    fn a_duplicate_name_and_a_bad_name_are_refused() {
        let mut b = file();
        b.name = "installer".into();
        assert!(has(&[file(), b], "used by another"));
        let mut c = file();
        c.name = "has space".into();
        assert!(has(&[c], "name must be"));
    }

    #[test]
    fn hash_and_sums_are_exactly_one() {
        let mut f = file();
        f.sums = Some("dist/SHA256SUMS".into());
        assert!(has(&[f], "not both"));
        let mut f = file();
        f.sha256 = None;
        assert!(has(&[f.clone()], "needs `sha256` or `sums`"));
        f.sums = Some("dist/SHA256SUMS".into());
        assert_eq!(validate(&[f]), Ok(()), "sums alone is fine");
    }

    #[test]
    fn a_field_of_the_other_kind_is_an_error() {
        let mut f = file();
        f.tags = Some(vec!["x".into()]);
        assert!(has(&[f], "`tags` does not belong"));
        let mut i = image();
        i.path = Some("x".into());
        assert!(has(&[i], "`path` does not belong"));
    }

    #[test]
    fn paths_must_stay_inside_the_artifacts_directory() {
        for bad in ["../x", "/abs/x", "C:/x", "a//b", "a/../b", "", "\\x"] {
            let mut f = file();
            f.path = Some(bad.into());
            assert!(has(&[f], "must be relative"), "{bad:?}");
        }
        let mut f = file();
        f.sha256 = None;
        f.sums = Some("../SUMS".into());
        assert!(has(&[f], "sums"));
    }

    #[test]
    fn image_target_tags_and_platforms_are_checked() {
        for bad in [
            "fuel",
            "https://docker.io/a/b",
            "docker.io/a/b:1",
            "docker.io/A/b",
            "docker.io/a/b@sha256:x",
            "",
        ] {
            let mut i = image();
            i.target = bad.into();
            assert!(has(&[i], "target"), "{bad:?}");
        }
        let mut i = image();
        i.target = "localhost:5000/team/app".into();
        assert_eq!(validate(&[i]), Ok(()), "a host port is allowed");
        let mut i = image();
        i.tags = Some(vec!["-bad".into(), "ok".into(), "ok".into()]);
        let p = problems(&[i]);
        assert!(p.iter().any(|m| m.contains("not a valid OCI tag")), "{p:?}");
        assert!(p.iter().any(|m| m.contains("listed twice")), "{p:?}");
        let mut i = image();
        i.tags = Some(vec![]);
        assert!(has(&[i], "`tags` is empty"));
        let mut i = image();
        i.platforms = Some(vec![
            "linux".into(),
            "linux/amd64".into(),
            "linux/amd64".into(),
        ]);
        let p = problems(&[i]);
        assert!(p.iter().any(|m| m.contains("os/arch")), "{p:?}");
        assert!(p.iter().any(|m| m.contains("listed twice")), "{p:?}");
        let mut i = image();
        i.source = Some("docker-daemon:x".into());
        assert!(has(&[i], "OCI image layout"));
        let mut i = image();
        i.source = Some("oci-archive:dist/i.tar".into());
        assert!(
            has(&[i], "an OCI tar is not accepted"),
            "a named error, not the generic one: an archive tarball cannot be pushed by crane"
        );
    }

    #[test]
    fn a_file_target_other_than_github_release_is_refused() {
        let mut f = file();
        f.target = "s3".into();
        assert!(has(&[f], "not supported"));
    }

    #[test]
    fn every_problem_of_every_artifact_is_listed() {
        let mut f = file();
        f.sha256 = Some("x".into());
        f.target = "s3".into();
        let mut i = image();
        i.tags = Some(vec![]);
        assert!(problems(&[f, i]).len() >= 3);
    }

    // The negative control for the plan hash: with no artifacts the fingerprint is the crates' own,
    // byte for byte (an old ledger resumes); any change of a declaration changes it.
    #[test]
    fn the_plan_hash_changes_with_a_declaration_and_not_without_artifacts() {
        assert_eq!(combined_fingerprint("a@1;b@2", &[]), "a@1;b@2");
        let base = combined_fingerprint("a@1", &[file(), image()]);
        assert_ne!(base, "a@1");
        let mut changed = vec![file(), image()];
        changed[0].sha256 = Some(H.replace('0', "1"));
        assert_ne!(
            combined_fingerprint("a@1", &changed),
            base,
            "a declared hash"
        );
        let mut changed = vec![file(), image()];
        changed[1].tags = Some(vec!["0.3.2".into()]);
        assert_ne!(combined_fingerprint("a@1", &changed), base, "a tag");
        let mut changed = vec![file(), image()];
        changed[1].platforms = Some(vec!["linux/amd64".into()]);
        assert_ne!(combined_fingerprint("a@1", &changed), base, "a platform");
        assert_eq!(
            combined_fingerprint("a@1", &[file(), image()]),
            base,
            "same input, same hash"
        );
    }

    #[test]
    fn describe_names_kind_target_and_where_the_hash_comes_from() {
        assert!(describe(&file()).contains("github-release"));
        assert!(describe(&image()).contains("linux/arm64"));
        let mut f = file();
        f.sha256 = None;
        f.sums = Some("dist/SHA256SUMS".into());
        assert!(describe(&f).contains("read at run time"));
    }
}
