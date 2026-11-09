# Changelog

## 0.0.69

- CLI slice 2a: `publish --artifacts-only --artifacts-dir <dir>` stages and reads back the declared artifacts (files to a draft GitHub release through `GhForge`, images through crane, images tagged) with no crate step and no project tag; without `--execute` it prints the plan. The GitHub repository is derived from the crates' `repository` (`src/release_repo.rs`); `--artifacts-repo owner/name` fills a missing one and must equal a declared one; a missing, non-GitHub or conflicting `repository` is a named refusal, never a guess. New `--crates-only`; a real `publish` of a commit that declares `[[artifact]]` refuses unless `--crates-only` or `--artifacts-only` is given (the mixed release is slice 2b).
- Tests that spawn the binary (`tip_cli`, `ci_cli`, `artifacts_cli`) now run with an empty `CARGO_HOME` and no registry token variables, after a mutation check of a new test published a fixture crate (`zzart-one` 0.1.0, yanked) through the shared `CARGO_HOME` credentials.

## 0.0.68

- Artifacts slice A3: `GhForge` (`src/gh_forge.rs`), the real `stage::Forge` over `gh api` (`$GH`, else `gh` on the PATH): finds a release by LISTING (a draft is not returned by `releases/tags/<tag>`), remembers the id of a draft it created because the listing can miss it for a moment, refuses a stored asset name that differs from the requested one (GitHub turns spaces into dots), reads back by downloading and hashing the bytes, and undrafts after an annotated tag exists. `tests/forge_runner.rs` (feature `runner-tests`) runs the whole cycle against `ciresnave-bot/releaseant-artifact-rehearsal`. Measurements, dated, in `docs/ARTIFACT-MEASUREMENTS.md` (A3); size caps, 429 wording and the state a dropped upload leaves stay `UNVERIFIED`. Not reachable from `publish` yet.
- Fix: `Cargo::check` (the verification build of a packaged crate) now always passes `--target-dir <scratch>/target`. Left to the environment, a `CARGO_TARGET_DIR` (or `build.target-dir`) made every verification share one build cache, and a crate with the same name and version as an earlier different one could verify OK without being built from its own tarball. Measured: with `CARGO_TARGET_DIR` set, `pipeline::tests::a_crate_that_only_builds_from_the_tree_blocks_every_upload` failed in 3 of 4 full lib runs, with it unset 0 of 4 (the "known flake"); after the fix 0 of 4 with it set. `verify::tests::crate_that_only_builds_from_the_tree_fails_verification` now also asserts the build went into `<scratch>/target` (fails if the flag is dropped). Not shown: that hosted CI had `CARGO_TARGET_DIR` set when it flaked.

## 0.0.67

- `plan --check-externals` now also reports whether `crane` can be run (`$CRANE`, else the PATH) when `[[artifact]]` image artifacts are declared: a `crane` line with its version, or `MISSING EXTERNAL: crane (reason)` and a non-zero exit. New `Crane::from_env` and `Crane::version`. `release()` is still not reachable from `publish` (it needs the real forge, slice A3).

## 0.0.66

- Docs only: ROADMAP R9 and the status of `docs/ARTIFACTS-DESIGN.md` now say that artifacts slices A1, A2, A4 and A6 are built and merged (0.0.61 to 0.0.65) and that A3, A5 and the CLI wiring are open. No code change.

## 0.0.65

- Artifacts slice A6, part 2: the real-`crane` check against a real `registry:2` as a committed test, `tests/crane_runner.rs`, behind the new `runner-tests` cargo feature (not `#[ignore]`d; a missing Docker or `crane` is a failure with a message). `.github/workflows/runner-tests.yml` runs it on the self-hosted runner (push to main, schedule, manual dispatch only; no `pull_request` trigger, no secrets; inert until the repository variable `RUNNER_TESTS_ENABLED` is `true`); `runner-tests-detector.yml` goes red when no run has succeeded in 7 days. The hosted `cargo test` steps no longer pass `--all-features` (it would run the runner test without Docker; there was no other feature), clippy and doc still build everything. `docs/RUNNER-TESTS.md` says plainly that the runner lane is NOT running yet: no runner is registered for this repository, so no CI run has exercised the real-crane check. Nothing in the library changes.

