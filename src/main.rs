//! The `releaseant` command line: a wrapper around [`releaseant::cli::run`], which holds all of it.

use std::process::ExitCode;

fn main() -> ExitCode {
    releaseant::cli::run(std::env::args_os().collect())
}
