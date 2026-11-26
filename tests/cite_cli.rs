//! `releaseant cite-update`: after a MINOR (or pre-1.0 breaking) bump has been published and read back,
//! find every place that still cites the old `X.Y` of the project's own crates and say how to change it.
//! The fixture is the 0.27.2 -> 0.28.0 file list of vulkane (`.lane-state/stamp-hand-updates.md`, three
//! hand updates in one repo): README x4, `lib.rs`, `safe/naga.rs`, `safe/shaderc.rs`,
//! `vulkane_derive/README.md`, and the crate requirements in three manifests. The historical CHANGELOG
//! cites 0.27 too and must stay as it is. No network, no registry, no cargo.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap()
}

const LIB_MANIFEST: &str = "[package]\nname = \"vulkane\"\nversion = \"0.28.0\"\n";

/// The vulkane-shaped tree, already at version 0.28.0 (the stamp has run), citations still at 0.27.
fn vulkane_tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    write(
        r,
        "Cargo.toml",
        "[workspace]\nmembers = [\"vulkane\", \"vulkane-glsl-emit\", \"vulkane-wgsl-emit\"]\n",
    );
    write(
        r,
        "vulkane/Cargo.toml",
        &format!(
            "{LIB_MANIFEST}\n[dependencies]\nvulkane-glsl-emit = {{ path = \"../vulkane-glsl-emit\", version = \"0.27\" }}\nserde = \"1.0\"\ntree-sitter = \"0.27\"\n"
        ),
    );
    write(
        r,
        "vulkane-glsl-emit/Cargo.toml",
        "[package]\nname = \"vulkane-glsl-emit\"\nversion = \"0.28.0\"\n\n[dependencies.vulkane-wgsl-emit]\npath = \"../vulkane-wgsl-emit\"\nversion = \"0.27\"\n",
    );
    write(
        r,
        "vulkane-wgsl-emit/Cargo.toml",
        "[package]\nname = \"vulkane-wgsl-emit\"\nversion = \"0.28.0\"\n\n[dev-dependencies]\nvulkane = { path = \"../vulkane\", version = \"^0.27\" }\n",
    );
    // CRLF on purpose: the worktree files here are CRLF.
    write(
        r,
        "README.md",
        "# vulkane\r\n\r\n```toml\r\nvulkane = { version = \"0.27\", features = [\"fetch-spec\"] }\r\n```\r\n\r\n```toml\r\nvulkane = { version = \"0.27\", features = [\"fetch-spec\", \"derive\"] }\r\n```\r\n\r\n```toml\r\nvulkane = { version = \"0.27\", features = [\"naga\"] }\r\n```\r\n\r\n```toml\r\nvulkane = { version = \"0.27\", features = [\"shaderc\"] }\r\n```\r\n\r\nUses tree-sitter = \"0.27\" (not ours).\r\n",
    );
    write(
        r,
        "vulkane/src/lib.rs",
        "//! # vulkane\n//!\n//! ```toml\n//! vulkane = { version = \"0.27\", features = [\"fetch-spec\"] }\n//! ```\n\npub fn f() {\n    let s = \"vulkane = { version = \\\"0.27\\\" }\";\n    let _ = s;\n}\n",
    );
    write(
        r,
        "vulkane/src/safe/naga.rs",
        "//! ```toml\n//! vulkane = { version = \"0.27\", features = [\"naga\"] }\n//! ```\n",
    );
    write(
        r,
        "vulkane/src/safe/shaderc.rs",
        "//! ```toml\n//! vulkane = { version = \"0.27\", features = [\"shaderc\"] }\n//! ```\n",
    );
    write(
        r,
        "vulkane_derive/README.md",
        "vulkane = { version = \"0.27\", features = [\"derive\"] }\n",
    );
    write(
        r,
        "CHANGELOG.md",
        "## [0.28.0] - BREAKING\n\nMoves to 0.28.\n\n## [0.27.2]\n\nREADME said `vulkane = { version = \"0.27\" }` then.\n\n> vulkane = { version = \"0.27\" }\n",
    );
    write(
        r,
        "changelog.d/x.md",
        "kind: x\n---\nvulkane = { version = \"0.27\" }\n",
    );
    write(
        r,
        "docs/design/old.md",
        "vulkane = { version = \"0.27\" }\n",
    );
    d
}