## 0.0.64

- Artifacts slice A6, part 1 (`docs/ARTIFACTS-DESIGN.md` section 7): `release::release` runs a mixed release in the designed order against any `Forge`, `ImageRegistry` and `Registry`: stage artifacts (draft release, images pushed by digest, nothing public), publish the crates, promote the image tags, create the project tag through a caller-supplied step, then publish the draft release onto it. A step that does not finish stops the sequence: a held or unverified artifact means no crate is published and no tag exists, a failed file stage stops before any image is pushed, an image that cannot be tagged or a failing tag step leaves the release a draft. The artifact declarations are part of the plan identity (a changed declaration for the same commit is refused). `Select::{Both, CratesOnly, ArtifactsOnly}`; a dry run touches nothing. `Forge` gains `publish_draft`. Not reachable from the CLI yet (it needs a measured real forge, slice A3).

## 0.0.63

- Artifacts slice A4 (`docs/ARTIFACTS-DESIGN.md`, measurements in `docs/ARTIFACT-MEASUREMENTS.md`): `images::stage_images` pushes `image` artifacts BY DIGEST (no tag) through `crane`, after checking the local OCI layout's digest and platforms against the declaration; a tag already pointing at another digest holds the image before anything is pushed; the registry's served manifest is hashed and every child manifest is looked up before the image counts as visible. `images::promote_images` sets the tags last, checking all of them first and moving none if one is taken, reading each back. `ImageRegistry` trait with a fake and a `Crane` implementation; `classify_digest` tells absent from could-not-ask using the real crane wording. New ledger event `artifact_promoted`. Image sources are `dir:` OCI layout directories only: `oci-archive:` is refused with a named error (crane cannot push an OCI tar). Not reachable from the CLI yet (A6). Nothing about Docker Hub is measured.

## 0.0.62

- Artifacts slice A2 (`docs/ARTIFACTS-DESIGN.md`): `stage::stage_files` stages `file` artifacts into a DRAFT release through a `Forge` trait (tested against an in-memory fake; no real forge and no CLI path yet). Each file is hashed locally and must equal its declaration before anything is uploaded; an upload counts only when the bytes the forge serves back hash to the declaration; an existing asset is never replaced or removed (the same bytes with a ledger entry for this commit resume as already published, anything else, a partial asset or a public release is held); a forge that cannot be asked stops the run, resumable. Six `artifact_*` ledger event kinds.

## 0.0.61

- `[[artifact]]` in `releaseant.toml` (design: `docs/ARTIFACTS-DESIGN.md`, slice A1): installers (`kind = "file"`, a GitHub release asset) and container images (`kind = "image"`) are parsed and validated, every problem of every artifact listed at once; a field that does not belong to the kind, an unknown key, a path that leaves the artifacts directory, a bad hash, tag, platform or target are errors naming the artifact. `plan` prints one line per artifact after the crates. `artifact::combined_fingerprint` is the plan-hash extension: unchanged when there are no artifacts, so an old ledger still resumes. Nothing uploads yet and the run loop does not read artifacts (slice A2).

## 0.0.60

- Docs only: `docs/ARTIFACTS-DESIGN.md`, the design note for publishing installers (GitHub release assets) and container images against a declared SHA-256, read back by hash, never overwriting, tagging last; ROADMAP row R9. No code.

## 0.0.59

