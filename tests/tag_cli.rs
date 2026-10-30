//! `releaseant tag` end to end: the real binary, a real repository, a bare repository as origin.
//! No network and no registry.

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

fn releaseant(work: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .current_dir(work)
        .args(args)
        .output()
        .unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .replace("\r\n", "\n")
}

#[test]
fn tag_dry_run_then_execute_then_again_then_a_commit_not_on_origin() {
    let tmp = tempfile::tempdir().unwrap();
    let (origin, work) = (tmp.path().join("origin.git"), tmp.path().join("work"));
    fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    // The tool uses the repository's own identity, like a real user's.
    git(&work, &["config", "user.name", "t"]);
    git(&work, &["config", "user.email", "t@example.invalid"]);
    git(
        &work,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    fs::write(work.join("a"), "a").unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "one"]);
    let sha = git(&work, &["rev-parse", "HEAD"]);
    git(&work, &["push", "-q", "origin", "main"]);

    let base = [
        "tag",
        "--sha",
        sha.as_str(),
        "--version",
        "0.1.0",
        "--name",
        "demo",
    ];
    let origin_has_tag = || {
        Command::new("git")
            .arg("-C")
            .arg(&origin)
            .args(["rev-parse", "-q", "--verify", "refs/tags/v0.1.0"])
            .output()
            .unwrap()
            .status
            .success()
    };

    // Dry run by default: says what it would do, writes nothing.
    let out = releaseant(&work, &base);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains(&format!("would tag v0.1.0 -> {sha}")),
        "{}",
        text(&out)
    );
    assert!(
        text(&out).contains("--execute"),
        "says how to do it: {}",
        text(&out)
    );
    assert!(!origin_has_tag());

    // --execute tags and reads it back.
    let mut exec = base.to_vec();
    exec.push("--execute");
    let out = releaseant(&work, &exec);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains(&format!("tagged v0.1.0 -> {sha} (read back from origin)")),
        "{}",
        text(&out)
    );
    assert!(origin_has_tag());

    // Again: idempotent.
    let out = releaseant(&work, &exec);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains(&format!("already tagged v0.1.0 -> {sha}")),
        "{}",
        text(&out)
    );

    // A commit nobody can fetch: refused with exit code 2 and what to do.
    fs::write(work.join("b"), "b").unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "two"]);
    let local = git(&work, &["rev-parse", "HEAD"]);
    let out = releaseant(
        &work,
        &[
            "tag",
            "--sha",
            &local,
            "--version",
            "0.2.0",
            "--name",
            "demo",
            "--execute",
        ],
    );
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("on no origin branch"), "{}", text(&out));
    assert!(text(&out).contains("git push"), "{}", text(&out));
}

// A crate released on its own (`tag --crate`) gets `<crate>-v<version>` and no `v<version>`.
#[test]
fn tag_crate_makes_a_crate_tag_and_not_the_project_tag() {
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
    fs::write(work.join("a"), "a").unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "one"]);
    let sha = git(&work, &["rev-parse", "HEAD"]);
    git(&work, &["push", "-q", "origin", "main"]);

    let out = releaseant(
        &work,
        &[
            "tag",
            "--sha",
            &sha,
            "--version",
            "0.1.0",
            "--crate",
            "demo-kernels",
            "--execute",
        ],
    );
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains(&format!(
            "tagged demo-kernels-v0.1.0 -> {sha} (read back from origin)"
        )),
        "{}",
        text(&out)
    );
    let tags = git(&origin, &["tag", "--list"]);
    assert_eq!(tags, "demo-kernels-v0.1.0", "and no v0.1.0");
    // `publish --only` cannot be combined with a version to tag.
    let out = releaseant(
        &work,
        &[
            "publish",
            "--sha",
            &sha,
            "--only",
            "x",
            "--tag",
            "--tag-version",
            "0.1.0",
        ],
    );
    assert!(!out.status.success());
    assert!(text(&out).contains("cannot be used with"), "{}", text(&out));
}
