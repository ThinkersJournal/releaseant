//! Publishing source: `git archive <sha>` into an empty directory (DESIGN section 2, trap 1).
//!
//! A working tree can hold uncommitted edits and untracked files that the packaged crate would
//! then contain. The archive holds exactly the commit.

use std::fmt;
use std::io;
use std::path::Path;
use std::process::Command;

/// Why a source archive could not be made.
#[derive(Debug)]
pub enum SourceError {
    /// The argument is not 40 lowercase hex digits (a branch, tag or abbreviation is refused).
    NotFullSha(String),
    /// The destination exists and is not empty.
    DestinationNotEmpty,
    /// The SHA is not a commit in the repository.
    NotACommit(String),
    /// `--repo` is not a git repository (missing, or not one), with git's own message.
    NotARepository {
        /// The path as given.
        repo: String,
        /// What git said.
        why: String,
    },
    /// git failed.
    Git(String),
    /// I/O error.
    Io(io::Error),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFullSha(s) => write!(
                f,
                "not a full 40-hex commit SHA: {s:?} (get one with `git rev-parse {s}`)"
            ),
            Self::DestinationNotEmpty => write!(f, "destination directory is not empty"),
            Self::NotACommit(s) => write!(f, "{s} is not a commit in this repository"),
            Self::NotARepository { repo, why } => {
                write!(f, "{repo} is not a git repository: {why}")
            }
            Self::Git(m) => write!(f, "git failed: {m}"),
            Self::Io(e) => write!(f, "I/O: {e}"),
        }
    }
}

impl std::error::Error for SourceError {}

impl From<io::Error> for SourceError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Whether `s` is a full lowercase 40-hex SHA.
#[must_use]
pub fn is_full_sha(s: &str) -> bool {
    s.len() == 40
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Extract the tree of commit `sha` from `repo` into `dest` (created if missing, must be empty).
///
/// # Errors
/// See [`SourceError`]. Nothing is written to `dest` unless the SHA is a commit.
pub fn archive(repo: &Path, sha: &str, dest: &Path) -> Result<(), SourceError> {
    if !is_full_sha(sha) {
        return Err(SourceError::NotFullSha(sha.to_owned()));
    }
    if dest.exists() && dest.read_dir()?.next().is_some() {
        return Err(SourceError::DestinationNotEmpty);
    }
    // `cat-file` exits 128 for a missing directory and for an unknown SHA alike, so ask first
    // whether `repo` is a repository at all and pass git's own words on.
    let probe = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--git-dir"])
        .output()?;
    if !probe.status.success() {
        return Err(SourceError::NotARepository {
            repo: repo.display().to_string(),
            why: String::from_utf8_lossy(&probe.stderr).trim().to_owned(),
        });
    }
    let commit = format!("{sha}^{{commit}}");
    let check = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "-e", &commit])
        .output()?;
    if !check.status.success() {
        return Err(SourceError::NotACommit(sha.to_owned()));
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        // The user's core.autocrlf would rewrite line endings in the archive, so the same commit
        // would package differently on different machines.
        .args(["-c", "core.autocrlf=false", "archive", "--format=tar", sha])
        .output()?;
    if !out.status.success() {
        return Err(SourceError::Git(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ));
    }
    std::fs::create_dir_all(dest)?;
    tar::Archive::new(out.stdout.as_slice()).unpack(dest)?;
    Ok(())
}

/// Why the commit is not safe to publish as the project's tip.
#[derive(Debug, PartialEq, Eq)]
pub enum TipError {
    /// origin's default branch is at another commit.
    NotTip {
        /// The branch origin's `HEAD` names.
        branch: String,
        /// The commit that branch is at on origin.
        tip: String,
    },
    /// origin could not be asked, or answered in a shape we do not read. Never read as "is the tip".
    Unknown(String),
}

impl fmt::Display for TipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotTip { branch, tip } => write!(
                f,
                "this commit is not the tip of origin's {branch} ({tip}); publish the tip, or pass --allow-not-tip"
            ),
            Self::Unknown(why) => write!(
                f,
                "could not read origin's tip ({why}); fix that, or pass --allow-not-tip"
            ),
        }
    }
}

impl std::error::Error for TipError {}