- Fix: an empty `--require-checks` entry (`--require-checks ""`, or `a,,b`) is refused at once with exit 2 and a message naming the entry, instead of being judged as a check with no name that can never be found and failing only after the whole `--ci-wait` (found in Fuel's second real publish). Docs: `docs/R5-LIVE.md` section 5 reads that publish (0.36.7, 61 min 34 s, loaded box) against the first: zero 429s again, the registry's share unchanged, the extra 910 s is local builds. Ledger and transcripts kept under `docs/rehearsal/`.

## 0.0.58

- `plan --check-rust-version` and `publish --check-rust-version` (off by default): a crate that declares a `rust-version` is refused (`RUST-VERSION`, exit 4, before the ledger opens or anything builds) when no live version of a dependency that satisfies the requirement fits it, judged from the `rust_version` the index records per version, or when a crate of the same set declares a newer one. The newest match being too new while an older one fits, and target-limited dependencies, are notes. Dev and optional dependencies are skipped. A registry that cannot be asked is an error, never a pass. New `Registry::version_infos`.

## 0.0.57

- `plan --check-names` and `publish --check-names` (off by default): for every crate whose planned version is not on the registry, the name is free (no versions in the index), or exists and has an owner of ours (`--owner LOGIN`, or `[names] owners` in `releaseant.toml`), or it is refused: a name held by none of ours is exit 4 (`NAME TAKEN`, with the owners), and so is a lookup that failed (`UNKNOWN`, never read as free). `publish` checks before the ledger is opened or anything is built. No owner named is an error. Read-only; crates.io's owners list is asked at most once a second.

## 0.0.56

- `--require-checks A,B`: `--require-green` judges only the named checks and fails closed (a named check that is absent or unfinished is exit 4, one that failed is exit 3, every other run is ignored). Without names it judges the checks that branch protection of `--protection-branch` (default `main`) requires, read from the forge; if that requires none or cannot be read, every run on the commit is judged as before, and a line printed first says which rule ran. Names are job names, not workflow names. Why: two queued self-hosted runs that gate nothing held Fuel's release up (`docs/R5-LIVE.md` section 4e).

## 0.0.55

- Docs: `docs/R5-LIVE.md` section 4 reads the real Fuel 0.36.4 publish (43 crates, one run, 46 min 24 s) from its ledger: no 429 at all (43 accepts in 29.5 min), builds are 89.8 % of the run, `fuel-datasets` dominates both build passes, and why `--require-green` was unusable there (two queued self-hosted runs outside the required checks), with the options. The ledger and transcripts are kept under `docs/rehearsal/`. No code change.

## 0.0.54

- Ledger: when an upload is rate-limited, the registry's own words (the 429 line cargo printed, cut to 400 characters) are kept as a `classified` event, `rate-limited: <line>`, so a ledger keeps the wording of each limit it met (the real baracuda run's 20 rate limits left only the parsed `retry_after`). New `Registry::take_rate_limit_text`, default none. Waiting and every outcome are unchanged.

## 0.0.53

- Docs: `docs/R5-LIVE.md` section 3 reads the real baracuda publish (73 crates, one run, 69 min 46 s) from its ledger: the time split, 20 real crates.io 429s (a burst of 46 accepted uploads, then about one a minute), the cost of each build pass (R8), and a 16 min 50 s stall that was the laptop sleeping. The ledger and transcript are kept under `docs/rehearsal/`. No code change.

## 0.0.52

- Docs: `docs/ADOPTION.md` gains "Notes for callers", the steps of a real multi-crate publish (`plan --check-externals` first, cross-repository order, the command with `--max-wait`, `--retries` and `--require-green`, rate limits, resume, `--reuse-verified-hours`, exit statuses, reading the ledger). No code change.

## 0.0.51

