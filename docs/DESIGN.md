# ReleaseAnt design (R1)

Status: draft for the PM gate. Inputs: `docs/SCOPE.md` (traps 1-8), `docs/SURVEY.md` (what existing tooling does). Nothing here is code; each rule ends in a test with a **negative control** (a test that fails when the behaviour is removed), listed in the test plan.

## 0. Measured fact that shapes the design (answers SURVEY Q1)

Experiment, 2026-10-24, cargo 1.99.1, a throwaway 5-crate workspace with a random name prefix that does not exist on crates.io, dependency crate never published. Command per crate: `cargo package -p <crate> --allow-dirty`, with and without `--no-verify`.

| Crate has, on the unpublished crate `dep` | `cargo package` | with `--no-verify` |
|---|---|---|
| nothing (the dep itself) | packages | packages |
| dev-dependency, `path` only | **packages** (dev-dep stripped) | packages |
| dev-dependency, `path` + `version` | **fails**: ``no matching package named `dep` found`` | fails, same |
| normal dependency, `path` + `version`, `optional = true` | **fails**, same message | fails, same |
| normal dependency, `path` + `version` | fails, same message | fails, same |

Consequences:
- A dev-dependency that has a `version` must be on the index first, exactly as fuel's guard and the PM scripts assume. baracuda's header claim that "single-crate publish doesn't enforce dev-dep availability" is **false for a versioned dev-dep** at this cargo version (it can look true for a path-only one, which is stripped). `--no-verify` does not relax resolution.
- Optional dependencies count like normal ones.
- The **positive control** is the first two rows (same workspace, same command, packages); the **negative** is the rest. Scope of the claim: one cargo version, one registry state. The test suite re-runs this experiment with real `cargo package` (section 8, T2, not ignored or feature-gated) so a cargo change that alters it is noticed.

So the edge rule is: **an edge A -> B exists if A's manifest, as `cargo package` would publish it, names B with a version, in any of normal, build, dev or optional position.** Path-only dev-deps are not edges. This is read from the packaged manifest, not guessed from `cargo metadata` flags (section 2).

## 1. Shape

Library crate `releaseant` with a thin CLI (R3). Pipeline, each stage a pure function over data except the marked I/O ones:

```
source(archive) -> graph -> plan -> [verify] -> upload -> readback -> ledger
                     pure   pure    I/O build   I/O       I/O         I/O
```

I/O sits behind two traits so everything is testable against fakes: `Registry` (index/API/upload) and `Cargo` (package/build). No stage reads global state.

Dry-run is the default (SCOPE). `plan` and `verify` never upload. `publish` needs an explicit `--execute`, and even then refuses without a recorded source SHA (section 3).

## 2. Source and publish set (traps 1, 2, 6)

**Source (trap 1).** `publish` takes a git SHA, runs `git archive <sha>` into a new empty directory, and every later stage works only in that directory. A working tree, or a SHA that is not a full 40-hex commit reachable in the repo, is refused. The SHA is the ledger's primary key (section 5). `--allow-dirty` is never passed because the archive has no `.git` and no dirty state; if cargo needs it, that is a bug to report.

**Members.** `cargo metadata --no-deps` on the archive; a package is a candidate unless `publish = []`. A `publish = ["registry"]` allowlist that excludes crates.io also excludes it (the tool targets crates.io only in R0-R5).

**Edges.** For each candidate, edges come from its **packaged manifest**: the plan stage runs `cargo package -p <crate> --no-verify` into a scratch target and reads the packaged manifest **only when `plan --exact` is requested**; the default derivation is the cheap rule from section 0 applied to `cargo metadata` dependencies (every dep that names a workspace member and has a non-`*` `req`, any kind, optional included; path-only dev-deps skipped). The cheap rule and the exact one must agree on the test fixtures (T3); where they disagree the exact one wins and the disagreement is a reported finding. Rationale: the metadata `req` is `*` exactly when no version was written, which is how fuel's guard and the PM scripts distinguish stripped dev-deps; the exact mode exists to catch the case where that inference is wrong.

