# Changelog

## 0.0.16

- Pre-upload metadata check (ROADMAP R6), split by who refuses (PM, 2026-10-29). **Errors** stop `plan`, `verify` and `publish` before anything is written: no `description`, neither `license` nor `license-file`, more than five `keywords` or one crates.io refuses (ASCII, at most 20 characters, starting alphanumeric, only letters, digits, `_`, `-`, `+`), more than five `categories` or one that is not a crates.io slug, a named readme or licence file that is missing. **Warnings**, listed on stderr with exit 0: no `repository`, README, `keywords`, `categories` or `rust-version`. `--require-metadata FIELD,...` (on `plan`, `verify` and `publish`) turns chosen warnings into errors. Every problem of every planned crate is listed with what to add; crates outside the plan (excluded, `publish = false`) are not looked at. New modules `preflight` and `categories` (crates.io's 104 category slugs as of 2026-10-29), `PipelineError::Metadata`, `pipeline::check_metadata`, `PublishArgs::require_metadata`, `Summary::warnings`, more fields on `metadata::Package`. Found by mutating the README sample: deleting `description` used to leave `plan` and the dry-run `publish` green (cargo package only warns). At `origin/main` on 2026-10-29 every real set plans with zero errors and only warnings; see `docs/ADOPTION.md` open item 1.
- The README sample is now a complete workspace (workspace-inherited metadata); README tests delete its `description` and expect `plan` to fail naming the crate, delete its README files and expect warnings, and exercise `--require-metadata`.
- The run id, which names the scratch directory, now includes the process id: two runs on the same commit in the same second shared one scratch directory and the first to finish deleted the other's archive (found by parallel tests on identical demo commits).

## 0.0.15

- `docs/PATENT-SWEEP.md` third pass: five of the seen-but-unread results read claim by claim (hits 22 to 26, starting with Amazon's log-based transaction manager, US20150378775A1, read through its grant US10282228B2), all outside as read. 26 read in all, none needs review; 17 results remain unread and are listed. One design constraint added.
- `docs/ADOPTION.md`: the R5 adoption note with dry-run evidence for Unpopped (4 crates) and PersistAnt (9) at their tagged releases and at current `main`, compared with the order `workspace-publish.py` actually published in (from crates.io's `created_at`) and with a real-registry `readback` of all 13 crates; adoption order and a first-real-use sequence; six open items, the first found by this work (a missing `description` or `license` is not caught before upload).
- README: a sample workspace (the three manifests), the install time, and what to do with `.releaseant/`, after a second fresh-eyes trial. `tests/readme.rs` now builds the demo workspace from the README's own `toml` blocks, so the sample is the sample that runs (checked by mutation: a wrong dependency version in the README fails the test).

## 0.0.14

- Tag after publish (CIRESNAVE-EXPECTATIONS 6.20; replaces `tools/tag-release.sh`): `releaseant tag --sha S --version V [--name N] [--execute]` and `publish --execute --tag [--tag-version V] [--tag-name N]`. An annotated `v<version>` on exactly the published commit, pushed alone to origin and read back; the commit must be on an origin branch; an existing tag is never moved, whether local, on origin, annotated or lightweight (the script's `^{}` lookup missed a lightweight one); dry-run unless `--execute`. `--tag` tags only when the run has put everything on the registry and nothing was excluded, picks the one shared version or asks for `--tag-version`, and records an `EventKind::Tagged` ledger event. New module `tag`, `Summary::plan`.

## 0.0.13

- `docs/PATENT-SWEEP.md` second pass (the PM's condition for the first crates.io publish): one `patents.google.com` query each for ideas 6 to 9 with a positive control, and eight more patents read claim by claim (including the three priority reads from the first pass). 21 read in all, none needs review. A fourth published application, Red Hat's "Tagging non-upstream source code", was checked for a grant (US9639350B2) and the grant was read; two design constraints added.

## 0.0.12

- `docs/PATENT-SWEEP.md`: first-pass patent sweep of the code at 0.0.11 (13 patents read claim by claim, none needs review; routes not run and ideas with thin coverage are listed in the file). It is not complete until the PM accepts those gaps. The README no longer repeats the version number.

## 0.0.11

- README rewritten around a ten-line hello-world (`plan`, dry-run `publish`, the not-a-SHA error). `tests/readme.rs` runs every `$ ` command in the README's console blocks against a demo workspace and compares the output with what the README prints, so the README cannot drift (checked by mutating one README line: the test fails).
- `publish` prints the ledger path with forward slashes on every platform (it printed `.\.releaseant/ledger.jsonl` on Windows). Found by the fresh-eyes trial; `tests/readme.rs` compares the output exactly.
- A branch name or short SHA for `--sha` is refused with `get one with `git rev-parse <it>``.

## 0.0.10

- `--repo` naming a directory that is missing or is not a git repository is reported as such, with git's own message (`SourceError::NotARepository`). It used to be reported as "not a commit in this repository", the same text as a SHA that really is unknown, which hid a mistyped path.

## 0.0.9

- `publish --exclude NAME --exclude-reason TEXT` leaves a crate out of the run (nothing packaged, verified or uploaded for it) and records `excluded from this run: <reason>` in the ledger; `plan --exclude NAME` previews it. A planned crate that needs an excluded one still needs it on the registry (`missing_externals` now treats a publishable member outside the plan as external). `Plan::without`, `PublishArgs::excluded`, `PipelineError::{ExclusionWithoutReason, ExclusionForUnknownCrate}`. Found by `plan` on baracuda: `baracuda-cuda-seam` could not be left out.

## 0.0.8

- External prerequisites: `externals::missing_externals`, `Registry::versions`, `Index::versions`; `publish` refuses (before writing anything) when a dependency outside the set is not satisfiable on the registry; `plan --check-externals`.

## 0.0.7

- R4 preparation: `docs/REHEARSAL.md` and `docs/rehearsal/rehearse.sh` (the real-registry rehearsal on three placeholder crates, run by the PM with the token; the lane never sees it). Names verified free.

## 0.0.6

- R3b: `releaseant` CLI (`plan`, `verify`, `publish` (alias `resume`), `readback`, `yank`; dry-run unless `--execute`), real crates.io registry (sparse-index lookups, `cargo publish` adapter with SHA-256 of the uploaded `.crate`, server-advised rate-limit waits), `pipeline` (archive, package as a set, verify, run, consumer checks for recorded verification exceptions), `yank` with recorded reasons. `Cargo::package_set`. New `EventKind::Yanked`.

## 0.0.5

- R3a process layer: `source` (commit archive, independent of line-ending config), `cargo` (run cargo, package), `classify` (cargo output against captured samples; unknown stays unknown), `verify` (build the extracted tarballs). Captured cargo output in `tests/fixtures/`. Real-cargo test of the dev/optional dependency table.

## 0.0.4

- R2 core library: metadata parsing, publish plan (edge rule, cycle error, prerelease check, deterministic order), append-only ledger, run loop with resume, rate-limit waits, read-back and checksum comparison, over `Registry`/`Clock` traits with a scriptable fake. Decisions on the DESIGN review items recorded in `docs/DESIGN.md` section 10.

## 0.0.3

- R1 design: `docs/DESIGN.md` (publish-set rule measured with real `cargo package`, cycle = error, failure classification, ledger and resume, read-back, yank policy, test plan with a negative control per trap).

## 0.0.2

- R0 survey: `docs/SURVEY.md` lists what the hand-written publish tooling does, what it shares, and what none of it does.
- LICENSE-MIT holder is now "The releaseant contributors" (PM, quoting CireSnave 2026-10-24 on decision 263: "Go with your recommendations").

## 0.0.1

- Repository seeded by the PM: CI, deny policy, licences, roadmap and first handoff. No code yet.
