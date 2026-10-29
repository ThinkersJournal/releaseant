# Changelog

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
