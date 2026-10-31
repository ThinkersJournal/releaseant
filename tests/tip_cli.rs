//! `releaseant publish --execute` refuses a commit that is not origin's tip: the real binary, a real
//! repository, a bare repository as origin. The repository holds no crates, so a run that gets past
//! the tip check fails later for another reason; that is the control. No network and no registry.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
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

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .replace("\r\n", "\n")
}

fn publish(work: &Path, sha: &str, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .current_dir(work)
        .args(["publish", "--sha", sha])
        .args(extra)
        .output()
        .unwrap()
}

#[test]
fn an_execute_run_needs_the_tip_of_origin() {
    let tmp = tempfile::tempdir().unwrap();
    let (origin, work) = (tmp.path().join("origin.git"), tmp.path().join("work"));
    fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    git(&work, &["config", "user.name", "t"]);
    git(&work, &["config", "user.email", "t@example.invalid"]);
    git(
        &work,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    fs::write(work.join("a"), "1").unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "one"]);
    let old = git(&work, &["rev-parse", "HEAD"]);
    fs::write(work.join("a"), "2").unwrap();
    git(&work, &["commit", "-q", "-am", "two"]);
    let tip = git(&work, &["rev-parse", "HEAD"]);
    git(&work, &["push", "-q", "origin", "main"]);

    // The older commit is refused, and the refusal names the tip.
    let out = publish(&work, &old, &["--execute"]);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(2), "{t}");
    assert!(t.contains("not the tip of origin's refs/heads/main"), "{t}");
    assert!(t.contains(&tip), "{t}");

    // Controls: the tip, the override, and a dry run of the old commit all get past the tip check
    // (they fail or stop later, because the repository holds no crates).
    for (sha, extra) in [
        (&tip, vec!["--execute"]),
        (&old, vec!["--execute", "--allow-not-tip"]),
        (&old, vec![]),
    ] {
        let t = text(&publish(&work, sha, &extra));
        assert!(!t.contains("not the tip"), "{extra:?}: {t}");
    }
}