**Order.** Kahn topological sort with a deterministic tie-break (name). **A cycle is an error naming the cycle's members** (one shortest cycle found by DFS over the remaining nodes), exit non-zero, nothing uploaded. No retry-to-discover-order (SURVEY: baracuda) and no silent cycle (SURVEY: PM scripts). A known-publishable cycle shape (baracuda-types <-> baracuda-types-derive: one edge is a path-only dev-dep) is not a cycle under the edge rule, which is test T4's positive case.

**External prerequisites.** An edge to a crate outside the set (e.g. fuel waiting on another project) must already be on the index at a version satisfying the requirement; otherwise `plan` reports "missing external: <crate> <req>" before any upload.

**Prerelease requirements (trap 3).** For every edge, check with the `semver` crate that the version this plan will publish for the dependency satisfies the requirement. `^0.0.1-alpha.152` does not match `0.0.1-alpha.147`; the plan fails naming both, before any upload. Mismatches are computed from the metadata, not from the registry.

## 3. Per-crate state machine

```
pending -> verified -> uploaded -> visible
              |           |           |
           failed(...)  failed(...)  missing(timeout)
```

Terminal states per crate in a run: `visible`, `already-published`, `failed(reason)`, `skipped(blocked by failed dependency)`. A crate whose dependency is `failed` or `missing` is `skipped`, never attempted.

- **Verify (SCOPE intro).** `cargo package -p <crate>` into a scratch target dir, then extract the produced `.crate` into a fresh directory and `cargo check` it with the other crates of the set replaced by `[patch.crates-io]` paths to *their* extracted `.crate`s (the fuel `build` approach), so a manifest that only works from the repo tree fails here. Named hole, copied from fuel's tool and recorded in the report: sibling crates are patched by path, so this stage proves buildability from tarballs, not resolution against the registry. Resolution against the registry is proven by the real upload in order and by section 6.
- **`--no-verify` (trap 5).** Skipping verify is a flag `--no-verify-crate <name> --reason <text>`; it is written to the ledger as an `exception` event with the reason and **the run's report lists it**. After upload, readback adds a clean-consumer check for exception crates: a fresh project with `cargo add <crate>@<version>` and `cargo check`. A run with an exception whose consumer check fails ends `failed`, not green.
- **Upload.** `cargo publish -p <crate>@<version> --locked` from the archive (package-ids, because a bare name is ambiguous with a registry copy; SURVEY: baracuda).
- **Classification (section 4)** decides what each non-zero result means.

## 4. Failure classification and rate-limit policy (trap 4)

Classify by a closed enum, never by a loose substring (SURVEY: the `'rate'` match). Inputs, in order of trust: (1) HTTP status and headers where the tool itself talks to the registry; (2) for `cargo publish`, cargo's exit status plus a **pinned set of message patterns, each an anchored regex stored in one table with the captured real sample it was derived from**; (3) anything else is `Unknown`.

| Class | Action |
|---|---|
| `AlreadyPublished` | record, treat as done, but still run readback |
| `RateLimited { retry_after }` | wait, then retry the **same** crate; wait = server-advised time + margin if present, else exponential backoff with cap; unbounded retries with a total-time deadline (`--max-wait`), after which the run stops `resumable`, not failed |
| `DependencyNotVisible` | wait for index propagation (poll, section 6), retry; counts toward a per-crate deadline |
| `Transient` (DNS/connection/timeout) | bounded retries with backoff |
| `Permanent` (validation, auth, name taken, size) | stop this crate; dependents `skipped`; other independent crates continue |
| `Unknown` | **stop the run**, print the full output, record it. Never retried; an unrecognised message is never assumed to be a 429 |

The pattern table is the weakest part of any wrapper over `cargo publish`; R4 captures real samples (a real 429, a real already-exists, a real missing-dependency) from the rehearsal and the table cites them. Until R4 the table holds only patterns whose sample text is in the repo's tests as a fixture, and **unverified patterns do not ship**: they fall to `Unknown`, which fails safe.

