//! The README's hello-world is run, not trusted: every `$ ` line in the console blocks between
//! `<!-- hello:begin -->` and `<!-- hello:end -->` is executed with the real binary against the demo
//! workspace the README describes, and its output must be the output the README prints.
//!
//! Needs network (a publish dry run asks the crates.io index), like the other real-registry tests.

use std::fs;
use std::path::Path;
use std::process::Command;

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

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// The manifests the README shows, as (path, text): every ```toml block in the hello region whose
/// first line is `# <path>`. The README is the source of truth for the demo workspace, so the
/// sample it shows is the sample that is run.
fn readme_manifests(readme: &str) -> Vec<(String, String)> {
    let begin = readme
        .find("<!-- hello:begin -->")
        .expect("README has no hello:begin marker");
    let end = readme
        .find("<!-- hello:end -->")
        .expect("README has no hello:end marker");
    let mut files: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in readme[begin..end].lines() {
        let line = line.trim_end();
        if line == "```toml" {
            current = Some((String::new(), String::new()));
        } else if line.starts_with("```") {
            if let Some((path, text)) = current.take() {
                files.push((path, text));
            }
        } else if let Some((path, text)) = current.as_mut() {
            if path.is_empty() {
                *path = line
                    .strip_prefix("# ")
                    .unwrap_or_else(|| panic!("a toml block must start with `# <path>`: {line:?}"))
                    .to_owned();
            } else {
                text.push_str(line);
                text.push('\n');
            }
        }
    }
    files
}

/// The demo workspace from the README's manifests, each member with the empty `src/lib.rs` and the
/// `README.md` the README says it has, committed in a fresh repository.
fn demo_repo(dir: &Path, manifests: &[(String, String)]) {
    demo_repo_with(dir, manifests, true);
}

/// [`demo_repo`], optionally without the members' `README.md` files.
fn demo_repo_with(dir: &Path, manifests: &[(String, String)], readmes: bool) {
    for (path, text) in manifests {
        write(&dir.join(path), text);
        if let Some(member) = path.strip_suffix("/Cargo.toml") {
            write(&dir.join(member).join("src/lib.rs"), "");
            if readmes {
                write(&dir.join(member).join("README.md"), "A demo crate.\n");
            }
        }
    }
    git(dir, &["init", "-q"]);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "demo"]);
}

/// Console blocks between the markers, each as (command, expected output) pairs.
fn readme_steps(readme: &str) -> Vec<(String, String)> {
    let begin = readme
        .find("<!-- hello:begin -->")
        .expect("README has no hello:begin marker");
    let end = readme
        .find("<!-- hello:end -->")
        .expect("README has no hello:end marker");
    let region = &readme[begin..end];
    let mut steps: Vec<(String, String)> = Vec::new();
    let mut in_block = false;
    for line in region.lines() {
        let line = line.trim_end();
        if line.starts_with("```") {
            in_block = line.trim_start_matches('`') == "console";
            continue;
        }
        if !in_block {
            continue;
        }
        if let Some(cmd) = line.strip_prefix("$ ") {
            // `# ...` after a command is a comment for the reader, not part of the command.
            let cmd = cmd.split(" #").next().unwrap().trim();
            steps.push((cmd.to_owned(), String::new()));
        } else {
            let last = steps.last_mut().expect("output before any command");
            last.1.push_str(line);
            last.1.push('\n');
        }
    }
    steps
}

/// Line endings only: a path separator that differs by platform is a defect in the output, not noise
/// (the fresh-eyes trial found `.\.releaseant/ledger.jsonl` on Windows against the README's `./`).
fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[test]
fn the_readme_hello_world_does_what_the_readme_says() {
    let readme =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md")).unwrap();
    let steps = readme_steps(&readme);
    assert!(
        steps.len() >= 3,
        "the hello-world has too few steps: {steps:?}"
    );

    let manifests = readme_manifests(&readme);
    let paths: Vec<&str> = manifests.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        [
            "Cargo.toml",
            "releaseant-demo-core/Cargo.toml",
            "releaseant-demo-app/Cargo.toml"
        ],
        "the README must show the demo workspace's three manifests, in this order"
    );

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("work");
    fs::create_dir_all(&repo).unwrap();
    demo_repo(&repo, &manifests);

    let mut sha = String::new();
    let mut ran_releaseant = 0;
    for (cmd, expected) in &steps {
        if cmd == "SHA=$(git rev-parse HEAD)" {
            sha = git(&repo, &["rev-parse", "HEAD"]);
            assert!(expected.is_empty(), "that line prints nothing");
            continue;
        }
        let args = cmd
            .strip_prefix("releaseant ")
            .unwrap_or_else(|| panic!("README step is not one the test can run: {cmd:?}"));
        assert!(
            !sha.is_empty(),
            "README uses $SHA before setting it: {cmd:?}"
        );
        let out = Command::new(env!("CARGO_BIN_EXE_releaseant"))
            .current_dir(&repo)
            .args(args.replace("$SHA", &sha).split_whitespace())
            .output()
            .unwrap();
        let got = normalise(&format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
        // The SHA in an error message is the user's, not part of the contract.
        assert_eq!(
            got,
            normalise(expected),
            "README output is stale for: {cmd}"
        );
        ran_releaseant += 1;
    }
    assert!(
        ran_releaseant >= 2,
        "the README shows fewer than two releaseant commands"
    );
}