fn cite(root: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .args(["cite-update", "--root"])
        .arg(root)
        .args(["--old", "0.27", "--new", "0.28", "--served", "0.27.2"])
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

const CHANGED: [&str; 9] = [
    "README.md",
    "vulkane/Cargo.toml",
    "vulkane-glsl-emit/Cargo.toml",
    "vulkane-wgsl-emit/Cargo.toml",
    "vulkane/src/lib.rs",
    "vulkane/src/safe/naga.rs",
    "vulkane/src/safe/shaderc.rs",
    "vulkane_derive/README.md",
    "README.md",
];

#[test]
fn dry_run_prints_the_diff_and_changes_nothing() {
    let d = vulkane_tree();
    let before = read(d.path(), "README.md");
    let o = cite(d.path(), &[]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("served 0.27.2"), "{t}");
    for f in CHANGED {
        assert!(
            t.contains(&format!("+++ b/{f}")),
            "{f} missing from the diff:\n{t}"
        );
    }
    assert_eq!(
        read(d.path(), "README.md"),
        before,
        "dry run must not write"
    );
}

#[test]
fn apply_rewrites_every_citation_and_only_those() {
    let d = vulkane_tree();
    let o = cite(d.path(), &["--apply"]);
    assert!(o.status.success(), "{}", text(&o));
    let readme = read(d.path(), "README.md");
    assert_eq!(readme.matches("version = \"0.28\"").count(), 4, "{readme}");
    assert!(
        readme.contains("tree-sitter = \"0.27\""),
        "not our crate: {readme}"
    );
    assert!(
        readme.contains("\r\n") && !readme.replace("\r\n", "").contains('\r'),
        "CRLF kept"
    );
    assert!(read(d.path(), "vulkane/src/lib.rs").contains("//! vulkane = { version = \"0.28\""));
    assert!(
        read(d.path(), "vulkane/src/lib.rs").contains("\\\"0.27\\\""),
        "code string literal untouched"
    );
    for m in [
        "vulkane/src/safe/naga.rs",
        "vulkane/src/safe/shaderc.rs",
        "vulkane_derive/README.md",
    ] {
        assert!(read(d.path(), m).contains("\"0.28\""), "{m}");
    }
    let m = read(d.path(), "vulkane/Cargo.toml");
    assert!(
        m.contains("version = \"0.28\" }") && m.contains("tree-sitter = \"0.27\""),
        "{m}"
    );
    assert!(read(d.path(), "vulkane-glsl-emit/Cargo.toml").contains("version = \"0.28\""));
    assert!(read(d.path(), "vulkane-wgsl-emit/Cargo.toml").contains("version = \"^0.28\""));
}

#[test]
fn historical_text_is_refused_and_left_alone() {
    let d = vulkane_tree();
    let ch = read(d.path(), "CHANGELOG.md");
    let o = cite(d.path(), &["--apply"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert_eq!(read(d.path(), "CHANGELOG.md"), ch, "CHANGELOG is history");
    assert_eq!(
        read(d.path(), "changelog.d/x.md").matches("0.27").count(),
        1
    );
    assert_eq!(
        read(d.path(), "docs/design/old.md").matches("0.27").count(),
        1
    );
    assert!(
        t.contains("refused") && t.contains("CHANGELOG.md"),
        "refusals are listed:\n{t}"
    );
}

#[test]
fn a_quoted_line_in_a_readme_is_refused() {
    let d = vulkane_tree();
    write(
        d.path(),
        "NOTES.md",
        "> vulkane = { version = \"0.27\" }\nIt was `vulkane = \"0.27\"` before.\n",
    );
    let o = cite(d.path(), &["--apply"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(read(d.path(), "NOTES.md").matches("0.27").count(), 2);
    assert!(text(&o).contains("NOTES.md"));
}

#[test]
fn a_served_version_that_is_not_the_old_one_aborts_with_exit_3() {
    let d = vulkane_tree();
    let o = Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .args(["cite-update", "--root"])
        .arg(d.path())
        .args([
            "--old", "0.27", "--new", "0.28", "--served", "0.26.4", "--apply",
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(text(&o).contains("0.26.4"));
    assert!(
        read(d.path(), "README.md").contains("0.27"),
        "nothing written on abort"
    );
}

#[test]
fn a_tree_whose_own_version_is_not_the_new_one_aborts_with_exit_3() {
    let d = vulkane_tree();
    let o = Command::new(env!("CARGO_BIN_EXE_releaseant"))
        .args(["cite-update", "--root"])
        .arg(d.path())
        .args(["--old", "0.27", "--new", "0.29", "--served", "0.27.2"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
}

#[test]
fn the_patch_goes_to_a_file_and_applies_with_git() {
    let d = vulkane_tree();
    let g = |a: &[&str]| {
        let o = Command::new("git")
            .arg("-C")
            .arg(d.path())
            .args(["-c", "core.autocrlf=false"])
            .args(a)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{a:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    };
    g(&["init", "-q"]);
    g(&["add", "-A"]);
    let patch = d.path().join("..").join(format!(
        "cite-{}.patch",
        d.path().file_name().unwrap().to_string_lossy()
    ));
    let o = cite(d.path(), &["--patch-out", patch.to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    g(&["apply", "--check", patch.to_str().unwrap()]);
    g(&["apply", patch.to_str().unwrap()]);
    assert!(read(d.path(), "README.md").contains("version = \"0.28\""));
    let _ = fs::remove_file(&patch);
}
