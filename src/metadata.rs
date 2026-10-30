//! The slice of `cargo metadata --no-deps --format-version 1` that publishing needs.

use std::path::PathBuf;

use semver::Version;
use serde::Deserialize;

/// Parsed `cargo metadata` output.
#[derive(Debug, Clone, Deserialize)]
pub struct Metadata {
    /// Every workspace member.
    pub packages: Vec<Package>,
}

/// One workspace member.
#[derive(Debug, Clone, Deserialize)]
pub struct Package {
    /// Package name (the real name, not a dependency alias).
    pub name: String,
    /// Version as written in the manifest.
    pub version: Version,
    /// `None`: any registry. `Some([])`: `publish = false`. Otherwise an allowlist of registry names.
    #[serde(default)]
    pub publish: Option<Vec<String>>,
    /// Declared dependencies of every kind.
    pub dependencies: Vec<Dependency>,
    /// Absolute path of the package's `Cargo.toml`.
    #[serde(default)]
    pub manifest_path: PathBuf,
    /// `description`, resolved from the workspace when inherited.
    #[serde(default)]
    pub description: Option<String>,
    /// `license` (an SPDX expression).
    #[serde(default)]
    pub license: Option<String>,
    /// `license-file`, relative to the package directory.
    #[serde(default)]
    pub license_file: Option<String>,
    /// `repository`.
    #[serde(default)]
    pub repository: Option<String>,
    /// `homepage`.
    #[serde(default)]
    pub homepage: Option<String>,
    /// `documentation`.
    #[serde(default)]
    pub documentation: Option<String>,
    /// The readme path relative to the package directory: as written, or `README.md` when cargo found
    /// one next to the manifest; `None` when there is none or `readme = false`.
    #[serde(default)]
    pub readme: Option<String>,
    /// `keywords`.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// `categories`.
    #[serde(default)]
    pub categories: Vec<String>,
    /// `rust-version`.
    #[serde(default)]
    pub rust_version: Option<String>,
}

/// One declared dependency.
#[derive(Debug, Clone, Deserialize)]
pub struct Dependency {
    /// Name of the depended-on package.
    pub name: String,
    /// Version requirement; `*` exactly when the manifest wrote no version.
    pub req: String,
    /// `None` for a normal dependency, otherwise `"dev"` or `"build"`.
    pub kind: Option<String>,
    /// Whether the dependency is optional.
    #[serde(default)]
    pub optional: bool,
}

impl Dependency {
    /// A dev-dependency.
    #[must_use]
    pub fn is_dev(&self) -> bool {
        self.kind.as_deref() == Some("dev")
    }
}

impl Package {
    /// A package with complete publish metadata, for tests that are about something else.
    #[cfg(test)]
    pub(crate) fn test(name: &str, version: &str, dependencies: Vec<Dependency>) -> Self {
        Self {
            name: name.into(),
            version: version.parse().unwrap(),
            publish: None,
            dependencies,
            manifest_path: PathBuf::new(),
            description: Some("a test crate".into()),
            license: Some("MIT".into()),
            license_file: None,
            repository: Some("https://example.invalid/repo".into()),
            homepage: Some("https://example.invalid/".into()),
            documentation: Some("https://docs.example.invalid/".into()),
            readme: Some("README.md".into()),
            keywords: vec!["test".into()],
            categories: vec!["development-tools".into()],
            rust_version: Some("1.88".into()),
        }
    }

    /// Whether this package may go to crates.io: `publish = false` and an allowlist that omits
    /// `crates-io` both exclude it.
    #[must_use]
    pub fn is_publishable(&self) -> bool {
        match &self.publish {
            None => true,
            Some(registries) => registries.iter().any(|r| r == "crates-io"),
        }
    }
}

impl Metadata {
    /// Parse the JSON `cargo metadata` prints.
    ///
    /// # Errors
    /// Returns the JSON error if the text is not the expected shape.
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}
