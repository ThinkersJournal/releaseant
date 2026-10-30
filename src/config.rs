//! `releaseant.toml`: settings that belong to a project, read from the root of the commit that is
//! being published (never from the working tree, like everything else here), so a requirement is
//! versioned with the release it governs.
//!
//! ```toml
//! [metadata]
//! require = ["keywords", "rust-version", "readme"]
//! ```
//!
//! `require` names the metadata fields that are warnings by default and must be errors for this
//! project (see [`crate::preflight`]). The command line's `--require-metadata` adds to it; neither can
//! relax what is an error anyway. An unknown key or an unknown field name is an error, not ignored: a
//! misspelt requirement that silently enforced nothing is worse than none.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::Deserialize;

use crate::preflight::parse_require;

/// The file's name, at the root of the repository.
pub const FILE: &str = "releaseant.toml";

/// A project's settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// `[metadata]`.
    #[serde(default)]
    pub metadata: MetadataConfig,
}

/// `[metadata]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataConfig {
    /// Fields to treat as errors when missing.
    #[serde(default)]
    pub require: Vec<String>,
}

impl Config {
    /// The metadata fields this project requires, validated by [`load`] and [`parse`].
    #[must_use]
    pub fn required(&self) -> BTreeSet<String> {
        self.metadata.require.iter().cloned().collect()
    }
}

/// Why the file could not be used.
#[derive(Debug)]
pub enum ConfigError {
    /// The file exists and could not be read.
    Read(io::Error),
    /// It is not the expected TOML (the parser's message names the line).
    Parse(String),
    /// `require` names a field that cannot be required.
    Require(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(e) => write!(f, "{FILE}: could not be read: {e}"),
            Self::Parse(m) => write!(f, "{FILE}: {m}"),
            Self::Require(m) => write!(f, "{FILE}: [metadata] require: {m}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Parse the text of a `releaseant.toml`.
///
/// # Errors
/// [`ConfigError::Parse`] for bad TOML, an unknown key or a wrong type; [`ConfigError::Require`] for a
/// field name that cannot be required.
pub fn parse(text: &str) -> Result<Config, ConfigError> {
    let config: Config =
        toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string().trim().to_owned()))?;
    parse_require(&config.metadata.require).map_err(ConfigError::Require)?;
    Ok(config)
}

/// The configuration at the root of `dir` (an archive of the commit): the default when there is no
/// file.
///
/// # Errors
/// See [`ConfigError`]; an unreadable file is an error, a missing one is not.
pub fn load(dir: &Path) -> Result<Config, ConfigError> {
    match fs::read_to_string(dir.join(FILE)) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(ConfigError::Read(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    // The control: no file, or an empty one, requires nothing.
    #[test]
    fn no_file_and_an_empty_file_require_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap(), Config::default());
        assert_eq!(parse("").unwrap().required(), set(&[]));
        assert_eq!(parse("[metadata]\n").unwrap().required(), set(&[]));
    }

    #[test]
    fn the_required_fields_are_read_from_the_file_in_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE),
            "[metadata]\nrequire = [\"keywords\", \"rust-version\", \"readme\"]\n",
        )
        .unwrap();
        assert_eq!(
            load(dir.path()).unwrap().required(),
            set(&["keywords", "readme", "rust-version"])
        );
    }

    #[test]
    fn an_unknown_key_is_an_error_naming_it_not_ignored() {
        let err = parse("[metadata]\nrequire = []\nrequired = [\"readme\"]\n").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)), "{err}");
        assert!(err.to_string().contains("required"), "{err}");
        assert!(err.to_string().starts_with("releaseant.toml:"), "{err}");
        let err = parse("[meta]\nrequire = []\n").unwrap_err();
        assert!(err.to_string().contains("meta"), "{err}");
    }

    #[test]
    fn an_unknown_field_name_is_an_error_naming_it_and_the_valid_ones() {
        let err = parse("[metadata]\nrequire = [\"readme\", \"licence\"]\n").unwrap_err();
        assert!(matches!(err, ConfigError::Require(_)), "{err}");
        let text = err.to_string();
        assert!(text.contains("licence"), "{text}");
        assert!(text.contains("readme, keywords, rust-version"), "{text}");
    }

    #[test]
    fn a_wrong_type_and_bad_toml_are_errors() {
        assert!(matches!(
            parse("[metadata]\nrequire = \"readme\"\n").unwrap_err(),
            ConfigError::Parse(_)
        ));
        assert!(matches!(
            parse("[metadata\nrequire = []\n").unwrap_err(),
            ConfigError::Parse(_)
        ));
    }
}
