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
    /// git failed.
    Git(String),
    /// I/O error.
    Io(io::Error),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFullSha(s) => write!(f, "not a full 40-hex commit SHA: {s:?}"),
            Self::DestinationNotEmpty => write!(f, "destination directory is not empty"),
            Self::NotACommit(s) => write!(f, "{s} is not a commit in this repository"),
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