Pacing: no fixed "after N uploads" budget (SURVEY: baracuda's 28 is a folk number). Pace reactively on `RateLimited`; the numbers crates.io documents (new-crate burst, then one per interval) are read in R4 from the current documentation/429 response and recorded in `docs/` with the date, not hard-coded.

## 5. Ledger and resume (trap 4)

Append-only JSON Lines file, one event per line, written and `fsync`ed before the next action; default `.releaseant/ledger.jsonl` **outside** the archive directory (the archive is disposable). Event fields: `ts` (UTC), `run` (id), `sha` (full commit), `crate`, `version`, `event` (`planned`, `verified`, `exception`, `upload-started`, `upload-finished`, `classified`, `visible`, `missing`, `waiting`, `skipped`, `finished`), and `detail`.

Rules:
- The ledger is a **record, not the source of truth**. Truth about "is it published" is the registry. On resume the tool reads the registry for each crate in the plan (sparse index first, API as the second opinion); the ledger supplies *history* (which SHA, exceptions, attempts) and detects **a crate whose registry state contradicts the ledger** (ledger says uploaded, registry says absent past the deadline) which is reported, not silently overwritten.
- A registry lookup that **fails** (timeout, 5xx, DNS) is `Unknown`, not "not published" (SURVEY: PM scripts treat any exception as unpublished and attempt an upload). Only a definitive "absent" answer (index 404 with an index that is otherwise reachable) permits an upload attempt.
- `resume` requires the same SHA and the same plan hash as the ledger's last run, else refuses and says which differs. A different SHA is a new run (a new version must exist anyway).
- Torn last line (crash mid-write) is tolerated: ignored with a warning; any other malformed line is an error.
- An `upload-started` with no `upload-finished` is resolved by asking the registry, never by assuming.

## 6. Read-back (trap 7)

After the last upload (and after each crate in the dependency wait path), for every crate in the plan: poll the sparse index entry (`index.crates.io/<prefix>/<name>`) until a line with the exact `vers` appears **and** `yanked` is false, then confirm the version through the API (`/api/v1/crates/<name>/<version>`) and compare the **checksum** in the index to the SHA-256 of the `.crate` we uploaded. Timeout per crate and overall; on timeout the report names every missing crate and the run ends `missing` (resumable), not success. "Success" is defined as all crates `visible` with checksum equal. Anything weaker is a different word in the report.

## 7. Yank policy (trap 8)

The tool **never yanks on its own initiative**. `releaseant yank <crate> <version> --reason <text>` exists, requires `--execute` and a non-empty reason, writes the reason and the yanking SHA to the ledger first, and refuses unless the version is in a ledger run (it will not yank what it did not publish). `publish` has no yank path at all: a failed run is resumed or abandoned, never auto-cleaned. Yank reasons that the policy accepts are an explicit allowlist of kinds (`security`, `broken-build`, `wrong-contents`, `licence`) recorded as a field, to make a pattern of yanks auditable; `other` requires a longer text. (The list is a starting point for PM/CireSnave review; the mechanism, not the list, is the design.)

## 8. Test plan (negative control per trap)

Every test runs against a fake `Registry`/`Cargo` unless marked **[real cargo]**, which uses real `cargo package` on generated workspaces with random never-published names (like section 0's experiment; no network writes).

| ID | Trap | Test | Negative control (must FAIL if behaviour removed) |
|---|---|---|---|
| T1 | 1 | Plan from SHA creates the archive; a file present only in the working tree (untracked, and a tracked file edited after commit) is absent/old in the archive and in the packaged `.crate` | Same plan with the "use archive" step replaced by "use the tree": the test sees the untracked file and must fail |
| T2 | 2 | **[real cargo]** the section 0 table, row by row | Change fixture so the dev-dep has no version: expectation flips; asserts both directions |
| T3 | 2 | Edge rule on fixtures: normal, build, versioned-dev, path-only-dev, optional, renamed (`package =`), target-specific | Plan built from normal deps only misses the versioned-dev edge (baracuda-nvrtc shape); test asserts the set contains it |
| T4 | 6 | Cycle of 2 and of 3 is an error naming members, nothing uploaded; baracuda-types shape (path-only dev edge) is not a cycle | Remove the cycle check: topo sort must not terminate normally/must be caught by the test's assertion on the error |
| T5 | 3 | `^0.0.1-alpha.152` against plan version `0.0.1-alpha.147` fails naming both; `.152` against `.152` and `.153` passes | Replace `semver` match with a major.minor.patch comparison: the `.147` case would pass and the test fails |
| T6 | 4 | Fake registry returns 429 N times then OK: crate ends published, run did not abort, waits were the advised ones (fake clock) | Replace the rate-limit arm with "treat as failure": test fails |
| T7 | 4 | Kill (drop) the run after crate k of n; resume publishes only the rest; registry shows each crate uploaded exactly once | Resume that ignores registry state re-uploads crate 1..k: fake registry rejects as duplicate, test fails |
| T8 | 4 | Lookup error (fake returns timeout) is `Unknown`/stop, never an upload | The PM-script behaviour (error = not published) would upload: test asserts zero uploads |
| T9 | 4 | Message-table: each pattern fed its captured sample classifies as expected; a message containing "generate"/"separate" classifies `Unknown` | The substring `rate` match makes the second case classify `RateLimited`: test fails |
| T10 | 5 | `--no-verify-crate` without `--reason` is refused; with it, an `exception` event and a report line exist; the consumer check runs and a broken exception crate fails the run | Skipping the consumer check: test with a crate that only builds from the tree must fail the run |
| T11 | 5 | Verify stage catches a manifest/`include_str!` that works in the tree but not from the extracted tarball | Disable extraction (check the tree): the same crate passes, test fails |
| T12 | 6 | Order puts dependencies before dependents before the umbrella; stable tie-break (shuffled input, same output) | Reverse edges: the dependency-before-dependent assertion fails |
| T13 | 7 | Fake index lags N polls: success only after the line appears; permanent absence yields a report naming the missing crate and a non-success status | Report success on upload-accepted: the lagging/absent cases fail |
| T14 | 7 | Index checksum differs from the uploaded `.crate` SHA-256: reported mismatch, not success | Skip the checksum compare: test fails |
| T15 | 8 | `publish` never calls `yank` (fake registry panics on yank); `yank` refuses without reason, without `--execute`, or for a version not in the ledger | Wire an auto-yank on failure: fake panics, test fails |
| T16 | ledger | Torn last line tolerated; corrupt middle line is an error; fsync ordering (event written before the action: crash-point test) | Write the event after the action: crash-point test sees an action with no record |
| T17 | dry-run | Default invocation of every command performs zero writes to the fake registry and zero ledger lines other than `planned` | Make `--execute` default: test fails |

Rehearsal (R4): a real throwaway set of placeholder crates on crates.io, run once with induced interruption, to capture the real message samples for T9 and to measure the real limits; those samples are committed as fixtures.

## 9. Not in scope / decided against
- Token handling: the tool uses the token cargo already finds; it does not read, store or print one. (The portfolio is looking at Trusted Publishing; the tool must not stand in its way.)
- Registries other than crates.io.
- Version bumping and changelog generation (the PM allocates versions); the tool refuses a set whose versions it did not receive from the manifests.
- A name-ownership pre-flight (SURVEY Q5) is **in** as `plan --check-names` (API owners lookup), default off, because the failure it prevents (fuel-core, fuel-compression) is real but costs a network call per crate.

## 10. Decisions on the review items (lane, 2026-10-24; the PM delegated them with "defaults conservative")

1. **`plan --exact`**: kept, opt-in, not built in R2 (needs the `Cargo` trait, R3). The default edge rule (section 0) is implemented and tested on fixtures plus one real `cargo metadata` parse. If the exact mode ever disagrees with the rule, the exact result wins and the disagreement is a finding.
2. **Yank reasons**: the five-way list in section 7 stands (`security`, `broken-build`, `wrong-contents`, `licence`, `other` with a longer text). It is a field for auditing, not a gate on the mechanism.
3. **Ledger location**: beside the workspace, `.releaseant/ledger.jsonl`, outside the `git archive` directory. Not committed by default (it holds SHAs and timestamps only; committing it is the user's choice).
4. **`--max-wait`**: default 1 hour of total waiting per run (`Options::max_wait`). A longer window is a deliberate flag, because a stopped run is resumable and a run that waits forever is not visible. The first-publish pace the fuel wave doc quotes (about 7 hours) needs `--max-wait` raised or several resumed runs; R4 measures the real pace.

## 11. Status of the test plan (R3b, 0.0.6)

All of T1-T17 now exist, each with its negative control, except as noted:
- T10 (`--no-verify-crate NAME --reason TEXT`, repeatable pairs): `pipeline::publish` refuses a blank reason or an unknown crate, writes an `exception` event, skips only that crate's pre-upload build, and after upload runs a consumer check (`cargo add NAME@=VERSION` into a fresh project, then `cargo check`); a failing check makes the publish unsuccessful.
- T15 (`releaseant yank NAME VERSION --reason KIND:TEXT --ledger FILE [--execute]`): refuses a bad reason, a version the ledger has no `visible` event for, and anything without `--execute`; with it, the `yanked` event is on disk before `cargo yank` runs. `pipeline::publish` has no path to `yank`.
- Real index lookups are tested against canned HTTP responses (statuses, a 503, garbage, a closed port) and, with network, against the real index.

Mutation checks by hand (not in CI) in R3b: yank event not written first, yank without a ledger record, negative retry-after not clamped, yanked version read as present, 5xx read as absent, verification failures ignored, blank exception reason accepted, exception not consumer-checked, and packaging the plan one crate at a time instead of as a set; all made a named test fail (the first attempt at the first one did not compile and was redone).

Findings from R3b that change the design:
- **Package the plan in ONE cargo call.** With cargo 1.99.1, `cargo package -p a -p b` succeeds for a `b` whose versioned dependency `a` is unpublished (cargo overlays the set it is packaging), while `cargo package -p b` alone fails. Section 0's table therefore describes what *publishing* one crate at a time needs on the index, not what packaging a set needs. `Cargo::package_set` and `tests/package_table.rs` pin both.
- **TLS and licences**: `ureq`'s default `rustls` brings `webpki-roots` (CDLA-Permissive-2.0), which `deny.toml` does not allow; the HTTP client uses the platform TLS stack instead (`native-tls`; on Linux this needs the OpenSSL development package, present on GitHub's runners). The deny policy was not changed.
- A dry run still archives, packages and builds (those are local) and only asks the index questions.

Not done: `plan --exact` and the name-ownership check. (The external-prerequisite check was added in 0.0.8, below.) The CLI's `verified` ledger events are written before the run's `planned` event; harmless, noted.

## 12. External prerequisites and R4 (0.0.8)

- **R4 passed** (PM run 2026-10-24, see `docs/REHEARSAL.md`): three placeholder crates, interrupted and resumed, no 429 occurred. Still unmeasured: the real rate numbers, the wording of the "new crates" limit, a kill between upload and read-back, a lagging index.
- **External prerequisites** (`externals::missing_externals`, run by `publish` before anything is written, and by `plan --check-externals`): every dependency that names a package outside the workspace and carries a version (normal, build, optional or dev) needs a non-yanked registry version that matches the requirement under cargo's prerelease rules, or the dependent cannot be packaged or uploaded. The registry being unreachable is an error, never "missing". Found by running `plan` on baracuda: its `baracuda-cuda-seam` needs `fuel-kernel-seam 0.23.0`, which the PM had excluded by hand while it was unpublished. Measured 2026-10-24 on the current `origin/main` of both repos (baracuda `3d97927a`, fuel `b5420f18`): `plan` lists 72 crates for baracuda (the PM's run used 71, without `baracuda-cuda-seam`) and 43 for fuel (the PM's count); `plan --check-externals` finds nothing missing in either (13 s and 19 s), because `fuel-kernel-seam 0.23.0` and `0.24.0` are on the index now. Positive control for the check: the unit and pipeline tests that report a missing crate, a version mismatch and a yanked-only crate; a real-index control that `versions` answers for `semver` and returns an empty list for a nonexistent name.

## 13. Progress lines (R6, design; PM first-hand 2026-10-30: a real publish was silent for about 20 minutes)

- **What**: while `publish` runs, one line per step on **stderr** (stdout stays the final summary), each prefixed with elapsed time since the command started: `[03:12] packaging 13 crates`, `[04:40] verified foo 0.1.0 (3 of 13)`, `[06:02] uploading foo 0.1.0`, `[06:19] uploaded foo 0.1.0 (17 s)`, `[06:31] read back foo 0.1.0: visible`, `[07:00] waiting 60 s (rate limit)`, `[07:00] skipped bar 0.1.0 (dependency foo failed)`. Always on, no flag: a quiet run is the defect.
- **Where**: `PublishArgs.on_progress: Option<ProgressSink>` (same shape as `on_warnings`). Run-stage lines are a mirror of the ledger: `Recorder::log` and `pipeline::record` hand every event they append to the sink after the append succeeds, so the lines cannot disagree with the ledger and no step is hand-instrumented twice. Package and verify have no ledger event until the stage ends, so `package_all` announces the set before its single cargo call and `verify_set` gets a per-crate callback.
- **Elapsed time** comes from `std::time::Instant` held by the CLI closure, not from `Clock` (tests use a fake clock; elapsed is presentation, never recorded). Per-step durations (`uploaded ... (17 s)`) are the difference between the paired events, kept by the same closure.
- **Not changed**: ledger format, exit codes, stdout. A sink that panics is not caught; the sink only prints.
- **Tests**: a pipeline test with a recording sink asserts the lines in order for a three-crate chain and that every ledger event of the run produced exactly one line (count compared by event kind, not a bare number); a negative control with the sink absent runs unchanged; a CLI test asserts progress is on stderr and absent from stdout.
- **Not row 1**: `--only` / registry state in `plan` (row 2) and per-release tag naming (row 3) are separate designs.

## 14. Partial publish and registry state in `plan` (R6 row 2, design; PM first-hand 2026-10-30: Vulkane wanted one brand-new crate of five and fell back to plain `cargo publish -p`)

- **`--only NAME` (repeatable) on `plan` and `publish`**: the run covers exactly the named crates. It is `--exclude` for every other crate of the plan, so it reuses everything that exists: the other crates leave the plan before packaging, verification and the ledger; each is recorded as an exception event with the reason `not selected by --only`; and a named crate that depends on a left-out crate needs that dependency on the registry already, or the run stops before anything is written (the existing `MissingExternals` check, section 12). A name not in the plan is an error; `--only` together with `--exclude` is an error (one way to say it). No `--exclude-reason` is needed: the reason is the flag.
- **A partial run is never tagged**: `any_excluded` is already true, so `--tag` prints the existing "not tagged: crates were excluded" line. How a partial release gets a tag is row 3, not decided here.
- **`plan --registry`**: asks the index for each crate of the (possibly reduced) plan and prints the state after the version: `foo 0.1.0 (on registry)`, `foo 0.1.0 (not on registry)`, `foo 0.1.0 (registry unknown: <why>)`. A failed lookup is never read as "not on registry" (same rule as `publish`). Needs network, so it is a flag; the default `plan` output is unchanged (the README test pins it). It reuses the `IndexRegistry` that `--check-externals` already builds.
- **Not in scope**: choosing the set by registry state (`--only-unpublished`): `publish` already skips what is on the registry and resumes, so the flag would only restate it; say so if a real run wants it.
- **Tests**: `--only` leaves exactly the named crates in the plan and the ledger holds one exclusion event per other crate with the flag's reason; a dependent named without its dependency on the registry stops with the missing-external error and no event beyond the exclusions is written (positive control: the same selection with the dependency present publishes); unknown name and `--only` with `--exclude` are errors; `plan --registry` against a fake index prints all three states (a lookup that fails prints unknown, not absent).
