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

/// The two crates the README's example talks about, committed in a fresh repository.
fn demo_repo(dir: &Path) {
    write(
        &dir.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"releaseant-demo-core\", \"releaseant-demo-app\"]\n",
    );
    let manifest = |name: &str, deps: &str| {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\ndescription = \"demo\"\nlicense = \"MIT\"\n{deps}"
        )
    };
    write(
        &dir.join("releaseant-demo-core/Cargo.toml"),
        &manifest("releaseant-demo-core", ""),
    );
    write(
        &dir.join("releaseant-demo-core/src/lib.rs"),
        "pub fn hi() {}\n",
    );
    write(
        &dir.join("releaseant-demo-app/Cargo.toml"),
        &manifest(
            "releaseant-demo-app",
            "\n[dependencies]\nreleaseant-demo-core = { path = \"../releaseant-demo-core\", version = \"0.1.0\" }\n",
        ),
    );
    write(
        &dir.join("releaseant-demo-app/src/lib.rs"),
        "pub fn run() { releaseant_demo_core::hi() }\n",
    );
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

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("work");
    fs::create_dir_all(&repo).unwrap();
    demo_repo(&repo);

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
