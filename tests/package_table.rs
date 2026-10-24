//! T2: the table in DESIGN section 0, row by row, with real `cargo package`.
//!
//! Needs network access to the crates.io index (cargo looks the missing crate up there). The crate
//! names are never published; a collision would make a "fails" row pass, which the control row
//! (`dep` itself packaging fine) cannot detect, so the prefix includes the process id.

use std::fs;
use std::path::Path;

use releaseant::cargo::{Cargo, CargoError};
use releaseant::classify::{Class, classify};
use releaseant::metadata::Metadata;
use releaseant::plan::plan;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn member(ws: &Path, name: &str, extra: &str) {
    write(
        &ws.join(name).join("Cargo.toml"),
        &format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.1\"\nedition = \"2021\"\ndescription = \"probe\"\nlicense = \"MIT\"\n{extra}\n"
        ),
    );
    write(&ws.join(name).join("src/lib.rs"), "");
}

#[test]
fn dev_and_optional_dependency_rows_match_the_measured_table() {
    let p = format!("zzrlt2x{}", std::process::id());
    let dep = format!("{p}-dep");
    let ws = tempfile::tempdir().unwrap();
    let w = ws.path();
    write(
        &w.join("Cargo.toml"),
        &format!("[workspace]\nresolver = \"2\"\nmembers = [\"{p}-*\"]\n"),
    );
    member(w, &dep, "");
    member(
        w,
        &format!("{p}-pathdev"),
        &format!("[dev-dependencies]\n{dep} = {{ path = \"../{dep}\" }}"),
    );
    member(
        w,
        &format!("{p}-verdev"),
        &format!("[dev-dependencies]\n{dep} = {{ path = \"../{dep}\", version = \"0.0.1\" }}"),
    );
    member(
        w,
        &format!("{p}-opt"),
        &format!(
            "[dependencies]\n{dep} = {{ path = \"../{dep}\", version = \"0.0.1\", optional = true }}"
        ),
    );
    member(
        w,
        &format!("{p}-norm"),
        &format!("[dependencies]\n{dep} = {{ path = \"../{dep}\", version = \"0.0.1\" }}"),
    );

    let cargo = Cargo::from_env();
    let version = "0.0.1".parse().unwrap();
    let target = tempfile::tempdir().unwrap();
    let try_package = |n: &str| cargo.package(w, n, &version, target.path());

    // Rows that package: the crate itself, and the path-only dev-dependency.
    try_package(&dep).expect("control: the dependency itself packages");
    try_package(&format!("{p}-pathdev")).expect("a path-only dev-dependency is stripped");

    // Rows that fail with the missing-dependency class.
    for row in ["verdev", "opt", "norm"] {
        match try_package(&format!("{p}-{row}")) {
            Err(CargoError::Failed { text, .. }) => assert!(
                matches!(classify(false, &text), Class::DependencyMissing { ref package } if *package == dep),
                "{row}: {text}"
            ),
            other => panic!("{row} should fail, got {other:?}"),
        }
    }

    // The plan's cheap edge rule agrees with cargo on every row.
    let meta: Metadata = cargo.metadata(w).unwrap();
    let order = plan(&meta).unwrap();
    let pos = |n: &str| order.order.iter().position(|c| c.name == n).unwrap();
    for row in ["verdev", "opt", "norm"] {
        assert!(
            pos(&dep) < pos(&format!("{p}-{row}")),
            "{row} needs {dep} first"
        );
    }
    let pathdev = order
        .order
        .iter()
        .find(|c| c.name == format!("{p}-pathdev"))
        .unwrap();
    assert!(
        pathdev.depends_on.is_empty(),
        "path-only dev-dep is not an edge"
    );
}
