//! The `releaseant` command line. Dry-run is the default; uploading and yanking need `--execute`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};

use releaseant::cargo::Cargo;
use releaseant::clock::{SystemClock, now_unix};
use releaseant::crates_io::{CratesIo, INDEX_URL, Index};
use releaseant::externals::missing_externals;
use releaseant::ledger::{Event, EventKind, Ledger};
use releaseant::pipeline::{
    CargoAddCheck, Exception, PipelineError, PublishArgs, Summary, check_metadata, package_all,
    prepare, publish, readback,
};
use releaseant::preflight::{Problem, parse_require};
use releaseant::registry::{Clock, Lookup, Registry};
use releaseant::run::{Mode, Options, Outcome};
use releaseant::tag::{TagOutcome, TagRequest, release_version, tag_release};
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

#[derive(clap::Args, Clone)]
struct MetadataPolicy {
    /// Treat these missing quality fields as errors instead of warnings (comma-separated; one or more
    /// of readme, keywords, rust-version, categories, repository). A missing description or licence,
    /// or a value crates.io refuses, is always an error.
    #[arg(long = "require-metadata", value_delimiter = ',')]
    require_metadata: Vec<String>,
}

impl MetadataPolicy {
    fn set(&self) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
        parse_require(&self.require_metadata).map_err(Into::into)
    }
}

