# Changelog

## Unreleased (version at gate)

- Docs only: the owed patent re-sweep is done (eighth pass in `docs/PATENT-SWEEP.md`: 12 queries, 16 new patents read at every independent claim and 3 re-read, all outside as read; the plan's Q14 control was not met as written, recorded), ROADMAP R6 updated, and `docs/PUBLISH-PLAN.md` (the plan to publish releaseant itself; not run).

## 0.0.40

- Docs only: `docs/PATENT-RESWEEP-PLAN.md` (the exact queries, reads, wording rule, estimated cost and recording place for the owed re-sweep; nothing in it has been run) and `docs/HAND-PUBLISH.md` (checklist for a crate published by hand and the ledger line that clears the Held stop).

## 0.0.39

- Docs only: ROADMAP R6 marks the two second-hand rows (commit is origin's tip, 0.0.33; refuse a version the registry holds without our upload, 0.0.35) as done.

## 0.0.38

- Docs only: `docs/ADOPTION.md` section 3 no longer tells baracuda to `--exclude baracuda-cuda-seam`; it is in the set (PM, 2026-10-31), with the check at `ed1699af`.

## 0.0.37

- Docs only: R5 item 6 in `docs/R5-ADOPTION-DRYRUN.md`: baracuda `ed1699af` plans 73 crates with `baracuda-cuda-seam` in the set (line 58, after `baracuda-cuda-emit`), the exclusion dropped by the PM's decision; an earlier reading at `8f9b3013` is replaced.

## 0.0.36

- Docs only: R5 update in `docs/R5-ADOPTION-DRYRUN.md`: fuel `59bcbd3d` and baracuda `8f9b3013` dry runs; the hand-published seam trio is `Held`, the PM ledger line that clears it, and the run clearing it on a copy of the ledger. Registry checksums equal a `cargo package` of that commit on one machine; cross-machine reproducibility is not measured.

## 0.0.35

- Behaviour change: a crate already on the registry that this tool did not upload now stops the run (`Held`), dry runs included (DESIGN section 17). Only an `UploadStarted` or `Visible` ledger event for that crate, version and commit lets a run treat it as already published. No override flag; the content comparison is not built.

## 0.0.34

- Docs only: the design for a version the registry already holds (DESIGN section 17): no code. A held version with no ledger entry for this SHA becomes a report line and a resumable stop, no `--accept-held` override until asked; content comparison options, with same-machine packaging reproducibility measured and cross-machine not.

## 0.0.33

- `publish --execute` refuses a commit that is not the tip of origin's default branch (asked of origin with `git ls-remote --symref`; an unreadable origin is a refusal too). `--allow-not-tip` overrides; dry runs are not checked (DESIGN section 16).

## 0.0.32

- Docs only: the patent re-sweep for the partial-publish and per-crate-tag family (seventh pass, `docs/PATENT-SWEEP.md`): three queries, three claim-1 reads, nothing found, low confidence. The sweep stays INCOMPLETE ("outside as read"). The owed re-sweep is NOT done (ROADMAP row kept).

## 0.0.31

- `publish --only NAME --tag` tags each selected crate that is on the registry as an annotated `<crate>-v<version>` on the run's commit, pushed and read back; there is no project tag for a partial run, and a selected crate that did not land is named and left untagged while the others are tagged (PM ruling, DESIGN section 15). `--tag-version` cannot be combined with `--only`. `releaseant tag --crate NAME` tags one crate on its own. The whole-project `v<version>` is unchanged and still comes only from a run that publishes the entire plan. Ledger: one `tagged` event per tag, with the crate and the tag name. Library: `TagRequest::tag`, `tag::{release_tag, crate_tag, landed_crates}`.
- A crate that was already on the registry is tagged only when the ledger shows this commit uploading it (a resume of the same run); one published earlier from another commit is reported as `published earlier, commit unknown` and left untagged, because a tag on this commit would point at source the registry never saw. A crate tag that exists on another commit is refused, never moved.
- Not done: the patent re-sweep for this capability family (ROADMAP, owed before any release).

## 0.0.30

- `docs/DESIGN.md` section 15: design for tagging a partial publish (R6 row 3): per-crate `<crate>-v<version>` tags for `--only --tag`, full runs unchanged. Records the PM's ruling (per-crate tags for a partial publish; `v<version>` only when the whole plan is published) and that a patent re-sweep is owed, now a ROADMAP row. Design only.

## 0.0.29

- `publish --only NAME` (repeatable) runs just the named crates: every other crate of the plan is left out through the `--exclude` path (not packaged or verified, recorded in the ledger as `excluded from this run: not selected by --only`), and a named crate that needs a left-out crate needs it on the registry already. Not combinable with `--exclude` (clap, and `PipelineError::OnlyWithExclusions` for library callers); an unknown name is an error; a partial run is not tagged. `plan --only` shows the same selection.
- `plan --registry` prints `(on registry)`, `(not on registry)` or `(registry unknown: why)` after each crate; a failed lookup is never "not on registry". The default `plan` output is unchanged (R6 row 2, DESIGN section 14). Seen on this repo: `releaseant 0.0.28 (not on registry)`; the "on registry" state is covered by the unit test of the wording and the existing index-lookup tests, not by a live crate of this repo.

## 0.0.28

- `docs/DESIGN.md` section 14: design for `--only` (partial publish) and `plan --registry` (R6 row 2, from the Vulkane publish). Design only; no code change yet.

## 0.0.27

- `publish` prints one progress line per step on stderr with the time since the command started (`[03:12] uploading foo 0.1.0`, `upload returned foo 0.1.0 (17 s)`, `read back, visible`, waits, skips), so a long run is no longer silent (R6 row 1, DESIGN section 13). The lines mirror the ledger (the ledger gains an observer called after each append); packaging and each verification get their own lines. stdout, exit codes and the ledger are unchanged. Library: `progress` module, `PublishArgs::on_progress`, `verify::verify_set_with`.

## 0.0.26

- `docs/DESIGN.md` section 13: design for per-step progress lines on stderr during `publish` (R6 row 1, the PM's silent 20-minute run). Design only; no code change yet.

## 0.0.25

- `docs/PATENT-SWEEP.md`: sixth pass, the WIPO Patentscope pass (route B) through the web search tool limited to `patentscope.wipo.int`, nine dated queries (P1 to P9), one per idea. Five returned no patent record, four returned unrelated records; one record (WO2002057918) read with `tools/patent-claims.py`, outside as read. Recorded as low-confidence "found nothing"; the sweep stays INCOMPLETE; what was not searched or read is listed.
- `docs/FIRST-REAL-USE.md`: "Notes from the PM's two real publishes" (unpopped 0.34.11, configurant 0.5.33): three first-hand candidates from the PM (silent run, no `--only`, tagging a partial publish) plus two second-hand, labelled so.

## 0.0.22

- `docs/FIRST-REAL-USE.md`: the first real run (configurant `64e090a`, 0.5.22, five crates, `releaseant 0.0.21`, run by the PM, exit 0) is recorded as *observed*: 404 s in all, per-crate upload times, the 23-event ledger in order, `published and read back` five times and the tag read back from origin. Verified afterwards against crates.io (all five at 0.5.22) and the origin tag. Failure and recovery paths are still unseen and are marked so.

## 0.0.21

- `docs/FIRST-REAL-USE.md` re-done after a second dry run at configurant `daa0fd1` (0.5.21, 0.0.19 built from releaseant `origin/main`; exit 0, empty stderr, 217 s): a new section 0 says how to get a binary (none is installed) and how the version flows from the commit's manifests to the tag; `readback` exits 1 when crates are absent (expected before a first publish); the ledger order is five `verified`, then `planned`, then `finished` (the runbook had `planned` first, wrong since 0.0.6).

## 0.0.20

- `docs/PATENT-SWEEP.md` fifth pass: the last eleven results seen but unread read claim by claim (hits 33 to 43), all outside as read; 43 read in all, none needs review, no result a query returned is unread. Claims for these came through a summarising fetch (paraphrase), recorded under Limitations. The sweep stays INCOMPLETE; the PM accepted the 43 reads as sufficient for the first publish (2026-10-30), listed the routes not run with costs in the sweep, and scheduled WIPO Patentscope as a ROADMAP row.

## 0.0.19

- `publish` prints the metadata warnings as soon as they are known, before the first build or upload, instead of after the run: a run that fails or is interrupted later (a missing external, a cargo refusal) used to lose them, and a long run showed them last. `PublishArgs::on_warnings` is the hook (`None` keeps the old behaviour of reading `Summary::warnings`); `plan` and `verify` already printed them up front. Test `warnings_are_shown_before_anything_is_built`; control: moving the call to the end of `publish` makes it fail (the scratch `target` directory exists by then).

## 0.0.18

- A project setting for the required metadata fields (ROADMAP R6): `releaseant.toml` at the root of the repository, read from the commit being published, never the working tree, so the requirement is versioned with the release it governs: `[metadata]` with `require = ["keywords", "rust-version", "readme"]` makes those warnings errors on every `plan`, `verify` and `publish` of that commit. The command line's `--require-metadata` adds to it; neither relaxes the other, and neither relaxes what is an error anyway. An unknown key, a wrong type, bad TOML or an unknown field name is an error naming `releaseant.toml` and stopping before anything is written (a misspelt requirement that enforced nothing would be worse than none). New module `config`, `Prepared::config`, `PipelineError::Config`; new dependency `toml` 1.1.8 (newest on crates.io). The error text for a required field now says `required by --require-metadata or releaseant.toml`.

## 0.0.17

- `docs/FIRST-REAL-USE.md`: the runbook for the first real publish (the next ConfigurAnt batch, 5 crates, run by the PM with the lane on call): preconditions with the expected output of each, the exact commands, what each output line means, what to read back afterwards, and the recovery for every failure including `PUBLISHED but not tagged`. Built from a full dry run of the same set (753 s on a loaded machine, ledger of 7 events, shared checkout untouched). Writing it found that against the real registry any cargo refusal the tool has no sample for (a missing token, name taken, validation) prints `INTERRUPTED: unrecognised upload result for X: ...` and stops the run, not `FAILED:`.
- `docs/PATENT-SWEEP.md` fourth pass: six more of the unread results read claim by claim (hits 27 to 32), all outside as read; 32 read in all, 11 unread and listed. The closest on its face is the BlueTie application's claim 1 (identify dependencies, determine ordered lists of execution); its grant (US9477497B2, lapsed) and abandoned continuation need a web browser downloading modules from a server and hard versus soft dependency types. One design constraint added.
- `SECURITY.md`: a "Supported versions" section, the PM's text verbatim (the latest line gets security fixes; an older line a backport only when it is small; no support period is promised).

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
