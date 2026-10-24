//! The `releaseant` command line. Dry-run is the default; uploading and yanking need `--execute`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};

use releaseant::cargo::Cargo;
use releaseant::clock::{SystemClock, now_unix};
use releaseant::crates_io::{CratesIo, INDEX_URL, Index};
use releaseant::externals::missing_externals;
use releaseant::ledger::Ledger;
use releaseant::pipeline::{
    CargoAddCheck, Exception, PipelineError, PublishArgs, package_all, prepare, publish, readback,
};
use releaseant::registry::{Lookup, Registry};
use releaseant::run::{Mode, Options, Outcome};
use releaseant::verify::verify_set;
use releaseant::yank::{CargoYank, YankOutcome, yank};

#[derive(Parser)]
#[command(
    version,
    about = "Ordered, verified, resumable crates.io publishing with read-back"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args, Clone)]
struct Source {
    /// The git repository to publish from.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Full 40-hex commit SHA to publish (a branch name is refused).
    #[arg(long)]
    sha: String,
}

#[derive(Subcommand)]
enum Command {
    /// Print the publish order for a commit.
    Plan {
        #[command(flatten)]
        source: Source,
        /// Also ask the index whether every dependency outside the set is satisfiable (needs network).
        #[arg(long)]
        check_externals: bool,
    },
    /// Package the commit and build the extracted tarballs; uploads nothing.
    Verify(Source),
    /// Verify, then publish (dry-run unless `--execute`). Running it again resumes.
    #[command(alias = "resume")]
    Publish {
        #[command(flatten)]
        source: Source,
        /// Actually upload. Without this nothing is uploaded.
        #[arg(long)]
        execute: bool,
        /// Ledger file (default: `<repo>/.releaseant/ledger.jsonl`).
        #[arg(long)]
        ledger: Option<PathBuf>,
        /// Longest total wait per run, in seconds (default 3600).
        #[arg(long)]
        max_wait: Option<u64>,
        /// Skip pre-upload verification of this crate (needs a matching `--reason`).
        #[arg(long = "no-verify-crate")]
        no_verify_crate: Vec<String>,
        /// Why the matching `--no-verify-crate` is skipped.
        #[arg(long)]
        reason: Vec<String>,
        /// Keep the scratch directory.
        #[arg(long)]
        keep: bool,
    },
    /// Ask the index about every crate in the plan.
    Readback(Source),
    /// Yank a version this tool published (dry-run unless `--execute`).
    Yank {
        /// Crate name.
        name: String,
        /// Version to yank.
        version: semver::Version,
        /// `kind:text`, kind one of security, broken-build, wrong-contents, licence, other.
        #[arg(long)]
        reason: String,
        /// Ledger file recording the publish.
        #[arg(long)]
        ledger: PathBuf,
        /// Actually yank.
        #[arg(long)]
        execute: bool,
    },
}

fn work_dir(run_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("releaseant-{run_id}"))
}

fn run_id(sha: &str) -> String {
    format!("{}-{}", &sha[..8.min(sha.len())], now_unix())
}

