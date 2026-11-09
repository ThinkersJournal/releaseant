//! `publish --artifacts-only` and the refusal of an unwired mixed release, through the binary and the
//! library. No network and no upload: dry runs, and refusals that happen before anything is asked.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use releaseant::artifacts_cmd::refuse_unwired_mixed;

const HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

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

/// A committed one-crate repository; `repository` is the crate's declared repository, `toml` the
/// commit's `releaseant.toml` if any. Returns the guard, the repo path and the SHA.
fn fixture(who: &str, repository: Option<&str>, toml: Option<&str>) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let w = dir.path().join("repo");
    fs::create_dir_all(w.join("src")).unwrap();
    git(&w, &["init", "-q", "-b", "main"]);
    git(&w, &["config", "user.name", "t"]);
    git(&w, &["config", "user.email", "t@example.invalid"]);
    let repo_line = repository
        .map(|r| format!("repository = \"{r}\"\n"))
        .unwrap_or_default();
    fs::write(
        w.join("Cargo.toml"),
        format!(
            "[package]\nname = \"zzart-one\"\nversion = \"0.1.0\"\nedition = \"2021\"\ndescription = \"fixture\"\nlicense = \"MIT\"\n{repo_line}"
        ),
    )
    .unwrap();
    fs::write(w.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    if let Some(t) = toml {
        fs::write(w.join("releaseant.toml"), t).unwrap();
    }
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "-q", "-m", who]);
    let sha = git(&w, &["rev-parse", "HEAD"]);
    (dir, sha)
}

fn artifact_toml() -> String {
    format!(
        "[[artifact]]\nname = \"x.msi\"\nkind = \"file\"\npath = \"dist/x.msi\"\nsha256 = \"{HASH}\"\ntarget = \"github-release\"\n"
    )
}

fn publish(repo: &Path, sha: &str, extra: &[&str]) -> Output {
    // A regression that let a real run through must find no credentials (a mutation check of this
    // test once published a fixture crate to crates.io with the lane's inherited token).
    let home = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .env("CARGO_HOME", home.path())
        .env_remove("CARGO_REGISTRY_TOKEN")
        .env_remove("CARGO_REGISTRIES_CRATES_IO_TOKEN")
        .args(["publish", "--repo"])
        .arg(repo)
        .args(["--sha", sha])
        .args(extra)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn a_dry_run_names_the_release_and_the_repository_it_derived() {
    let (dir, sha) = fixture(
        "dry_derived",
        Some("https://github.com/o/n"),
        Some(&artifact_toml()),
    );
    let repo = dir.path().join("repo");
    let o = publish(
        &repo,
        &sha,
        &["--artifacts-only", "--artifacts-dir", "dist"],
    );
    let t = text(&o);
    assert_eq!(o.status.code(), Some(0), "{t}");
    assert!(t.contains("GitHub release v0.1.0 of o/n"), "{t}");
    assert!(t.contains("dry run: nothing was uploaded"), "{t}");
    assert!(
        !repo.join(".releaseant").exists(),
        "a dry run leaves no ledger"
    );
}

#[test]
fn a_missing_repository_refuses_and_an_explicit_one_fills_it() {
    let (dir, sha) = fixture("missing_repo", None, Some(&artifact_toml()));
    let repo = dir.path().join("repo");
    let args = ["--artifacts-only", "--artifacts-dir", "dist"];
    let o = publish(&repo, &sha, &args);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(2), "{t}");
    assert!(t.contains("declares `repository`"), "{t}");
    // The control: the same commit with the repository named.
    let mut with = args.to_vec();
    with.extend(["--artifacts-repo", "o/n"]);
    let o = publish(&repo, &sha, &with);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(0), "{t}");
    assert!(t.contains("of o/n"), "{t}");
}

#[test]
fn an_override_that_disagrees_with_the_crate_refuses() {
    let (dir, sha) = fixture(
        "override_disagrees",
        Some("https://github.com/o/n"),
        Some(&artifact_toml()),
    );
    let repo = dir.path().join("repo");
    let o = publish(
        &repo,
        &sha,
        &[
            "--artifacts-only",
            "--artifacts-dir",
            "dist",
            "--artifacts-repo",
            "o/other",
        ],
    );
    let t = text(&o);
    assert_eq!(o.status.code(), Some(2), "{t}");
    assert!(t.contains("disagrees"), "{t}");
}

#[test]
fn no_declared_artifacts_refuses_artifacts_only() {
    let (dir, sha) = fixture("none_declared", Some("https://github.com/o/n"), None);
    let o = publish(
        &dir.path().join("repo"),
        &sha,
        &["--artifacts-only", "--artifacts-dir", "dist"],
    );
    let t = text(&o);
    assert_eq!(o.status.code(), Some(2), "{t}");
    assert!(t.contains("declares no [[artifact]]"), "{t}");
}

#[test]
fn the_flags_exclude_each_other_and_need_their_directory() {
    let (dir, sha) = fixture(
        "flag_rules",
        Some("https://github.com/o/n"),
        Some(&artifact_toml()),
    );
    let repo = dir.path().join("repo");
    for extra in [
        &["--artifacts-only"][..],
        &["--artifacts-only", "--artifacts-dir", "d", "--crates-only"],
        &["--artifacts-only", "--artifacts-dir", "d", "--tag"],
        &[
            "--artifacts-only",
            "--artifacts-dir",
            "d",
            "--only",
            "zzart-one",
        ],
    ] {
        let o = publish(&repo, &sha, extra);
        assert_eq!(o.status.code(), Some(2), "{extra:?}: {}", text(&o));
    }
}

#[test]
fn a_real_mixed_publish_of_declared_artifacts_is_refused_before_anything_runs() {
    let (dir, sha) = fixture(
        "mixed_refused",
        Some("https://github.com/o/n"),
        Some(&artifact_toml()),
    );
    let repo = dir.path().join("repo");
    let o = publish(&repo, &sha, &["--execute", "--allow-not-tip"]);
    let t = text(&o);
    assert_eq!(o.status.code(), Some(2), "{t}");
    assert!(t.contains("--crates-only"), "{t}");
    assert!(!repo.join(".releaseant").exists(), "nothing ran: {t}");
}

#[test]
fn the_mixed_refusal_fires_only_for_a_commit_that_declares_artifacts() {
    let (dir, sha) = fixture("refusal_control", None, Some(&artifact_toml()));
    let repo = dir.path().join("repo");
    assert!(refuse_unwired_mixed(&repo, &sha).is_err());
    // Controls: no releaseant.toml at the commit, and one without artifacts.
    let (d2, s2) = fixture("refusal_no_toml", None, None);
    assert!(refuse_unwired_mixed(&d2.path().join("repo"), &s2).is_ok());
    let (d3, s3) = fixture("refusal_no_artifacts", None, Some("[names]\nowners = []\n"));
    assert!(refuse_unwired_mixed(&d3.path().join("repo"), &s3).is_ok());
}
