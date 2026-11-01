//! `releaseant publish --require-green`: the real binary with a stand-in `gh` (named by
//! `RELEASEANT_GH`) that prints what `gh run list --json status,conclusion` would. A red commit is
//! refused with exit status 3 before anything is built; a green one gets past the guard (and then
//! fails for another reason, because the repository holds no crates: that is the control).
//! No network and no registry.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// A program that prints `json` and exits 0, for `RELEASEANT_GH`.
fn fake_gh(dir: &Path, json: &str) -> PathBuf {
    fs::write(dir.join("runs.json"), json).unwrap();
    make_program(dir)
}

#[cfg(windows)]
fn make_program(dir: &Path) -> PathBuf {
    let p = dir.join("gh.cmd");
    fs::write(&p, "@echo off\r\ntype \"%~dp0runs.json\"\r\n").unwrap();
    p
}

#[cfg(unix)]
fn make_program(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("gh");
    fs::write(&p, "#!/bin/sh\ncat \"$(dirname \"$0\")/runs.json\"\n").unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn publish(work: &Path, gh: &Path, extra: &[&str]) -> (Option<i32>, String) {
    let out: Output = Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .current_dir(work)
        .env("RELEASEANT_GH", gh)
        .args([
            "publish",
            "--sha",
            SHA,
            "--require-green",
            "--github",
            "o/n",
        ])
        .args(extra)
        .output()
        .unwrap();
    let t = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .replace("\r\n", "\n");
    (out.status.code(), t)
}

#[test]
fn a_red_commit_is_refused_with_exit_3_and_a_green_one_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).unwrap();

    let red = fake_gh(
        tmp.path(),
        r#"[{"status":"completed","conclusion":"success"},{"status":"completed","conclusion":"failure"}]"#,
    );
    let (code, t) = publish(&work, &red, &["--execute"]);
    assert_eq!(code, Some(3), "{t}");
    assert!(t.contains("RED") && t.contains("not publishing"), "{t}");

    // The control: the same command with green runs gets past the guard.
    let green = fake_gh(
        tmp.path(),
        r#"[{"status":"completed","conclusion":"success"}]"#,
    );
    let (code, t) = publish(&work, &green, &["--execute"]);
    assert!(t.contains("CI is green"), "{t}");
    assert!(!t.contains("RED"), "{t}");
    assert_ne!(code, Some(3), "{t}");
}

#[test]
fn ci_that_never_appears_is_not_green() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).unwrap();
    let none = fake_gh(tmp.path(), "[]");
    let (code, t) = publish(&work, &none, &["--ci-wait", "0"]);
    assert_eq!(code, Some(4), "{t}");
    assert!(t.contains("no answer"), "{t}");
}

#[test]
fn the_ci_options_need_the_guard_and_retries_need_execute() {
    let tmp = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .current_dir(tmp.path())
        .args(["publish", "--sha", SHA, "--retries", "2"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .current_dir(tmp.path())
        .args(["publish", "--sha", SHA, "--ci-wait", "5"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}