fn main() -> ExitCode {
    match real_main(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn real_main(cli: Cli) -> Result<bool, Box<dyn std::error::Error>> {
    let cargo = Cargo::from_env();
    match cli.command {
        Command::Plan {
            source: s,
            check_externals,
        } => {
            let work = work_dir(&run_id(&s.sha));
            let prepared = prepare(&cargo, &s.repo, &s.sha, &work);
            let _ = std::fs::remove_dir_all(&work);
            let prepared = prepared?;
            for c in &prepared.plan.order {
                println!("{} {}", c.name, c.version);
            }
            if !check_externals {
                return Ok(true);
            }
            let mut idx = IndexRegistry(Index::new(INDEX_URL));
            let missing = missing_externals(&prepared.meta, &prepared.plan, &mut idx)?;
            for m in &missing {
                println!("MISSING EXTERNAL: {m}");
            }
            if missing.is_empty() {
                println!("externals: every dependency outside the set is on the registry");
            }
            Ok(missing.is_empty())
        }
        Command::Verify(s) => {
            let work = work_dir(&run_id(&s.sha));
            let result = (|| -> Result<bool, PipelineError> {
                let prepared = prepare(&cargo, &s.repo, &s.sha, &work)?;
                let packaged = package_all(&cargo, &prepared, &work.join("target"))?;
                let results = verify_set(&cargo, &packaged, &work.join("verify"), &[])?;
                let mut ok = true;
                for (n, r) in results {
                    match r {
                        Ok(()) => println!("{n}: verified"),
                        Err(why) => {
                            ok = false;
                            println!("{n}: FAILED\n{why}");
                        }
                    }
                }
                Ok(ok)
            })();
            let _ = std::fs::remove_dir_all(&work);
            Ok(result?)
        }
        Command::Publish {
            source,
            execute,
            ledger,
            max_wait,
            no_verify_crate,
            reason,
            keep,
        } => {
            if no_verify_crate.len() != reason.len() {
                return Err("every --no-verify-crate needs its own --reason".into());
            }
            let exceptions: Vec<Exception> = no_verify_crate
                .into_iter()
                .zip(reason)
                .map(|(krate, reason)| Exception { krate, reason })
                .collect();
            let id = run_id(&source.sha);
            let work = work_dir(&id);
            let ledger_path =
                ledger.unwrap_or_else(|| source.repo.join(".releaseant/ledger.jsonl"));
            let mut opts = Options {
                mode: if execute { Mode::Execute } else { Mode::DryRun },
                ..Options::default()
            };
            if let Some(s) = max_wait {
                opts.max_wait = Duration::from_secs(s);
            }
            let summary = publish(
                &cargo,
                &PublishArgs {
                    repo: &source.repo,
                    sha: &source.sha,
                    ledger: &ledger_path,
                    work: &work,
                    run_id: &id,
                    opts,
                    exceptions,
                },
                |dir, target| {
                    Box::new(CratesIo::new(
                        Cargo::from_env(),
                        dir.to_path_buf(),
                        target.to_path_buf(),
                    ))
                },
                &mut SystemClock,
                &mut CargoAddCheck {
                    cargo: Cargo::from_env(),
                    scratch: work.join("consumer"),
                },
            );
            if !keep {
                let _ = std::fs::remove_dir_all(&work);
            }
            let summary = summary?;
            for (n, o) in &summary.report.outcomes {
                println!("{n}: {}", describe(o));
            }
            for (n, r) in &summary.consumer {
                match r {
                    Ok(()) => println!("{n}: consumer check passed"),
                    Err(why) => println!("{n}: consumer check FAILED\n{why}"),
                }
            }
            if let Some(why) = &summary.report.stopped {
                println!("run stopped (resumable): {why}");
            }
            if !execute {
                println!("dry run: nothing was uploaded (use --execute)");
            }
            println!("ledger: {}", ledger_path.display());
            Ok(summary.is_success() || (!execute && summary.report.stopped.is_none()))
        }
        Command::Readback(s) => {
            let work = work_dir(&run_id(&s.sha));
            let prepared = prepare(&cargo, &s.repo, &s.sha, &work);
            let _ = std::fs::remove_dir_all(&work);
            let prepared = prepared?;
            let mut idx = IndexRegistry(Index::new(INDEX_URL));
            let mut ok = true;
            for (n, l) in readback(&prepared, &mut idx) {
                let word = match l {
                    Lookup::Present { checksum } => format!("present ({checksum})"),
                    Lookup::Absent => {
                        ok = false;
                        "ABSENT".to_owned()
                    }
                    Lookup::Unknown(why) => {
                        ok = false;
                        format!("UNKNOWN: {why}")
                    }
                };
                println!("{n}: {word}");
            }
            Ok(ok)
        }
        Command::Yank {
            name,
            version,
            reason,
            ledger,
            execute,
        } => {
            let ledger = Ledger::open(ledger)?;
            let mut y = CargoYank {
                cargo,
                dir: Path::new(".").to_path_buf(),
            };
            match yank(
                &ledger,
                &mut SystemClock,
                &mut y,
                &name,
                &version,
                &reason,
                execute,
            )? {
                YankOutcome::WouldYank => {
                    println!("dry run: would yank {name}@{version} (use --execute)")
                }
                YankOutcome::Yanked => println!("yanked {name}@{version}"),
            }
            Ok(true)
        }
    }
}

fn describe(o: &Outcome) -> String {
    match o {
        Outcome::WouldUpload => "would upload".into(),
        Outcome::AlreadyPublished => "already published".into(),
        Outcome::Visible => "published and read back".into(),
        Outcome::ChecksumMismatch { uploaded, index } => {
            format!("CHECKSUM MISMATCH (uploaded {uploaded}, index {index})")
        }
        Outcome::Missing => "MISSING from the index after upload".into(),
        Outcome::Failed(why) => format!("FAILED: {why}"),
        Outcome::Skipped { blocked_by } => format!("skipped (blocked by {blocked_by})"),
        Outcome::Interrupted(why) => format!("INTERRUPTED: {why}"),
        Outcome::NotAttempted => "not attempted".into(),
    }
}

/// `readback` only asks questions, so it needs no cargo.
struct IndexRegistry(Index);

impl Registry for IndexRegistry {
    fn lookup(&mut self, name: &str, version: &semver::Version) -> Lookup {
        self.0.lookup(name, version)
    }

    fn versions(&mut self, name: &str) -> Result<Vec<(semver::Version, bool)>, String> {
        self.0.versions(name)
    }

    fn upload(&mut self, _: &str, _: &semver::Version) -> releaseant::registry::Upload {
        unreachable!("readback never uploads")
    }
}
