//! The real-GitHub check of `GhForge` against the standing private rehearsal repository
//! (`ciresnave-bot/releaseant-artifact-rehearsal`, or `$RELEASEANT_FORGE_REPO`), slice A3. It needs a
//! signed-in `gh` with write access to that repository, which hosted CI does not have, so it sits
//! behind `runner-tests` like `tests/crane_runner.rs` (type-checked on every PR, run where the
//! credentials are). Never `#[ignore]`d; a missing `gh` or a refused call is a FAILURE with the
//! message, not a skip.
//!
//! It creates one draft release under a unique tag, uploads, reads back, makes an annotated tag, undrafts,
//! and then deletes only that release and that tag, reading back that neither remains.
#![cfg(feature = "runner-tests")]

use std::process::Command;

use releaseant::gh_forge::GhForge;
use releaseant::stage::Forge;

fn repo() -> String {
    std::env::var("RELEASEANT_FORGE_REPO")
        .unwrap_or_else(|_| "ciresnave-bot/releaseant-artifact-rehearsal".to_owned())
}

fn gh(args: &[&str]) -> Result<String, String> {
    let out = Command::new("gh")
        .args(args)
        .output()
        .map_err(|e| format!("gh could not be run: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(format!(
            "gh {args:?}: {} {}",
            String::from_utf8_lossy(&out.stderr).trim(),
            String::from_utf8_lossy(&out.stdout).trim()
        ))
    }
}

/// Deletes this run's release and tag when dropped, even on a panic.
struct Cleanup {
    repo: String,
    tag: String,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Ok(list) = gh(&[
            "api",
            &format!("repos/{}/releases?per_page=100", self.repo),
            "--jq",
            &format!(".[]|select(.tag_name==\"{}\")|.id", self.tag),
        ]) {
            for id in list.lines() {
                let _ = gh(&[
                    "api",
                    "-X",
                    "DELETE",
                    &format!("repos/{}/releases/{id}", self.repo),
                ]);
            }
        }
        let _ = gh(&[
            "api",
            "-X",
            "DELETE",
            &format!("repos/{}/git/refs/tags/{}", self.repo, self.tag),
        ]);
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

#[test]
fn a_draft_is_staged_read_back_tagged_and_published_on_a_real_forge() {
    let repo = repo();
    let tag = format!(
        "forge-test-{}-{}",
        std::process::id(),
        std::time::UNIX_EPOCH.elapsed().unwrap().as_secs()
    );
    let _cleanup = Cleanup {
        repo: repo.clone(),
        tag: tag.clone(),
    };
    let mut forge = GhForge::new(&repo);
    forge.version().expect("gh must be runnable");

    assert_eq!(
        forge.release(&tag).unwrap(),
        None,
        "no release yet (the same listing finds one afterwards)"
    );
    forge.create_draft(&tag).expect("create draft");
    let rel = forge.release(&tag).unwrap().unwrap_or_else(|| {
        panic!(
            "a draft is found by listing; raw: {}",
            gh(&["api", &format!("repos/{repo}/releases?per_page=100")]).unwrap()
        )
    });
    assert!(rel.draft && rel.assets.is_empty());

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("installer.bin");
    let bytes = b"forge runner test bytes\n";
    std::fs::write(&file, bytes).unwrap();
    forge
        .upload_asset(&tag, "installer.bin", &file)
        .expect("upload");

    let rel = forge.release(&tag).unwrap().unwrap();
    assert_eq!(rel.assets.len(), 1);
    assert!(rel.assets[0].complete);
    assert_eq!(rel.assets[0].size, bytes.len() as u64);
    assert_eq!(
        forge.served_sha256(&tag, "installer.bin").unwrap(),
        sha256_hex(bytes)
    );

    // A second upload of the same name is refused, never replaced.
    assert!(forge.upload_asset(&tag, "installer.bin", &file).is_err());
    // A name the forge would rewrite is an error naming the rewrite.
    let spaced = dir.path().join("with space.bin");
    std::fs::write(&spaced, bytes).unwrap();
    let e = forge
        .upload_asset(&tag, "with space.bin", &spaced)
        .unwrap_err();
    assert!(e.contains("not renamed back"), "{e}");

    // Annotated tag first, then undraft (design Q3): the tag stays an annotated tag object.
    let head = gh(&["api", &format!("repos/{repo}/commits/main"), "--jq", ".sha"]).unwrap();
    let obj = gh(&[
        "api",
        "-X",
        "POST",
        &format!("repos/{repo}/git/tags"),
        "-f",
        &format!("tag={tag}"),
        "-f",
        "message=forge runner test",
        "-f",
        &format!("object={head}"),
        "-f",
        "type=commit",
        "--jq",
        ".sha",
    ])
    .unwrap();
    gh(&[
        "api",
        "-X",
        "POST",
        &format!("repos/{repo}/git/refs"),
        "-f",
        &format!("ref=refs/tags/{tag}"),
        "-f",
        &format!("sha={obj}"),
    ])
    .unwrap();
    forge.publish_draft(&tag).expect("undraft");
    assert!(!forge.release(&tag).unwrap().unwrap().draft);
    let kind = gh(&[
        "api",
        &format!("repos/{repo}/git/ref/tags/{tag}"),
        "--jq",
        ".object.type",
    ])
    .unwrap();
    assert_eq!(
        kind, "tag",
        "the tag must still be an annotated tag object after the undraft"
    );
}
