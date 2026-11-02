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

// What `gh api .../check-runs --jq '.check_runs[] | {name, status, conclusion}'` prints: one object a line.
// Fuel's shape: the required checks are done, two self-hosted runs are queued, an unrelated one failed.
const CHECKS: &str = concat!(
    r#"{"name":"Clippy","status":"completed","conclusion":"success"}"#,
    "\n",
    r#"{"name":"checkpoint-tests","status":"queued","conclusion":null}"#,
    "\n",
    r#"{"name":"lavapipe-vulkan","status":"completed","conclusion":"failure"}"#,
    "\n",
);

#[test]
fn require_checks_judges_only_the_named_checks_and_fails_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).unwrap();
    let gh = fake_gh(tmp.path(), CHECKS);

    // The named check passed; the queued and the failed run are not named, so they do not count.
    let (code, t) = publish(&work, &gh, &["--execute", "--require-checks", "Clippy"]);
    assert!(
        t.contains("judging the 1 check(s) named by --require-checks: Clippy"),
        "{t}"
    );
    assert!(t.contains("CI is green"), "{t}");
    assert_ne!(code, Some(3), "{t}");
    assert_ne!(code, Some(4), "{t}");

    // A named check that is on the commit but not finished is no answer, not green.
    let (code, t) = publish(
        &work,
        &gh,
        &[
            "--execute",
            "--ci-wait",
            "0",
            "--require-checks",
            "Clippy,checkpoint-tests",
        ],
    );
    assert_eq!(code, Some(4), "{t}");
    assert!(t.contains("checkpoint-tests (queued)"), "{t}");

    // A named check that is absent is no answer too.
    let (code, t) = publish(
        &work,
        &gh,
        &[
            "--execute",
            "--ci-wait",
            "0",
            "--require-checks",
            "Packaged crates",
        ],
    );
    assert_eq!(code, Some(4), "{t}");
    assert!(t.contains("Packaged crates (absent)"), "{t}");

    // A named check that failed is red.
    let (code, t) = publish(
        &work,
        &gh,
        &["--execute", "--require-checks", "lavapipe-vulkan"],
    );
    assert_eq!(code, Some(3), "{t}");
    assert!(t.contains("RED") && t.contains("lavapipe-vulkan"), "{t}");
}

#[test]
fn with_no_names_an_unreadable_branch_protection_falls_back_and_the_line_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).unwrap();
    // The stand-in answers every call with a list of run objects: not a list of check names, so the
    // protection read fails and the guard judges every run, as it did before this option existed.
    let gh = fake_gh(
        tmp.path(),
        r#"[{"status":"completed","conclusion":"success"}]"#,
    );
    let (code, t) = publish(&work, &gh, &["--execute"]);
    assert!(
        t.contains("could not read branch protection of main") && t.contains("judging every run"),
        "{t}"
    );
    assert!(t.contains("CI is green"), "{t}");
    assert_ne!(code, Some(3), "{t}");
}

#[test]
fn an_empty_check_name_exits_2_at_once_and_never_asks_the_forge() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    fs::create_dir_all(&work).unwrap();
    // A stand-in that would answer, were it asked: the guard must not get as far as asking.
    let gh = fake_gh(tmp.path(), CHECKS);
    for value in ["", "Clippy,,Rustfmt"] {
        let (code, t) = publish(&work, &gh, &["--execute", "--require-checks", value]);
        assert_eq!(code, Some(2), "{value:?}: {t}");
        assert!(t.contains("empty"), "{t}");
        assert!(!t.contains("waiting for CI"), "failed before waiting: {t}");
    }
    // The control: the same command with a real name goes on to the guard.
    let (_, t) = publish(&work, &gh, &["--execute", "--require-checks", "Clippy"]);
    assert!(t.contains("waiting for CI"), "{t}");
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
    let out = Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .current_dir(tmp.path())
        .args(["publish", "--sha", SHA, "--require-checks", "Clippy"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}
