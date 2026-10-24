//! The slice of `cargo metadata --no-deps --format-version 1` that publishing needs.

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
