//! Verify packaged crates before upload (SCOPE intro, DESIGN section 3).
//!
//! A manifest or source that only works from the repository tree (an `include_str!` of a file
//! outside the crate directory, say) packages fine and fails for every consumer. This stage
//! extracts each `.crate` and builds *that*, with the other crates of the set patched in from their
//! own extracted tarballs.
//!
//! Named hole (same as fuel's `verify-packaged-crates.py`): siblings are patched by path, so this
//! proves buildability from tarballs, not resolution against the registry in publish order.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;

use crate::cargo::Cargo;

/// One packaged crate.
#[derive(Debug, Clone)]
pub struct PackagedCrate {
    /// Package name.
    pub name: String,
    /// The `.crate` file `cargo package` produced.
    pub crate_file: PathBuf,
}

/// Extract a `.crate` into `dest`; returns the directory it unpacked (`name-version`).
///
/// # Errors
/// I/O errors, or a tarball that tries to leave `dest`.
pub fn extract_crate(file: &Path, dest: &Path) -> io::Result<PathBuf> {
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a .crate file name"))?;
    let gz = GzDecoder::new(fs::File::open(file)?);
    tar::Archive::new(gz).unpack(dest)?;
    let dir = dest.join(stem);
    if dir.is_dir() {
        Ok(dir)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} did not unpack to {stem}/", file.display()),
        ))
    }
}

/// Extract every crate into `scratch` and `cargo check` each one there.
///
/// Returns one `(name, result)` per crate not named in `skip`; `Err` holds cargo's output. A skipped
/// crate is still extracted (the others may depend on it) but is not built here: skipping is a
/// recorded exception made by the caller, never a default.
///
/// # Errors
/// I/O errors preparing the throwaway workspace.
pub fn verify_set(
    cargo: &Cargo,
    crates: &[PackagedCrate],
    scratch: &Path,
    skip: &[String],
) -> io::Result<Vec<(String, Result<(), String>)>> {
    verify_set_with(cargo, crates, scratch, skip, &|_, _, _| {})
}

/// [`verify_set`], calling `before(name, index, total)` (1-based, over the crates actually built)
/// before each crate's build starts.
///
/// # Errors
/// I/O errors preparing the throwaway workspace.
pub fn verify_set_with(
    cargo: &Cargo,
    crates: &[PackagedCrate],
    scratch: &Path,
    skip: &[String],
    before: &dyn Fn(&str, usize, usize),
) -> io::Result<Vec<(String, Result<(), String>)>> {
    fs::create_dir_all(scratch)?;
    let mut dirs = Vec::new();
    for c in crates {
        let dir = extract_crate(&c.crate_file, scratch)?;
        dirs.push((
            c.name.clone(),
            dir.file_name().unwrap().to_string_lossy().into_owned(),
        ));
    }
    let mut manifest = String::from("[workspace]\nresolver = \"2\"\nmembers = [\n");
    for (_, d) in &dirs {
        manifest.push_str(&format!("    \"{d}\",\n"));
    }
    manifest.push_str("]\n\n[patch.crates-io]\n");
    for (n, d) in &dirs {
        manifest.push_str(&format!("{n} = {{ path = \"{d}\" }}\n"));
    }
    fs::write(scratch.join("Cargo.toml"), manifest)?;

    let built: Vec<&(String, String)> = dirs.iter().filter(|(n, _)| !skip.contains(n)).collect();
    Ok(built
        .iter()
        .enumerate()
        .map(|(i, (n, _))| {
            before(n, i + 1, built.len());
            let r = cargo.check(scratch, n).map_err(|e| e.to_string());
            (n.clone(), r)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn manifest(name: &str) -> String {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.1\"\nedition = \"2021\"\ndescription = \"probe\"\nlicense = \"MIT\"\n"
        )
    }

    // T11: a crate that builds in the tree but not from its tarball is caught; the control passes.
    #[test]
    fn crate_that_only_builds_from_the_tree_fails_verification() {
        let id = std::process::id();
        let (bad, good) = (format!("zzt11bad{id}"), format!("zzt11good{id}"));
        let ws = tempfile::tempdir().unwrap();
        let w = ws.path();
        write(
            &w.join("Cargo.toml"),
            &format!("[workspace]\nresolver = \"2\"\nmembers = [\"{bad}\", \"{good}\"]\n"),
        );
        write(&w.join("outside.txt"), "lives outside the crate directory");
        write(&w.join(&bad).join("Cargo.toml"), &manifest(&bad));
        write(
            &w.join(&bad).join("src/lib.rs"),
            "pub const D: &str = include_str!(\"../../outside.txt\");\n",
        );
        write(&w.join(&good).join("Cargo.toml"), &manifest(&good));
        write(&w.join(&good).join("inside.txt"), "inside");
        write(
            &w.join(&good).join("src/lib.rs"),
            "pub const D: &str = include_str!(\"../inside.txt\");\n",
        );

        let cargo = Cargo::from_env();
        // Control: both build in the tree.
        cargo.check(w, &bad).unwrap();
        cargo.check(w, &good).unwrap();

        let target = tempfile::tempdir().unwrap();
        let version = "0.0.1".parse().unwrap();
        let crates: Vec<PackagedCrate> = [&bad, &good]
            .iter()
            .map(|n| PackagedCrate {
                name: (*n).clone(),
                crate_file: cargo.package(w, n, &version, target.path()).unwrap(),
            })
            .collect();

        let scratch = tempfile::tempdir().unwrap();
        let results = verify_set(&cargo, &crates, scratch.path(), &[]).unwrap();
        let get = |n: &str| &results.iter().find(|(x, _)| x == n).unwrap().1;
        assert!(
            get(&bad).is_err(),
            "the tree-only crate must fail from its tarball"
        );
        assert!(get(&good).is_ok(), "{:?}", get(&good));
    }
}