/// Check that `sha` is the commit origin's default branch is at *now*: asked of origin itself with
/// `git ls-remote --symref origin HEAD`, not of a local remote-tracking ref that may be stale.
///
/// # Errors
/// [`TipError::NotTip`] when origin is elsewhere; [`TipError::Unknown`] when origin cannot be asked.
pub fn check_tip(repo: &Path, sha: &str) -> Result<(), TipError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-remote", "--symref", "origin", "HEAD"])
        .output()
        .map_err(|e| TipError::Unknown(e.to_string()))?;
    if !out.status.success() {
        return Err(TipError::Unknown(
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let branch = text
        .lines()
        .find_map(|l| {
            l.strip_prefix("ref: ")?
                .split_once('\t')
                .map(|(r, _)| r.to_owned())
        })
        .ok_or_else(|| TipError::Unknown(format!("no symbolic HEAD in {text:?}")))?;
    let tip = text
        .lines()
        .find_map(|l| {
            let (oid, r) = l.split_once('\t')?;
            (r == "HEAD" && !oid.starts_with("ref: ")).then(|| oid.to_owned())
        })
        .ok_or_else(|| TipError::Unknown(format!("no HEAD commit in {text:?}")))?;
    if tip == sha {
        Ok(())
    } else {
        Err(TipError::NotTip { branch, tip })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    // T1: the archive is the commit, not the working tree.
    #[test]
    fn archive_is_the_commit_not_the_working_tree() {
        let repo = tempfile::tempdir().unwrap();
        let r = repo.path();
        git(r, &["init", "-q"]);
        fs::write(r.join("tracked.txt"), "committed\n").unwrap();
        git(r, &["add", "-A"]);
        git(r, &["commit", "-q", "-m", "one"]);
        let sha = git(r, &["rev-parse", "HEAD"]);
        fs::write(r.join("tracked.txt"), "edited after the commit\n").unwrap();
        fs::write(r.join("untracked.txt"), "never committed\n").unwrap();

        // Control: the working tree really does differ, so the assertions below can tell the two apart.
        assert_eq!(
            fs::read_to_string(r.join("tracked.txt")).unwrap(),
            "edited after the commit\n"
        );
        assert!(r.join("untracked.txt").exists());

        let out = tempfile::tempdir().unwrap();
        archive(r, &sha, out.path()).unwrap();
        assert_eq!(
            fs::read_to_string(out.path().join("tracked.txt")).unwrap(),
            "committed\n"
        );
        assert!(!out.path().join("untracked.txt").exists());
        assert!(!out.path().join(".git").exists());
    }

    // A stranger types `--sha HEAD` or `--sha main` first. The error must say what to do next.
    #[test]
    fn a_branch_name_is_refused_with_the_command_that_gets_the_sha() {
        let msg = SourceError::NotFullSha("main".into()).to_string();
        assert!(msg.contains("git rev-parse main"), "{msg}");
        // The wording for a too-short SHA (the other way to get here) is just as actionable.
        let msg = SourceError::NotFullSha("6c1b56c".into()).to_string();
        assert!(msg.contains("git rev-parse 6c1b56c"), "{msg}");
    }

    // Found when a shell-quoting slip made `--repo` name a directory that did not exist and the
    // tool answered "not a commit". The control (a real repository and an unknown SHA is still
    // `NotACommit`) is in the test below.
    #[test]
    fn a_missing_or_non_repository_path_is_reported_as_such_with_gits_message() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let sha = "6c1b56cb6574f6d49035bf36e5de88df8262e124";

        let missing = tmp.path().join("C:Projects$r");
        match archive(&missing, sha, out.path()) {
            Err(SourceError::NotARepository { repo, why }) => {
                assert!(repo.contains("C:Projects$r"), "{repo}");
                assert!(
                    why.contains("cannot change to") || why.contains("No such file"),
                    "git's own message is kept: {why}"
                );
            }
            other => panic!("expected NotARepository, got {other:?}"),
        }

        // A directory that exists but is no repository (the tempdir is not inside one).
        assert!(matches!(
            archive(tmp.path(), sha, out.path()),
            Err(SourceError::NotARepository { .. })
        ));
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    // The tip check asks origin itself; the control is the same repository with the tip as input.
    #[test]
    fn check_tip_accepts_the_tip_and_refuses_an_older_commit_and_an_unreachable_origin() {
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin.git");
        let work = tmp.path().join("work");
        fs::create_dir_all(&work).unwrap();
        git(
            tmp.path(),
            &["init", "-q", "--bare", "-b", "main", "origin.git"],
        );
        git(&work, &["init", "-q", "-b", "main"]);
        git(
            &work,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        fs::write(work.join("a"), "1").unwrap();
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-q", "-m", "one"]);
        let first = git(&work, &["rev-parse", "HEAD"]);
        fs::write(work.join("a"), "2").unwrap();
        git(&work, &["commit", "-q", "-am", "two"]);
        let second = git(&work, &["rev-parse", "HEAD"]);
        git(&work, &["push", "-q", "origin", "main"]);

        assert_eq!(check_tip(&work, &second), Ok(()));
        match check_tip(&work, &first) {
            Err(TipError::NotTip { branch, tip }) => {
                assert_eq!(branch, "refs/heads/main");
                assert_eq!(tip, second);
            }
            other => panic!("expected NotTip, got {other:?}"),
        }
        // A local commit that origin has not seen is not the tip either.
        fs::write(work.join("a"), "3").unwrap();
        git(&work, &["commit", "-q", "-am", "three"]);
        let third = git(&work, &["rev-parse", "HEAD"]);
        assert!(matches!(
            check_tip(&work, &third),
            Err(TipError::NotTip { .. })
        ));
        // origin gone: unknown, never "is the tip".
        fs::remove_dir_all(&origin).unwrap();
        assert!(matches!(
            check_tip(&work, &second),
            Err(TipError::Unknown(_))
        ));
    }

    #[test]
    fn refuses_branch_names_abbreviations_unknown_shas_and_dirty_destinations() {
        let repo = tempfile::tempdir().unwrap();
        let r = repo.path();
        git(r, &["init", "-q"]);
        fs::write(r.join("a"), "x").unwrap();
        git(r, &["add", "-A"]);
        git(r, &["commit", "-q", "-m", "one"]);
        let sha = git(r, &["rev-parse", "HEAD"]);
        let out = tempfile::tempdir().unwrap();

        assert!(matches!(
            archive(r, "HEAD", out.path()),
            Err(SourceError::NotFullSha(_))
        ));
        assert!(matches!(
            archive(r, &sha[..12], out.path()),
            Err(SourceError::NotFullSha(_))
        ));
        assert!(matches!(
            archive(r, &"0".repeat(40), out.path()),
            Err(SourceError::NotACommit(_))
        ));
        fs::write(out.path().join("stray"), "x").unwrap();
        assert!(matches!(
            archive(r, &sha, out.path()),
            Err(SourceError::DestinationNotEmpty)
        ));
        assert_eq!(
            fs::read_dir(out.path()).unwrap().count(),
            1,
            "nothing was written"
        );
    }
}