- New `publish` flag `--reuse-verified-hours N` (default 12; 0 = off). A crate the ledger shows as `verified` at the same commit and version no more than N hours ago is not built again, so a resume after a rate-limit stop, or each `--retries` run, no longer repeats the whole verification (105 minutes for Fuel's 43 crates at 0.35.2). The ledger records the crate as `verified` with `carried over, not built again: run <id> at <time>`, and the progress line says how many were carried over. The first run at a commit still builds everything. Additive.
- Measured (dry runs, no writes): `plan` and `verify` on baracuda `87957c57` (73 crates, 73 of 73 verified in 8.5 minutes) and fuel `3f6ec6a4` (43 crates; its `verify` failed at once because `baracuda-kernels-sys ^0.0.1-alpha.158` is yanked, and `plan --check-externals` says so in seconds); see `docs/R5-LIVE.md`.

## 0.0.50

- Measured live (`docs/R5-LIVE.md`): `--require-green` against the real forge exits 3 on a commit whose CI failed, 4 on a commit with no runs, and a green commit passes the guard. `--retries` against crates.io on the three rehearsal crates: confirmed after 1 run (0.0.2, no lag seen), and after 2 runs when the first `cargo publish` was made to fail (0.0.3, failure injected at the cargo boundary; a real transient failure and a lagging index remain unmeasured). First real `phase` ledger events recorded.
- Fix: with `--retries`, a run that stopped is reported as `the run did not complete: <reason>` (it said `not yet on the registry` and showed the line ending as `\r\n`).
- Docs: `docs/ADOPTION.md` maps `publish-loop.sh` and `publish-when-green.sh` to the flags; `docs/REHEARSAL.md` corrected (the vault has no crates.io token entry).

## 0.0.49

- New `publish` flags, both off by default (ROADMAP R5, retiring the hand-written publish loops). `--retries N` (with `--execute`; `--retry-pause` seconds) runs again after any failure until the index, asked afresh, lists every crate of the plan at its version and not yanked; a held version or a checksum mismatch stops it at once. `--require-green` (`--github OWNER/NAME`, `--ci-wait` seconds) waits for the commit's CI runs and refuses unless all succeeded: exit status 3 on a failed run, 4 on no answer (no runs at all is not green). Additive: without them nothing changes. New `ci` and `retry` modules; `tests/ci_cli.rs`.

## 0.0.48

- Fix: a run id (and so its scratch directory) is unique within a process. It was the first 8 characters of the SHA, the second and the process id, so two runs on one commit in one second inside one process (the in-process tests, or a binary wrapping `releaseant::cli::run`) shared a scratch directory and the first to finish deleted the other's archive. A per-process counter is now part of it. The run id in the ledger gains a fourth field.

## 0.0.47

- New public API: `releaseant::cli::run(args)` (and `run_code`, the same returning the exit status as a number) is the whole command line; `src/main.rs` is a one-line call to it, so the library's CLI can be tested in process and wrapped by other binaries. Additive: behaviour, output and exit statuses are unchanged (clap errors, help and version included). New `tests/cli_inproc.rs`.

## 0.0.46

- Ledger: new event kind `phase` with timing notes, so a run says where its wall time went: package time, `cargo check` time per crate, and for each upload the offsets of cargo publish's status lines (first `Compiling`, `Verifying`, `Finished`, `Uploading`, `Uploaded`, `Waiting`, `Published`). Ledger-only: progress output is unchanged, and no check is skipped or reordered. Not yet seen on a real upload.

## 0.0.45

- Docs only: the README Status line says the crate is published (0.0.44) and early, pre-1.0, instead of "not ready for developer use"; the Install section no longer says "once it is published". ROADMAP R7 is closed with the evidence in `docs/PUBLISH-PLAN.md`.

## 0.0.44

- Docs only: `docs/PATENT-SWEEP.md` ninth pass reads the four patents the earlier passes had seen and not read (hits 63 to 66, every independent claim): all outside as read, none needs review. The sweep stays INCOMPLETE; not legal advice.

## 0.0.43

- CI only: the portfolio workflow is pinned to coderipper `b45eb41` (0.10.0, with the default-branch exclusion) instead of `c665210` (0.8.19), which turned the portfolio job red on `main`.

## 0.0.42

- README: the hello-world is the first section (SHA, `plan`, dry-run `publish`), run by `tests/readme.rs` on every CI OS and now a named CI step; the metadata paragraph moved below it; wording fixed after three fresh-eyes trials of the packaged form (demo directories, any README text, git identity, "the release owner" for "the PM").
- Package: an `include` list (the package no longer ships `HANDOFF.md`, `CLAUDE.md`, `.github/`, `docs/` or the ROADMAP); CI job `package` runs `cargo package --locked`.

## 0.0.41

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
