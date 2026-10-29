//! Tag a published release: an annotated `v<version>` on the exact commit it was published from,
//! pushed to origin and read back (CIRESNAVE-EXPECTATIONS 6.20).
//!
//! Rules, in the order they are checked: the SHA is a full SHA and a commit in this repository, it is
//! on some origin branch (a tag on a commit nobody can fetch is worthless), an existing tag of that
//! name, local or on origin, annotated or not, must already point at that commit and is **never
//! moved**, only that one tag is pushed, and the result is read back from origin. Without `execute`
//! everything up to the push is checked and nothing is written.

use std::fmt;
use std::path::Path;
use std::process::Command;

use semver::Version;

use crate::plan::Plan;
use crate::source::is_full_sha;

/// A self-contained git identity for the tag (tests). `None` in a [`TagRequest`] uses the
/// repository's own git configuration.
#[derive(Debug, Clone)]
pub struct Identity {
    /// `user.name`.
    pub name: String,
    /// `user.email`.
    pub email: String,
}

/// What to tag.
#[derive(Debug, Clone)]
pub struct TagRequest<'a> {
    /// The git repository (its `origin` remote is where the tag goes).
    pub repo: &'a Path,
    /// The project name, for the tag message (`<name> <version>`).
    pub name: &'a str,
    /// The released version; the tag is `v<version>`.
    pub version: &'a Version,
    /// The full commit SHA the release was published from.
    pub sha: &'a str,
    /// Create and push the tag. Without it nothing is written.
    pub execute: bool,
    /// Identity for the tag, or the repository's configuration.
    pub identity: Option<&'a Identity>,
}

/// What a tag request did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagOutcome {
    /// Every check passed; nothing was written (no `execute`).
    WouldTag,
    /// Origin already has this tag on this commit.
    AlreadyTagged,
    /// The tag was created, pushed and read back from origin.
    Tagged,
}

/// Why a tag was not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagError {
    /// The argument is not 40 lowercase hex digits.
    NotFullSha(String),
    /// The SHA is not a commit in this repository.
    NotACommit(String),
    /// The commit is on no origin branch.
    NotOnOrigin(String),
    /// A tag of that name already points at another commit; tags are never moved.
    TagPointsElsewhere {
        /// The tag name.
        tag: String,
        /// Where it points (the commit it peels to).
        have: String,
        /// Where it was asked to point.
        want: String,
        /// `origin` or `local`.
        place: &'static str,
    },
    /// Origin does not show the tag on the commit after the push.
    ReadBackMismatch {
        /// The tag name.
        tag: String,
        /// What origin showed (empty when absent).
        got: String,
    },
    /// git failed.
    Git(String),
}

impl fmt::Display for TagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFullSha(s) => write!(
                f,
                "not a full 40-hex commit SHA: {s:?} (get one with `git rev-parse {s}`)"
            ),
            Self::NotACommit(s) => write!(f, "{s} is not a commit in this repository"),
            Self::NotOnOrigin(s) => write!(
                f,
                "{s} is on no origin branch; push it first (`git push origin <branch>`), then tag"
            ),
            Self::TagPointsElsewhere {
                tag,
                have,
                want,
                place,
            } => write!(
                f,
                "{tag} already points at {have} on {place}, not {want}; tags are never moved"
            ),
            Self::ReadBackMismatch { tag, got } => {
                write!(f, "pushed {tag} but origin shows {got:?} for it")
            }
            Self::Git(m) => write!(f, "git failed: {m}"),
        }
    }
}

impl std::error::Error for TagError {}

