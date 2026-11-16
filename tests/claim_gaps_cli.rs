//! README claims that had a unit test but no test through the real binary: `readback`, and `publish --tag` with
//! `--tag-version` and with `--exclude`. A real repository with a bare repository as origin. These read the
//! crates.io index (like the README hello-world test) and upload nothing: dry runs and a read-only command,
//! and every command runs with an empty `CARGO_HOME` and no token variables.

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

fn releaseant(work: &Path, args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_releaseant"));
    c.env(
        "CARGO_HOME",
        std::env::temp_dir().join("releaseant-test-empty-cargo-home"),
    )
    .env_remove("CARGO_REGISTRY_TOKEN")
    .env_remove("CARGO_REGISTRIES_CRATES_IO_TOKEN")
    .current_dir(work)
    .args(args);
    c.output().unwrap()
}

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

const PACKAGE: &str = "description = \"A test crate\"\nedition = \"2021\"\nlicense = \"MIT OR Apache-2.0\"\n\
repository = \"https://github.com/example/releaseant-test\"\nrust-version = \"1.70\"\n\
keywords = [\"test\"]\ncategories = [\"development-tools\"]\n";

/// A repository with a bare origin that holds `crates` (name, version, dependency on the first crate):
/// each in a directory of its name with a `README.md` and an empty `src/lib.rs`, pushed to `main`.
/// Returns the working directory and the commit.
fn repo(tmp: &Path, crates: &[(&str, &str, bool)]) -> (std::path::PathBuf, String) {
    let (origin, work) = (tmp.join("origin.git"), tmp.join("work"));
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
    let members: Vec<String> = crates.iter().map(|(n, _, _)| format!("\"{n}\"")).collect();
    write(
        &work.join("Cargo.toml"),
        &format!(
            "[workspace]\nmembers = [{}]\nresolver = \"2\"\n",
            members.join(", ")
        ),
    );
    for (name, version, depends) in crates {
        let mut manifest =
            format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n{PACKAGE}");
        if *depends {
            let (first, first_version, _) = crates[0];
            manifest.push_str(&format!(
                "\n[dependencies]\n{first} = {{ path = \"../{first}\", version = \"{first_version}\" }}\n"
            ));
        }
        write(&work.join(name).join("Cargo.toml"), &manifest);
        write(&work.join(name).join("src/lib.rs"), "");
        write(&work.join(name).join("README.md"), "A test crate.\n");
    }
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-q", "-m", "one"]);
    let sha = git(&work, &["rev-parse", "HEAD"]);
    git(&work, &["push", "-q", "origin", "main"]);
    (work, sha)
}

#[test]
fn readback_says_present_for_a_version_on_the_index_and_absent_for_one_that_is_not() {
    // Control: `releaseant` 0.0.0 is on crates.io (the first version ever published), so it must read present.
    let tmp = tempfile::tempdir().unwrap();
    let (work, sha) = repo(tmp.path(), &[("releaseant", "0.0.0", false)]);
    let out = releaseant(&work, &["readback", "--sha", &sha]);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(0), "{t}");
    assert!(t.contains("releaseant: present ("), "{t}");

    // The same command on a version nobody published says ABSENT and exits non-zero.
    let tmp = tempfile::tempdir().unwrap();
    let (work, sha) = repo(tmp.path(), &[("releaseant", "0.0.0-claim-gap.1", false)]);
    let out = releaseant(&work, &["readback", "--sha", &sha]);
    let t = text(&out);
    assert_ne!(out.status.code(), Some(0), "{t}");
    assert!(t.contains("releaseant: ABSENT"), "{t}");
}

#[test]
fn tag_version_picks_the_tag_when_crates_differ_and_an_exclusion_means_no_project_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let (work, sha) = repo(
        tmp.path(),
        &[
            ("releaseant-claim-gap-core", "0.1.0", false),
            ("releaseant-claim-gap-app", "0.2.0", true),
        ],
    );
    let base = ["publish", "--sha", sha.as_str(), "--tag"];

    // Without --tag-version the tag is never a guess: it names the versions and the flag.
    let out = releaseant(&work, &base);
    let t = text(&out);
    assert_ne!(out.status.code(), Some(0), "{t}");
    assert!(
        t.contains("not tagged: the crates are at several versions (0.1.0, 0.2.0)"),
        "{t}"
    );
    assert!(t.contains("--tag-version"), "{t}");

    // With it, the dry run says which tag it would make, on this commit.
    let mut args = base.to_vec();
    args.extend(["--tag-version", "0.2.0"]);
    let out = releaseant(&work, &args);
    let t = text(&out);
    assert_eq!(out.status.code(), Some(0), "{t}");
    assert!(t.contains("v0.2.0"), "{t}");
    assert!(!t.contains("not tagged"), "{t}");

    // A run that leaves a crate out is not tagged, and says so. It has its own ledger: the same commit
    // with a different plan is refused in one.
    let other_ledger = tmp.path().join("excluded-ledger.jsonl");
    let mut args = base.to_vec();
    args.extend([
        "--ledger",
        other_ledger.to_str().unwrap(),
        "--exclude",
        "releaseant-claim-gap-app",
        "--exclude-reason",
        "test",
    ]);
    let out = releaseant(&work, &args);
    let t = text(&out);
    assert!(
        t.contains("not tagged: crates were excluded from this run"),
        "{t}"
    );
}