/// `releaseant plan --sha HEAD's sha <extra>` in a demo repo built from the README, without the
/// members' README files when `readmes` is false: (exit code, stdout, stderr).
fn plan_demo(readmes: bool, extra: &[&str]) -> (Option<i32>, String, String) {
    let readme =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md")).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("work");
    fs::create_dir_all(&repo).unwrap();
    demo_repo_with(&repo, &readme_manifests(&readme), readmes);
    let sha = git(&repo, &["rev-parse", "HEAD"]);
    let out = Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .current_dir(&repo)
        .args(["plan", "--sha", &sha])
        .args(extra)
        .output()
        .unwrap();
    (
        out.status.code(),
        normalise(&String::from_utf8_lossy(&out.stdout)),
        normalise(&String::from_utf8_lossy(&out.stderr)),
    )
}

// What the registry accepts without is a warning (PM, 2026-10-29): the demo with its README files gone
// still plans, exit 0, with a warning per crate on stderr and the plan alone on stdout. The control is
// the main test, the same sample with its README files.
#[test]
fn a_missing_readme_is_a_warning_that_does_not_stop_plan() {
    let (code, stdout, stderr) = plan_demo(false, &[]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "releaseant-demo-core 0.1.0\nreleaseant-demo-app 0.1.0\n"
    );
    for krate in ["releaseant-demo-core", "releaseant-demo-app"] {
        assert!(
            stderr.contains(&format!("warning: {krate}: readme:")),
            "a warning for {krate}: {stderr}"
        );
    }
}

// `--require-metadata` turns a chosen warning into an error, and only that one.
#[test]
fn require_metadata_turns_the_chosen_warning_into_an_error() {
    let (code, stdout, stderr) = plan_demo(false, &["--require-metadata", "readme"]);
    assert_eq!(code, Some(2), "{stderr}");
    assert!(stdout.is_empty(), "no plan for a set that cannot publish");
    assert!(stderr.contains("releaseant-demo-core: readme:"), "{stderr}");
    assert!(stderr.contains("--require-metadata"), "{stderr}");
    // Requiring something the sample has changes nothing.
    let (code, stdout, stderr) = plan_demo(true, &["--require-metadata", "keywords,rust-version"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "releaseant-demo-core 0.1.0\nreleaseant-demo-app 0.1.0\n"
    );
}

#[test]
fn an_unknown_required_field_is_refused_naming_the_ones_that_exist() {
    let (code, stdout, stderr) = plan_demo(true, &["--require-metadata", "licence"]);
    assert_eq!(code, Some(2), "{stderr}");
    assert!(stdout.is_empty());
    assert!(stderr.contains("\"licence\""), "{stderr}");
    assert!(
        stderr.contains("readme, keywords, rust-version"),
        "{stderr}"
    );
}

// The mutation that started ROADMAP R6, end to end through the real binary: the README's own sample with
// `description` deleted from one crate. `plan` (which used to pass) must fail, name the crate and the
// field, and say what to add; the control is the test above, the same sample untouched.
#[test]
fn deleting_a_description_from_the_readme_sample_makes_plan_fail_naming_it() {
    let readme =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md")).unwrap();
    let mut manifests = readme_manifests(&readme);
    let core = manifests
        .iter_mut()
        .find(|(p, _)| p == "releaseant-demo-core/Cargo.toml")
        .expect("the README shows the core manifest");
    let before = core.1.clone();
    core.1 = before
        .lines()
        .filter(|l| !l.starts_with("description"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(core.1, before, "the mutation removed a description line");

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("work");
    fs::create_dir_all(&repo).unwrap();
    demo_repo(&repo, &manifests);
    let sha = git(&repo, &["rev-parse", "HEAD"]);

    let out = Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .current_dir(&repo)
        .args(["plan", "--sha", &sha])
        .output()
        .unwrap();
    let stderr = normalise(&String::from_utf8_lossy(&out.stderr));
    assert_eq!(out.status.code(), Some(2), "{stderr}");
    assert!(
        out.stdout.is_empty(),
        "no plan is printed for a set that cannot publish"
    );
    assert!(
        stderr.contains("releaseant-demo-core: description:"),
        "names the crate and the field: {stderr}"
    );
    assert!(
        stderr.contains("description = "),
        "says what to add: {stderr}"
    );
    assert!(stderr.contains("nothing was uploaded"), "{stderr}");
    assert!(
        !stderr.contains("releaseant-demo-app"),
        "only the crate with the problem is named: {stderr}"
    );
}