/// Tag (or check that it could be tagged, or find it already tagged).
///
/// # Errors
/// See [`TagError`]. Nothing is pushed unless every check passed.
pub fn tag_release(req: &TagRequest<'_>) -> Result<TagOutcome, TagError> {
    if !is_full_sha(req.sha) {
        return Err(TagError::NotFullSha(req.sha.to_owned()));
    }
    let tag = format!("v{}", req.version);
    let git = |args: &[&str]| run_git(req.repo, &[], args);

    git(&["fetch", "-q", "origin"])?;
    let commit = format!("{}^{{commit}}", req.sha);
    if git(&["cat-file", "-e", &commit]).is_err() {
        return Err(TagError::NotACommit(req.sha.to_owned()));
    }
    if git(&["branch", "-r", "--contains", req.sha])?.is_empty() {
        return Err(TagError::NotOnOrigin(req.sha.to_owned()));
    }

    if let Some(have) = origin_tag_commit(req.repo, &tag)? {
        return if have == req.sha {
            Ok(TagOutcome::AlreadyTagged)
        } else {
            Err(TagError::TagPointsElsewhere {
                tag,
                have,
                want: req.sha.to_owned(),
                place: "origin",
            })
        };
    }

    let local = format!("refs/tags/{tag}^{{commit}}");
    let local_have = git(&["rev-parse", "-q", "--verify", &local]).ok();
    if let Some(have) = &local_have
        && have != req.sha
    {
        return Err(TagError::TagPointsElsewhere {
            tag,
            have: have.clone(),
            want: req.sha.to_owned(),
            place: "local",
        });
    }
    if !req.execute {
        return Ok(TagOutcome::WouldTag);
    }

    if local_have.is_none() {
        let identity: Vec<String> = req
            .identity
            .map(|i| {
                vec![
                    "-c".into(),
                    format!("user.name={}", i.name),
                    "-c".into(),
                    format!("user.email={}", i.email),
                    "-c".into(),
                    "tag.gpgsign=false".into(),
                ]
            })
            .unwrap_or_default();
        let identity: Vec<&str> = identity.iter().map(String::as_str).collect();
        let message = format!("{} {}", req.name, req.version);
        run_git(
            req.repo,
            &identity,
            &["tag", "-a", &tag, req.sha, "-m", &message],
        )?;
    }
    git(&["push", "-q", "origin", &format!("refs/tags/{tag}")])?;
    match origin_tag_commit(req.repo, &tag)? {
        Some(got) if got == req.sha => Ok(TagOutcome::Tagged),
        got => Err(TagError::ReadBackMismatch {
            tag,
            got: got.unwrap_or_default(),
        }),
    }
}

