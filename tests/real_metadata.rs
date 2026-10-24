//! Parse what the real `cargo metadata` prints, so the serde shapes are checked against cargo itself.

use std::process::Command;

use releaseant::metadata::Metadata;
use releaseant::plan::plan;

#[test]
fn real_cargo_metadata_parses_and_plans_this_crate() {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta = Metadata::parse(&String::from_utf8(out.stdout).unwrap()).unwrap();
    let p = plan(&meta).unwrap();
    assert_eq!(
        p.order.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["releaseant"]
    );
}