/// Metadata problems that did not stop the run, on stderr so a plan on stdout stays clean.
fn print_warnings(warnings: &[Problem]) {
    for w in warnings {
        eprintln!("warning: {w}");
    }
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
        /// Show the plan without this crate, as `publish --exclude` would run it.
        #[arg(long = "exclude")]
        exclude: Vec<String>,
        #[command(flatten)]
        policy: MetadataPolicy,
    },
    /// Package the commit and build the extracted tarballs; uploads nothing.
    Verify {
        #[command(flatten)]
        source: Source,
        #[command(flatten)]
        policy: MetadataPolicy,
    },
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
        /// Leave this crate out of the run (needs a matching `--exclude-reason`). Crates that need it
        /// still need it on the registry.
        #[arg(long = "exclude")]
        exclude: Vec<String>,
        /// Why the matching `--exclude` is left out; recorded in the ledger.
        #[arg(long = "exclude-reason")]
        exclude_reason: Vec<String>,
        #[command(flatten)]
        policy: MetadataPolicy,
        /// Keep the scratch directory.
        #[arg(long)]
        keep: bool,
        /// When the run has published everything, tag the commit `v<version>` on origin and read it
        /// back (with `--execute`; without it, check that the tag could be made).
        #[arg(long)]
        tag: bool,
        /// The version to tag, when the crates are at several versions.
        #[arg(long)]
        tag_version: Option<semver::Version>,
        /// The project name in the tag message (default: the repository directory's name).
        #[arg(long)]
        tag_name: Option<String>,
    },
    /// Tag a published release: an annotated `v<version>` on the commit, pushed to origin and read
    /// back (dry-run unless `--execute`).
    Tag {
        #[command(flatten)]
        source: Source,
        /// The released version; the tag is `v<version>`.
        #[arg(long)]
        version: semver::Version,
        /// The project name in the tag message (default: the repository directory's name).
        #[arg(long)]
        name: Option<String>,
        /// Actually create and push the tag. Without this nothing is written.
        #[arg(long)]
        execute: bool,
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

/// Names a run and its scratch directory. The process id is part of it: two runs on the same commit in
/// the same second (a CI matrix, parallel tests) would otherwise share one scratch directory, and the
/// first to finish would delete the other's archive.
fn run_id(sha: &str) -> String {
    format!(
        "{}-{}-{}",
        &sha[..8.min(sha.len())],
        now_unix(),
        std::process::id()
    )
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
            exclude,
            policy,
        } => {
            let require = policy.set()?;
            let work = work_dir(&run_id(&s.sha));
            let prepared = prepare(&cargo, &s.repo, &s.sha, &work);
            // The metadata check reads files in the archive, so it runs before the scratch directory goes.
            let prepared = prepared
                .map_err(Box::<dyn std::error::Error>::from)
                .and_then(|mut p| {
                    if let Some(unknown) = exclude
                        .iter()
                        .find(|n| !p.plan.order.iter().any(|c| &c.name == *n))
                    {
                        return Err(format!("--exclude {unknown}: not in the publish plan").into());
                    }
                    p.plan = p.plan.without(&exclude.into_iter().collect());
                    let warnings = check_metadata(&p, &require)?;
                    Ok((p, warnings))
                });
            let _ = std::fs::remove_dir_all(&work);
            let (prepared, warnings) = prepared?;
            print_warnings(&warnings);
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
        Command::Verify { source: s, policy } => {
            let require = policy.set()?;
            let work = work_dir(&run_id(&s.sha));
            let result = (|| -> Result<bool, PipelineError> {
                let prepared = prepare(&cargo, &s.repo, &s.sha, &work)?;
                print_warnings(&check_metadata(&prepared, &require)?);
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
            exclude,
            exclude_reason,
            policy,
            keep,
            tag,
            tag_version,
            tag_name,
        } => {
            let require_metadata = policy.set()?;
            if no_verify_crate.len() != reason.len() {
                return Err("every --no-verify-crate needs its own --reason".into());
            }
            if exclude.len() != exclude_reason.len() {
                return Err("every --exclude needs its own --exclude-reason".into());
            }
            let excluded: Vec<Exception> = exclude
                .into_iter()
                .zip(exclude_reason)
                .map(|(krate, reason)| Exception { krate, reason })
                .collect();
            let any_excluded = !excluded.is_empty();
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
                    excluded,
                    require_metadata,
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
            print_warnings(&summary.warnings);
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
            // Forward slashes work on every platform and keep the output the same everywhere
            // (the default path would print as `.\.releaseant/ledger.jsonl` on Windows).
            println!(
                "ledger: {}",
                ledger_path.display().to_string().replace('\\', "/")
            );
            let published = summary.is_success() || (!execute && summary.report.stopped.is_none());
            let tagged = !tag
                || tag_after_publish(
                    &TagAfter {
                        source: &source,
                        execute,
                        any_excluded,
                        version: tag_version.as_ref(),
                        name: tag_name,
                        ledger: &ledger_path,
                        run_id: &id,
                    },
                    &summary,
                );
            Ok(published && tagged)
        }
        Command::Tag {
            source,
            version,
            name,
            execute,
        } => {
            let name = name.unwrap_or_else(|| project_name(&source.repo));
            let outcome = tag_release(&TagRequest {
                repo: &source.repo,
                name: &name,
                version: &version,
                sha: &source.sha,
                execute,
                identity: None,
            })?;
            println!("{}", describe_tag(&outcome, &version, &source.sha));
            Ok(true)
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

fn describe_tag(o: &TagOutcome, version: &semver::Version, sha: &str) -> String {
    match o {
        TagOutcome::WouldTag => {
            format!("would tag v{version} -> {sha} (dry run: nothing was created; use --execute)")
        }
        TagOutcome::AlreadyTagged => format!("already tagged v{version} -> {sha}"),
        TagOutcome::Tagged => format!("tagged v{version} -> {sha} (read back from origin)"),
    }
}

/// The repository directory's name, for the tag message.
fn project_name(repo: &Path) -> String {
    std::fs::canonicalize(repo)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "release".to_owned())
}

/// What `publish --tag` needs beyond the summary.
struct TagAfter<'a> {
    source: &'a Source,
    execute: bool,
    any_excluded: bool,
    version: Option<&'a semver::Version>,
    name: Option<String>,
    ledger: &'a Path,
    run_id: &'a str,
}

/// Tag the release once a publish has put everything on the registry. `true` unless a tag that
/// should have been made was not; "not yet" and "not for this run" are reported and are not failures.
fn tag_after_publish(t: &TagAfter<'_>, summary: &Summary) -> bool {
    if t.execute && !summary.is_success() {
        println!(
            "not tagged: the run did not finish; run the same command again to resume, it tags when done"
        );
        return true;
    }
    if t.any_excluded {
        println!(
            "not tagged: crates were excluded from this run; tag when the set is complete with \
             `releaseant tag --sha {} --version <version> --execute`",
            t.source.sha
        );
        return true;
    }
    let version = match release_version(&summary.plan, t.version) {
        Ok(v) => v,
        Err(why) => {
            println!(
                "{}not tagged: {why}",
                unless_dry(t.execute, "PUBLISHED but ")
            );
            return false;
        }
    };
    let name = t
        .name
        .clone()
        .unwrap_or_else(|| project_name(&t.source.repo));
    let outcome = tag_release(&TagRequest {
        repo: &t.source.repo,
        name: &name,
        version: &version,
        sha: &t.source.sha,
        execute: t.execute,
        identity: None,
    });
    match outcome {
        Ok(o) => {
            println!("{}", describe_tag(&o, &version, &t.source.sha));
            if o == TagOutcome::Tagged {
                record_tag(t, &version);
            }
            true
        }
        Err(why) => {
            println!(
                "{}not tagged: {why}\nfix that, then run `releaseant tag --sha {} --version {version} --execute`",
                unless_dry(t.execute, "PUBLISHED but "),
                t.source.sha
            );
            false
        }
    }
}

fn unless_dry(execute: bool, text: &'static str) -> &'static str {
    if execute { text } else { "" }
}

/// The ledger notes the tag; a failure to write it is reported, the tag itself stands.
fn record_tag(t: &TagAfter<'_>, version: &semver::Version) {
    let event = Event {
        ts: SystemClock.now_utc(),
        run: t.run_id.to_owned(),
        sha: t.source.sha.clone(),
        crate_name: None,
        version: Some(version.to_string()),
        event: EventKind::Tagged,
        detail: format!("v{version} read back from origin"),
    };
    if let Err(e) = Ledger::open(t.ledger).and_then(|l| l.append(&event)) {
        println!("warning: the tag stands but the ledger could not record it: {e}");
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