/// The version a publish run is tagged with: the one version every crate in the plan shares (a
/// workspace at one version gets one tag), or the explicit `--tag-version` when the crates are at
/// several versions (for example a crate that follows another project's number). Never a guess.
///
/// # Errors
/// A message naming the versions and the flag to pass, when they differ and none was given.
pub fn release_version(plan: &Plan, explicit: Option<&Version>) -> Result<Version, String> {
    if let Some(v) = explicit {
        return Ok(v.clone());
    }
    let mut versions: Vec<&Version> = plan.order.iter().map(|c| &c.version).collect();
    versions.sort();
    versions.dedup();
    match versions.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err("the plan is empty, so there is no version to tag".to_owned()),
        many => Err(format!(
            "the crates are at several versions ({}); pass --tag-version to choose the one to tag",
            many.iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// `git -C repo <config> <args>`: trimmed stdout, or git's stderr as the error.
fn run_git(repo: &Path, config: &[&str], args: &[&str]) -> Result<String, TagError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(config)
        .args(args)
        .output()
        .map_err(|e| TagError::Git(e.to_string()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(TagError::Git(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ))
    }
}

/// The commit `tag` points at on origin, or `None` when origin has no such tag. An annotated tag
/// is read through its peeled `^{}` line; a lightweight tag has none, so its own line is the
/// commit. A lookup that fails is an error, never "absent".
fn origin_tag_commit(repo: &Path, tag: &str) -> Result<Option<String>, TagError> {
    let direct = format!("refs/tags/{tag}");
    let peeled = format!("{direct}^{{}}");
    // One call for both names; a process spawn is the expensive part of every git step.
    let text = run_git(
        repo,
        &[],
        &["ls-remote", "--tags", "origin", &direct, &peeled],
    )?;
    let find = |name: &str| {
        text.lines().find_map(|l| {
            let (oid, r) = l.split_once('\t')?;
            (r == name).then(|| oid.to_owned())
        })
    };
    Ok(find(&peeled).or_else(|| find(&direct)))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

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
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        origin: std::path::PathBuf,
        work: std::path::PathBuf,
        /// Pushed commits, oldest first.
        pushed: Vec<String>,
    }

    impl Fixture {
        /// A bare `origin`, a clone `work`, and two commits pushed to `main`.
        fn new() -> Self {
            let tmp = tempfile::tempdir().unwrap();
            let (origin, work) = (tmp.path().join("origin.git"), tmp.path().join("work"));
            fs::create_dir_all(&origin).unwrap();
            git(&origin, &["init", "-q", "--bare", "-b", "main"]);
            fs::create_dir_all(&work).unwrap();
            git(&work, &["init", "-q", "-b", "main"]);
            git(
                &work,
                &["remote", "add", "origin", origin.to_str().unwrap()],
            );
            let mut f = Self {
                _tmp: tmp,
                origin,
                work,
                pushed: vec![],
            };
            for n in ["one", "two"] {
                let sha = f.commit(n);
                f.pushed.push(sha);
            }
            git(&f.work, &["push", "-q", "origin", "main"]);
            f
        }

        fn commit(&self, name: &str) -> String {
            fs::write(self.work.join(name), name).unwrap();
            git(&self.work, &["add", "-A"]);
            git(&self.work, &["commit", "-q", "-m", name]);
            git(&self.work, &["rev-parse", "HEAD"])
        }

        fn id() -> Identity {
            Identity {
                name: "t".into(),
                email: "t@example.invalid".into(),
            }
        }

        fn run(&self, sha: &str, execute: bool) -> Result<TagOutcome, TagError> {
            let v: Version = "0.1.0".parse().unwrap();
            tag_release(&TagRequest {
                repo: &self.work,
                name: "demo",
                version: &v,
                sha,
                execute,
                identity: Some(&Self::id()),
            })
        }

        /// What origin holds for `v0.1.0`: (object type, commit it peels to), or `None`.
        fn origin_tag(&self) -> Option<(String, String)> {
            let out = Command::new("git")
                .arg("-C")
                .arg(&self.origin)
                .args(["rev-parse", "-q", "--verify", "refs/tags/v0.1.0^{commit}"])
                .output()
                .unwrap();
            if !out.status.success() {
                return None;
            }
            let commit = String::from_utf8(out.stdout).unwrap().trim().to_owned();
            Some((
                git(&self.origin, &["cat-file", "-t", "refs/tags/v0.1.0"]),
                commit,
            ))
        }
    }

    #[test]
    fn tags_the_exact_sha_with_an_annotated_tag_and_reads_it_back() {
        let f = Fixture::new();
        let first = f.pushed[0].clone();
        assert_ne!(
            first, f.pushed[1],
            "the tag target is not HEAD, so the test can tell"
        );
        assert_eq!(f.run(&first, true), Ok(TagOutcome::Tagged));
        assert_eq!(f.origin_tag(), Some(("tag".to_owned(), first)));
        assert!(
            git(&f.origin, &["cat-file", "-p", "refs/tags/v0.1.0"]).contains("demo 0.1.0"),
            "the message names the project and version"
        );
    }

    #[test]
    fn without_execute_every_check_runs_and_nothing_is_written() {
        let f = Fixture::new();
        assert_eq!(f.run(&f.pushed[0], false), Ok(TagOutcome::WouldTag));
        assert_eq!(f.origin_tag(), None);
        assert_eq!(git(&f.work, &["tag", "--list"]), "", "no local tag either");
        // A dry run still fails on a bad request, so it is a real preview.
        assert!(matches!(
            f.run(&"0".repeat(40), false),
            Err(TagError::NotACommit(_))
        ));
    }

    #[test]
    fn a_commit_on_no_origin_branch_is_refused_and_nothing_is_pushed() {
        let f = Fixture::new();
        let local_only = f.commit("three");
        assert!(matches!(
            f.run(&local_only, true),
            Err(TagError::NotOnOrigin(_))
        ));
        assert_eq!(f.origin_tag(), None);
        // Control: the same commit, once pushed, tags.
        git(&f.work, &["push", "-q", "origin", "main"]);
        assert_eq!(f.run(&local_only, true), Ok(TagOutcome::Tagged));
    }

    #[test]
    fn an_existing_tag_on_the_same_commit_is_fine_and_on_another_is_never_moved() {
        let f = Fixture::new();
        let (first, second) = (f.pushed[0].clone(), f.pushed[1].clone());
        assert_eq!(f.run(&first, true), Ok(TagOutcome::Tagged));
        assert_eq!(f.run(&first, true), Ok(TagOutcome::AlreadyTagged));
        match f.run(&second, true) {
            Err(TagError::TagPointsElsewhere {
                have, want, place, ..
            }) => assert_eq!((have, want, place), (first.clone(), second, "origin")),
            other => panic!("expected TagPointsElsewhere, got {other:?}"),
        }
        assert_eq!(f.origin_tag(), Some(("tag".to_owned(), first)));
    }

    // The shell script this replaces looks only at the peeled `^{}` line, which a lightweight tag
    // does not have, so it would have tried to push over it.
    #[test]
    fn a_lightweight_tag_on_origin_at_another_commit_is_refused_too() {
        let f = Fixture::new();
        let (first, second) = (f.pushed[0].clone(), f.pushed[1].clone());
        git(
            &f.work,
            &["push", "-q", "origin", &format!("{first}:refs/tags/v0.1.0")],
        );
        assert!(matches!(
            f.run(&second, true),
            Err(TagError::TagPointsElsewhere {
                place: "origin",
                ..
            })
        ));
        assert_eq!(f.origin_tag(), Some(("commit".to_owned(), first)));
    }

    #[test]
    fn a_stale_local_tag_at_another_commit_is_refused_not_overwritten() {
        let f = Fixture::new();
        let (first, second) = (f.pushed[0].clone(), f.pushed[1].clone());
        git(&f.work, &["tag", "v0.1.0", &second]);
        assert!(matches!(
            f.run(&first, true),
            Err(TagError::TagPointsElsewhere { place: "local", .. })
        ));
        assert_eq!(f.origin_tag(), None, "nothing was pushed");
        assert_eq!(git(&f.work, &["rev-parse", "v0.1.0"]), second);
    }

    fn plan_of(crates: &[(&str, &str)]) -> Plan {
        Plan {
            order: crates
                .iter()
                .map(|(n, v)| crate::plan::PlannedCrate {
                    name: (*n).to_owned(),
                    version: v.parse().unwrap(),
                    depends_on: Default::default(),
                })
                .collect(),
        }
    }

    #[test]
    fn the_release_version_is_the_shared_one_or_the_explicit_one_never_a_guess() {
        let one = plan_of(&[("a", "0.5.0"), ("b", "0.5.0")]);
        assert_eq!(release_version(&one, None), Ok("0.5.0".parse().unwrap()));

        let two = plan_of(&[("a", "0.32.0"), ("b", "0.1.0-alpha.44"), ("c", "0.32.0")]);
        let err = release_version(&two, None).unwrap_err();
        assert!(
            err.contains("0.32.0")
                && err.contains("0.1.0-alpha.44")
                && err.contains("--tag-version"),
            "{err}"
        );
        let pick: Version = "0.32.0".parse().unwrap();
        assert_eq!(release_version(&two, Some(&pick)), Ok(pick.clone()));
        // An explicit version is taken as given, also when the plan has only one (it is the user's word).
        let other: Version = "0.9.0".parse().unwrap();
        assert_eq!(release_version(&one, Some(&other)), Ok(other));
    }

    #[test]
    fn a_branch_name_is_refused_with_how_to_get_the_sha() {
        let f = Fixture::new();
        let err = f.run("main", true).unwrap_err();
        assert!(err.to_string().contains("git rev-parse main"), "{err}");
    }
}
