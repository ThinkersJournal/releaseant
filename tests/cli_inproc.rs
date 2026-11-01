//! The command line run in process, through `releaseant::cli::run_code`: exit statuses and the
//! ledger a run leaves. (Output text is covered by `tests/readme.rs` and the other `*_cli.rs` tests,
//! which run the binary.) No network: the fixture's crates have no outside dependencies.

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;

use releaseant::cli::run_code;
use releaseant::ledger::{Event, EventKind, Ledger};

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

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn manifest(name: &str, deps: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\ndescription = \"fixture\"\n\
         license = \"MIT\"\n\n[dependencies]\n{deps}"
    )
}

const CORE: &str = "zzcli-core";
const APP: &str = "zzcli-app";

/// A committed two-crate workspace (`app` depends on `core`); returns the directory guard and SHA.
///
/// `who` (the test's name) goes into the commit message so that each test has its own SHA. The run
/// id is the SHA's first 8 characters, the second and the process id; these tests share one process
/// and run in parallel, so two fixtures with the same SHA would share one scratch directory.
fn fixture(who: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let w = dir.path().join("repo");
    fs::create_dir_all(&w).unwrap();
    git(&w, &["init", "-q", "-b", "main"]);
    git(&w, &["config", "user.name", "t"]);
    git(&w, &["config", "user.email", "t@example.invalid"]);
    write(
        &w.join("Cargo.toml"),
        &format!("[workspace]\nresolver = \"2\"\nmembers = [\"{CORE}\", \"{APP}\"]\n"),
    );
    write(&w.join(CORE).join("Cargo.toml"), &manifest(CORE, ""));
    write(&w.join(CORE).join("src/lib.rs"), "pub fn f() {}\n");
    write(
        &w.join(APP).join("Cargo.toml"),
        &manifest(
            APP,
            &format!("{CORE} = {{ path = \"../{CORE}\", version = \"0.1.0\" }}\n"),
        ),
    );
    write(
        &w.join(APP).join("src/lib.rs"),
        "pub fn g() { zzcli_core::f() }\n",
    );
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "-q", "-m", who]);
    let sha = git(&w, &["rev-parse", "HEAD"]);
    (dir, sha)
}

fn code(args: &[&str]) -> u8 {
    let mut v: Vec<OsString> = vec!["releaseant".into()];
    v.extend(args.iter().map(OsString::from));
    run_code(v)
}

fn events(ledger: &Path) -> Vec<Event> {
    Ledger::open(ledger).unwrap().read().unwrap()
}

#[test]
fn usage_help_and_version_exit_like_clap_does() {
    assert_eq!(code(&["--version"]), 0);
    assert_eq!(code(&["--help"]), 0);
    // Parse errors are status 2, as with `Cli::parse`.
    assert_eq!(code(&[]), 2);
    assert_eq!(code(&["no-such-command"]), 2);
    assert_eq!(code(&["plan"]), 2, "--sha is required");
}

#[test]
fn plan_succeeds_and_refuses_unknown_selections() {
    let (dir, sha) = fixture("plan_succeeds_and_refuses_unknown_selections");
    let repo = dir.path().join("repo");
    let repo = repo.to_str().unwrap();
    assert_eq!(code(&["plan", "--repo", repo, "--sha", &sha]), 0);
    assert_eq!(
        code(&["plan", "--repo", repo, "--sha", &sha, "--only", CORE]),
        0
    );
    // Controls for the refusals: the same command with a crate that is in the plan succeeds above.
    assert_eq!(
        code(&["plan", "--repo", repo, "--sha", &sha, "--only", "nope"]),
        2
    );
    assert_eq!(
        code(&["plan", "--repo", repo, "--sha", &sha, "--exclude", "nope"]),
        2
    );
    assert_eq!(
        code(&["plan", "--repo", repo, "--sha", "main"]),
        2,
        "a branch name is not a SHA"
    );
}

#[test]
fn verify_builds_the_extracted_packages() {
    let (dir, sha) = fixture("verify_builds_the_extracted_packages");
    let repo = dir.path().join("repo");
    assert_eq!(
        code(&["verify", "--repo", repo.to_str().unwrap(), "--sha", &sha]),
        0
    );
}

#[test]
fn a_dry_run_publish_writes_the_ledger_with_the_phase_timing_notes() {
    let (dir, sha) = fixture("a_dry_run_publish_writes_the_ledger_with_the_phase_timing_notes");
    let repo = dir.path().join("repo");
    let ledger = dir.path().join("ledger.jsonl");
    let st = code(&[
        "publish",
        "--repo",
        repo.to_str().unwrap(),
        "--sha",
        &sha,
        "--ledger",
        ledger.to_str().unwrap(),
    ]);
    assert_eq!(st, 0);
    let ev = events(&ledger);
    let kinds: Vec<EventKind> = ev.iter().map(|e| e.event).collect();
    for k in [EventKind::Planned, EventKind::Verified, EventKind::Finished] {
        assert!(kinds.contains(&k), "{k:?} in {kinds:?}");
    }
    assert!(!kinds.contains(&EventKind::UploadStarted), "dry run");
    let notes: Vec<&Event> = ev.iter().filter(|e| e.event == EventKind::Phase).collect();
    assert!(
        notes
            .iter()
            .any(|e| e.crate_name.is_none() && e.detail.starts_with("package took ")),
        "{notes:?}"
    );
    assert!(
        notes
            .iter()
            .any(|e| e.crate_name.is_none() && e.detail.starts_with("verify all took ")),
        "{notes:?}"
    );
    for krate in [CORE, APP] {
        assert!(
            notes.iter().any(|e| e.crate_name.as_deref() == Some(krate)
                && e.detail.starts_with("verify (cargo check) took ")),
            "{krate}: {notes:?}"
        );
    }
}

#[test]
fn publish_refusals_exit_2_and_write_no_ledger() {
    let (dir, sha) = fixture("publish_refusals_exit_2_and_write_no_ledger");
    let repo = dir.path().join("repo");
    let repo = repo.to_str().unwrap();
    let ledger = dir.path().join("ledger.jsonl");
    let ledger = ledger.to_str().unwrap();
    let base = [
        "publish",
        "--repo",
        repo,
        "--sha",
        sha.as_str(),
        "--ledger",
        ledger,
    ];
    let with = |extra: &[&str]| code(&[&base[..], extra].concat());
    assert_eq!(with(&["--no-verify-crate", CORE]), 2, "needs its --reason");
    assert_eq!(with(&["--exclude", CORE]), 2, "needs its --exclude-reason");
    assert_eq!(
        with(&["--exclude", "nope", "--exclude-reason", "r"]),
        2,
        "not in the plan"
    );
    assert_eq!(
        with(&["--only", CORE, "--exclude", APP, "--exclude-reason", "r"]),
        2,
        "--only conflicts with --exclude"
    );
    assert!(!Path::new(ledger).exists(), "a refusal leaves no ledger");
    // Control: the same base command with no extra argument runs.
    assert_eq!(with(&[]), 0);
    assert!(Path::new(ledger).exists());
}
