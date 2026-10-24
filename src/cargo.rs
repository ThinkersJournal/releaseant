//! The cargo process layer: run cargo, hand back its output and artifacts.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;

use crate::metadata::Metadata;

/// What a cargo invocation printed.
#[derive(Debug, Clone)]
pub struct Output {
    /// Exit status was success.
    pub success: bool,
    /// stderr followed by stdout (cargo writes its progress to stderr).
    pub text: String,
}

/// Why a cargo step failed.
#[derive(Debug)]
pub enum CargoError {
    /// cargo could not be started or a file operation failed.
    Io(io::Error),
    /// cargo ran and failed.
    Failed {
        /// The arguments, for the report.
        command: String,
        /// Combined output.
        text: String,
    },
    /// `cargo metadata` printed something unexpected.
    Json(serde_json::Error),
    /// cargo said it succeeded but the expected file is not there.
    MissingOutput(PathBuf),
}

impl fmt::Display for CargoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O: {e}"),
            Self::Failed { command, text } => write!(f, "`cargo {command}` failed:\n{text}"),
            Self::Json(e) => write!(f, "unexpected cargo metadata output: {e}"),
            Self::MissingOutput(p) => {
                write!(f, "cargo reported success but {} is missing", p.display())
            }
        }
    }
}

impl std::error::Error for CargoError {}

impl From<io::Error> for CargoError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// A cargo executable.
#[derive(Debug, Clone)]
pub struct Cargo {
    bin: OsString,
}

impl Cargo {
    /// The cargo named by `$CARGO` (set when running under cargo), else `cargo` on `PATH`.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            bin: std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()),
        }
    }

    /// Run `cargo <args>` in `dir`.
    ///
    /// # Errors
    /// If cargo cannot be started.
    pub fn run(&self, dir: &Path, args: &[&str]) -> io::Result<Output> {
        // Colour codes would break the line-anchored patterns in `classify` (CI sets
        // CARGO_TERM_COLOR=always), and the environment overrides any cargo config file.
        let out = Command::new(&self.bin)
            .args(args)
            .env("CARGO_TERM_COLOR", "never")
            .current_dir(dir)
            .output()?;
        let mut text = String::from_utf8_lossy(&out.stderr).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stdout));
        Ok(Output {
            success: out.status.success(),
            text,
        })
    }

    fn run_ok(&self, dir: &Path, args: &[&str]) -> Result<Output, CargoError> {
        let out = self.run(dir, args)?;
        if out.success {
            Ok(out)
        } else {
            Err(CargoError::Failed {
                command: args.join(" "),
                text: out.text,
            })
        }
    }

    /// `cargo metadata --no-deps` for the workspace at `dir`.
    ///
    /// # Errors
    /// See [`CargoError`].
    pub fn metadata(&self, dir: &Path) -> Result<Metadata, CargoError> {
        let out = self.run(dir, &["metadata", "--no-deps", "--format-version", "1"])?;
        if !out.success {
            return Err(CargoError::Failed {
                command: "metadata --no-deps --format-version 1".into(),
                text: out.text,
            });
        }
        // stdout only is JSON; stderr text was put first, so find the JSON object start.
        let start = out.text.find("{\"packages\"").unwrap_or(0);
        Metadata::parse(&out.text[start..]).map_err(CargoError::Json)
    }

    /// `cargo package -p name@version --no-verify` into `target_dir`; returns the `.crate` path.
    ///
    /// Verification is a separate stage ([`crate::verify`]) that builds the extracted tarball.
    ///
    /// # Errors
    /// [`CargoError::Failed`] carries cargo's text, which [`crate::classify`] can read.
    pub fn package(
        &self,
        dir: &Path,
        name: &str,
        version: &Version,
        target_dir: &Path,
    ) -> Result<PathBuf, CargoError> {
        let spec = format!("{name}@{version}");
        let target = target_dir.to_string_lossy();
        self.run_ok(
            dir,
            &[
                "package",
                "-p",
                &spec,
                "--no-verify",
                "--target-dir",
                &target,
            ],
        )?;
        let file = target_dir
            .join("package")
            .join(format!("{name}-{version}.crate"));
        if file.is_file() {
            Ok(file)
        } else {
            Err(CargoError::MissingOutput(file))
        }
    }

    /// `cargo check -p name` in `dir`.
    ///
    /// # Errors
    /// [`CargoError::Failed`] with cargo's text.
    pub fn check(&self, dir: &Path, name: &str) -> Result<(), CargoError> {
        self.run_ok(dir, &["check", "-p", name]).map(|_| ())
    }
}
