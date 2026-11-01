//! The cargo process layer: run cargo, hand back its output and artifacts.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

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

    /// [`Self::run`], also noting when cargo's status lines appeared (see [`phase_line`]; the first
    /// `Compiling` too), as `(seconds since cargo started, line)`. For attributing wall time.
    ///
    /// # Errors
    /// If cargo cannot be started or its output cannot be read.
    pub fn run_marked(
        &self,
        dir: &Path,
        args: &[&str],
    ) -> io::Result<(Output, Vec<(f64, String)>)> {
        use std::io::{BufRead, Read};
        let started = Instant::now();
        let mut child = Command::new(&self.bin)
            .args(args)
            .env("CARGO_TERM_COLOR", "never")
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let drain = std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = stdout.read_to_end(&mut b);
            b
        });
        let mut stderr = io::BufReader::new(child.stderr.take().expect("stderr is piped"));
        let (mut err, mut raw, mut marks) = (Vec::new(), Vec::new(), Vec::new());
        let mut compiling = false;
        loop {
            raw.clear();
            if stderr.read_until(b'\n', &mut raw)? == 0 {
                break;
            }
            err.extend_from_slice(&raw);
            let line = String::from_utf8_lossy(&raw);
            let first_compile = !compiling && line.trim_start().starts_with("Compiling ");
            if first_compile {
                compiling = true;
            }
            if first_compile || phase_line(&line).is_some() {
                let mut text = line.trim().to_owned();
                text.truncate(text.char_indices().nth(120).map_or(text.len(), |(i, _)| i));
                marks.push((started.elapsed().as_secs_f64(), text));
            }
        }
        let status = child.wait()?;
        let out = drain.join().unwrap_or_default();
        let mut text = String::from_utf8_lossy(&err).into_owned();
        text.push_str(&String::from_utf8_lossy(&out));
        Ok((
            Output {
                success: status.success(),
                text,
            },
            marks,
        ))
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

    /// `cargo package -p a@1 -p b@1 ... --no-verify` for the whole set in one call.
    ///
    /// One call matters: measured with cargo 1.99.1, a crate whose versioned dependency is not yet
    /// published packages fine **together with** that dependency (cargo overlays the set it is
    /// packaging) but fails alone (see `tests/package_table.rs`). The returned paths are in the
    /// order of `crates`.
    ///
    /// # Errors
    /// [`CargoError::Failed`] with cargo's text, or [`CargoError::MissingOutput`].
    pub fn package_set(
        &self,
        dir: &Path,
        crates: &[(String, Version)],
        target_dir: &Path,
    ) -> Result<Vec<PathBuf>, CargoError> {
        let specs: Vec<String> = crates.iter().map(|(n, v)| format!("{n}@{v}")).collect();
        let target = target_dir.to_string_lossy().into_owned();
        let mut args = vec!["package", "--no-verify", "--target-dir", target.as_str()];
        for s in &specs {
            args.push("-p");
            args.push(s);
        }
        self.run_ok(dir, &args)?;
        crates
            .iter()
            .map(|(n, v)| {
                let file = target_dir.join("package").join(format!("{n}-{v}.crate"));
                if file.is_file() {
                    Ok(file)
                } else {
                    Err(CargoError::MissingOutput(file))
                }
            })
            .collect()
    }

    /// `cargo check -p name` in `dir`.
    ///
    /// # Errors
    /// [`CargoError::Failed`] with cargo's text.
    pub fn check(&self, dir: &Path, name: &str) -> Result<(), CargoError> {
        self.run_ok(dir, &["check", "-p", name]).map(|_| ())
    }
}

/// Cargo's status words that bound the phases of a `cargo publish` (`Verifying` .. `Finished` is
/// the verification build, `Uploading` .. `Uploaded` the upload, `Uploaded` .. `Published` the wait).
const PHASE_WORDS: [&str; 7] = [
    "Packaging",
    "Packaged",
    "Verifying",
    "Finished",
    "Uploading",
    "Uploaded",
    "Waiting",
];

/// The line when it starts with one of [`PHASE_WORDS`] (or `Published`), else `None`.
#[must_use]
pub fn phase_line(line: &str) -> Option<&str> {
    let t = line.trim();
    let word = t.split_whitespace().next()?;
    (PHASE_WORDS.contains(&word) || word == "Published").then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines_are_marked_and_other_lines_are_not() {
        for l in [
            "   Verifying fuel-conv v0.35.2 (C:/x)",
            "    Finished `dev` profile in 2.0s",
            "   Uploading fuel-conv v0.35.2 (C:/x)",
            "    Uploaded fuel-conv v0.35.2 to registry `crates-io`",
            "     Waiting for `fuel-conv` to be available at registry `crates-io`.",
            "   Published fuel-conv v0.35.2 at registry `crates-io`",
        ] {
            assert!(phase_line(l).is_some(), "{l}");
        }
        for l in [
            "   Compiling serde v1.0.0",
            "warning: unused",
            "",
            "Verifyingx a",
        ] {
            assert!(phase_line(l).is_none(), "{l:?}");
        }
    }

    // The control: the marked run returns the same success and text as the plain one.
    #[test]
    fn a_marked_run_returns_what_a_plain_run_returns() {
        let c = Cargo::from_env();
        let d = std::env::temp_dir();
        let (out, _marks) = c.run_marked(&d, &["--version"]).unwrap();
        let plain = c.run(&d, &["--version"]).unwrap();
        assert!(
            out.success && out.text.starts_with("cargo "),
            "{}",
            out.text
        );
        assert_eq!(out.text, plain.text);
        let (bad, _) = c.run_marked(&d, &["no-such-subcommand-zz"]).unwrap();
        assert!(!bad.success && !bad.text.is_empty());
    }

    // Positive control for the marks: a real `cargo package` run says Packaging and Packaged, in
    // that order, with non-decreasing offsets.
    #[test]
    fn a_real_cargo_run_yields_its_status_lines_with_offsets() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().to_string_lossy().into_owned();
        let args = [
            "package",
            "--no-verify",
            "--allow-dirty",
            "--offline",
            "--target-dir",
            target.as_str(),
        ];
        let (out, marks) = Cargo::from_env()
            .run_marked(Path::new(env!("CARGO_MANIFEST_DIR")), &args)
            .unwrap();
        assert!(out.success, "{}", out.text);
        let words: Vec<&str> = marks
            .iter()
            .filter_map(|(_, l)| l.split_whitespace().next())
            .collect();
        assert_eq!(words, ["Packaging", "Packaged"], "{marks:?}");
        assert!(marks[0].0 <= marks[1].0);
    }
}
