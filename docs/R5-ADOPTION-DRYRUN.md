# R5 step 1: adoption dry run against baracuda and fuel

Status: evidence for the PM (ROADMAP R5, `[TASK]` of 2026-10-30). **Read-only on baracuda and fuel; nothing was
published, tagged, yanked or edited anywhere.** Written by the releaseant lane. Every number names its ref.

## What was run

| | baracuda | fuel |
|---|---|---|
| Ref | `ee1e7ce3fd5b9aef7dc6d5691eefd72801b54b45` | `e3ff342702393f94801cf05f6486d7f66145936f` |
| What that ref was | `origin/main` at 2026-10-30 ~11:30 UTC | `origin/main` at 2026-10-30 ~11:30 UTC |
| `origin/main` at 19:46 UTC (after the run) | `d6f9cc751027b1c9692cbab04e43217619817b1b` | `c3150e5962a31f96fa927aec2d10359e867e829a` |
| Crates in the plan | **73** | **43** |
| `plan` | exit 0 | exit 0 |
| `plan --check-externals` | exit 0, same 73 | exit 0, same 43 |
| `verify` | exit 0, **73 of 73 `verified`**, 6 min (19:39:36 to 19:45:42 UTC), run with `DOCS_RS=1` | exit 0, **43 of 43 `verified`**, 3 h 18 min (16:21:32 to 19:39:36 UTC) |
| `readback` | exit 1: 73 ABSENT, 0 present | exit 1: 40 ABSENT, 3 present |

Tool: `releaseant 0.0.21` (`target/release/releaseant.exe` of the bin worktree), `CARGO_BUILD_JOBS=2`, one build at a
time on a box that other lanes were also loading (so the durations are not a measure of the tool). Source for every
command was a `git archive` of the SHA above; no working tree was read. Both refs have moved since: this note is
about those two SHAs, not about today's `main`.

**The task text says baracuda has 10 crates. At this ref it has 73** (77 workspace members, 4 `publish = false`:
`baracuda-bench-stats`, `baracuda-examples`, `baracuda-kernels-bench`, `xtask`). `docs/ADOPTION.md` already said 73.
Fuel has 45 members, 2 `publish = false` (`fuel-model-fixture`, `fuel-model-umbrella-fixture`), 43 planned.

## Compared with what the hand-written loops do

The loops were read at: baracuda `scripts/publish.ps1` at `ee1e7ce` (git show); fuel has **no** publish script in its
repository at `e3ff342` (only the two CI guards), its loop is the PM's `%TEMP%\fu-pub.py` (and `bk3-pub.py`), read
today as files on this box, not at a ref. The set and order were recomputed from `cargo metadata --no-deps` of the same
archives with a script that mirrors each loop's own rule (`publish != []`; `fu-pub.py`: normal and build dependencies
plus dev-dependencies that have no `path`; `publish.ps1`: whatever `cargo publish` accepts, simulated below). My
edge rule for the plan comparison is: every dependency on a set member counts except a dev-dependency that is
path-only. That is my reading of DESIGN section 0, not the tool's code, and it matches the plan's output (below).

### Disagreements, baracuda

1. **`publish.ps1` would refuse this ref; ReleaseAnt plans and verifies it.** `publish.ps1` stops
   ("`MAJOR.MINOR must match`") when `baracuda-cuda-emit` or `-parse` has a `MAJOR.MINOR` different from its `unpopped`
   requirement. At `ee1e7ce` both crates are `0.32.0` and require `unpopped ^0.34.3`, so `0.32` vs `0.34`: the script
   throws before uploading anything. ReleaseAnt does not have this rule (ADOPTION.md already lists it as a baracuda
   rule, to stay as a separate preflight), so `plan`, `plan --check-externals` and `verify` all pass on a set that the
   baracuda script rejects. **Not decided here which is right**: either that ref is mid-change in baracuda or the rule
   caught a real mismatch. Whoever adopts needs `scripts/preflight.ps1` to keep this check, and this ref is a live
   example of it firing.
2. **The licence guard** (`scripts/check-crate-licences.py`, run first by `publish.ps1`) was **not** run: it calls
   `git rev-parse --show-toplevel` and the archive extraction is not a repository (it exited with `CalledProcessError`
   from that call, which says nothing about licences). ReleaseAnt has no such check either; same conclusion as 1.
3. **Order.** `publish.ps1` has no order: it walks `cargo metadata` order and defers any crate whose dependency is not
   on the index. Simulated with the plan's edge rule (each upload visible to the next; no index delay), it needs
   **7 passes** (cap in the script: 12), deferring 66 crates after pass 1; 36 crates come before something they depend
   on in metadata order. The plan orders them dependency-first in one pass. The two lists are different (metadata
   order is not the plan order); both are valid by the edge rule. Real index propagation was not modelled, so the pass
   count is a lower bound for the script.
4. **Sets are identical:** 73 = 73, nothing in one and not the other, no crate planned at a version other than its own
   (`baracuda-cuda-parse` and `-emit` plan at `0.32.0`, the other 71 at `0.1.0-alpha.56`).
5. **Verification.** `publish.ps1` uploads with `--no-verify`; ReleaseAnt builds every extracted `.crate` first (73 of
   73 passed, with `DOCS_RS=1`). I did not run `verify` without `DOCS_RS=1`, so I cannot say what the CUDA crates do
   without it. The 6 minutes against fuel's 3 h 18 min is an observation, not an explanation; I did not investigate
   why.
6. **`baracuda-cuda-seam` is in the plan and in the script's set.** ADOPTION.md recorded the PM's rule to keep it out
   until fuel 0.24 is consumed (`--exclude baracuda-cuda-seam --exclude-reason ...`); `publish.ps1` has no exclusion.
   Not rechecked whether the rule still stands.
7. **Two version streams.** One run plans both (`0.32.0` for emit/parse, `0.1.0-alpha.56` for the rest). `publish
   --tag` needs `--tag-version`; not exercised here (dry-run `publish` was not run).
8. **Quality warnings** (stderr, exit 0; identical text from `plan` and `verify`, 80 lines): `rust-version` missing on
   67 of 73; no README on 5 (`baracuda-cuda-emit`, `-cuda-parse`, `-cuda-seam`, `-cuda-vocab`, `-seam`); no
   `keywords` and no `categories` on 4 (`-cuda-emit`, `-cuda-parse`, `-cuda-seam`, `-cuda-vocab`). No ERRORS (no `description` or `license` problem, no invalid keyword or category).
   Neither repo has a `releaseant.toml` at the ref, so nothing is enforced; `--require-metadata` was not used.

### Disagreements, fuel

1. **Set:** 43 = 43, identical to `fu-pub.py`'s (`publish != []`), same versions (all `0.31.0`).
2. **Order:** `fu-pub.py`'s depth-first order over sorted names is **not** the plan's order (first difference at
   position 0: `fuel-ir` against `fuel-conv`). Checked both ways with the edge rule above: the DFS order has **0**
   dependencies after their dependent, and the plan order has **0**; `fu-pub.py` sees every edge the plan does (0
   edges in the plan that the loop ignores). So the difference is tie-breaking, not a missing edge. This is the same
   kind of difference the PersistAnt rehearsal showed (ADOPTION.md).
3. **Already published:** `fuel-kernel-seam`, `fuel-kernel-seam-announce` and `fuel-kernel-seam-types` are **present**
   at `0.31.0` on the registry (read-back printed their SHA-256); the other 40 are absent. `fu-pub.py` would skip those
   three through its API lookup; ReleaseAnt skips them through the index and compares the checksum. Who published them
   is not established here. A real run would upload 40, not 43.
4. **Loop behaviours ReleaseAnt does not copy:** `fu-pub.py` uses `--locked --no-verify`, waits a fixed 90 s on any
   text matching `429`, `Too Many` or `rate`, treats any API error as "not published", and exits on the first other
   failure. ReleaseAnt waits for the server's time within `--max-wait` and stops resumable; a failed lookup stops the
   run (ADOPTION.md table). ReleaseAnt's upload passes `--locked` when the repository has a `Cargo.lock` (`src/crates_io.rs`, `upload`).
5. **Quality warnings** (84 lines): no README on 35 of 43, `rust-version` missing on 43 of 43, no `keywords` and no
   `categories` on 3 (`fuel-kiss-ref-backend`, `fuel-model-core`, `fuel`); no ERRORS. Same remark about `releaseant.toml`.
6. **Repository's own guards stay:** `scripts/check-dev-dep-publish-cycles.py` and `scripts/verify-packaged-crates.py`
   are CI guards, not publish tools; not run here.

## Not done

- `releaseant publish` as a dry run (adds the index lookups per crate, the tag check and the ledger) on either set.
- `verify` for baracuda without `DOCS_RS=1`.
- The licence guard, the dev-dependency cycle guard and the packaged-crate scan.
- A comparison with `cargo publish --workspace`.
- Anything against a ref newer than the two above.

## Where the evidence is

Outputs (disposable): `%TEMP%\r5\{b,f}-plan.{out,err}`, `{b,f}-verify.{out,err,rc}`, `{baracuda,fuel}-ext.out`,
`{baracuda,fuel}-rb.out`, metadata JSON and the comparison script in the lane scratchpad.

## Suggested next steps (for the PM to accept or drop)

1. Decide the emit/parse rule at `ee1e7ce` (disagreement 1) before baracuda adopts.
2. Adopting fuel needs no preflight script; its two CI guards stay. Run the dry-run `publish` at the SHA the PM picks.
3. For both, commit a `releaseant.toml` with the required fields the project wants before the first real run.

## Update 2026-10-31: the seam trio, held, and the ledger line that clears it

`[TASK]` of 2026-10-31 (PM). **Dry runs only; nothing was uploaded, tagged or yanked.** Tool: `releaseant 0.0.35` built
from `3f28e82` (release profile), run with `--repo C:/Projects/<name> --sha <SHA>` and `--ledger` in a scratch
directory (no file in either project was written). Refs: fuel `origin/main` `59bcbd3d4c99cbc8c8ccdab4db3f5d881c65baee`
(fetched first; equals the SHA in the task), baracuda `origin/main` `8f9b30139bc942ed47bcadc8ac892d64249ac55a`.

| | fuel `59bcbd3d` | baracuda `8f9b3013` |
|---|---|---|
| `plan` | exit 0, **43** crates, all `0.32.0` | exit 0, **73** crates: 71 at `0.1.0-alpha.66`, `baracuda-cuda-emit` and `-cuda-parse` at `0.34.11` |
| `plan --check-externals` | exit 0, same 43 plus the line "every dependency outside the set is on the registry" | exit 0, same 73 plus that line |
| `readback` | 3 present (the seam trio), 40 absent | 0 present |
| errors | none (84 and 67 quality-warning lines on stderr, same kinds as above) | none |

The task text says baracuda has 10 crates; at this ref the plan has 73 (as before).

### 1. `publish --only` the trio: Held, as #34 predicts

`publish --only fuel-kernel-seam-types --only fuel-kernel-seam-announce --only fuel-kernel-seam` (dry run, empty
ledger): exit 1, and the report says

```
fuel-kernel-seam-announce: HELD: on the registry, but not uploaded by this tool from this commit
fuel-kernel-seam-types: not attempted
fuel-kernel-seam: not attempted
run stopped (resumable): fuel-kernel-seam-announce@0.32.0 is already on the registry and the ledger has no upload of it
from commit 59bcbd3d...; ask the PM before going on
```

The stop is at the first crate of the plan order; the other two are only "not attempted", so each of the three needs its own
ledger line. The 40 crates not selected are recorded as `not selected by --only`. All three were verified (packaged and built
from the extracted `.crate`) before the stop.

### 2. Is the registry's copy ours? Checked, all three equal

The registry's index checksums (`readback`) against `sha256sum` of `cargo package --no-verify` run on a `git archive` of
`59bcbd3d` (same machine as the hand publish; cross-machine reproducibility is not measured):

| crate (0.32.0) | registry (index) | local `.crate` of `59bcbd3d` |
|---|---|---|
| fuel-kernel-seam-types | `49546dde16b81004d8347e724f326e5426c9036f81aa09c20398c62dac3c291c` | same |
| fuel-kernel-seam-announce | `98217ff5bbe53752e4355ecd3df2076538a38234458e5d78f744559119794265` | same |
| fuel-kernel-seam | `c3d437a5a35c1358d40fbf0dca03483acea021ee65de2c71db21391d88b76c79` | same |

An equal checksum means the registry holds the bytes `cargo package` makes of this commit; it does not say who uploaded
them. (The PM said he hand-published them; that is the PM's statement, not read from the registry here.)

### 3. The exact PM ledger line

One line per crate, appended to the ledger the real run will use (`.releaseant/ledger.jsonl` of the repository, or the
`--ledger` file), by the PM only, after the comparison above is repeated by whoever adds it. `<UTC>` is the time of adding,
`<who>` the person:

```
{"ts":"<UTC>","run":"manual","sha":"59bcbd3d4c99cbc8c8ccdab4db3f5d881c65baee","crate":"fuel-kernel-seam-types","version":"0.32.0","event":"visible","detail":"published by hand; checked by <who>: registry index checksum 49546dde16b8... equals sha256 of cargo package of this commit"}
{"ts":"<UTC>","run":"manual","sha":"59bcbd3d4c99cbc8c8ccdab4db3f5d881c65baee","crate":"fuel-kernel-seam-announce","version":"0.32.0","event":"visible","detail":"published by hand; checked by <who>: registry index checksum 98217ff5bbe5... equals sha256 of cargo package of this commit"}
{"ts":"<UTC>","run":"manual","sha":"59bcbd3d4c99cbc8c8ccdab4db3f5d881c65baee","crate":"fuel-kernel-seam","version":"0.32.0","event":"visible","detail":"published by hand; checked by <who>: registry index checksum c3d437a5a35c... equals sha256 of cargo package of this commit"}
```

The `sha` is the commit being published: a line naming another commit does not clear a run at this one (unit test
`a_ledger_from_another_commit_does_not_vouch_for_a_held_version`, `src/run.rs`).

### 4. The run clearing it, on a COPY of the ledger

The copy was the ledger of run 1 plus the three lines above (with `checked by releaseant lane (rehearsal copy)`); the real
ledger was never touched. Same command, dry run: **exit 0**, `run finished: complete`, and

```
fuel-kernel-seam-announce: already published
fuel-kernel-seam-types: already published
fuel-kernel-seam: already published
dry run: nothing was uploaded (use --execute)
```

Each was `classified: already-published`. With the lines absent (run 1) the same command stopped at HELD, so the lines are
what changed the outcome.

### 5. Plan order against the `cargo publish` order that worked today

| | order |
|---|---|
| plan (full plan, and the `planned` event of the `--only` run) | announce, types, seam |
| PM's `cargo publish` today | types, announce, seam |

`fuel-kernel-seam` depends on `fuel-kernel-seam-types` (`workspace = true`, `Cargo.toml` line 19); neither `-types` nor
`-announce` lists the other or `fuel-kernel-seam` among its dependencies (read from the archive). So the only edge is
types before seam, both orders satisfy it, and the difference between them is tie-breaking between two independent
crates, as with `fu-pub.py` above. No disagreement about a dependency.

### Not done in this update

- `publish` dry run over all 43 or all 73 crates (builds every crate: hours for fuel last time); only `plan`, `readback`
  and the three-crate `--only` runs were done.
- A cross-machine comparison of packaged bytes, and any check of who uploaded the trio.
- The baracuda rules from "Disagreements, baracuda" (preflight, licence guard) were not rerun.

## Update 2026-10-31 (2): `baracuda-cuda-seam` and the exclusion rule (R5 item 6)

`[TASK]` of 2026-10-31 (PM): options, with the 73-crate plan; no release. Dry runs of `plan` only, `releaseant 0.0.35`,
baracuda `origin/main` `8f9b30139bc942ed47bcadc8ac892d64249ac55a`, registry read 2026-10-31 ~03:00 UTC. Nothing was
uploaded, tagged or edited.

### What the rule says, and what is read now

ADOPTION.md (section 3) records the PM's rule of 2026-10-24: `baracuda-cuda-seam` stays out until fuel 0.24 is
consumed, as `--exclude baracuda-cuda-seam --exclude-reason "waits for the fuel release"`. The rule's text was not
re-asked here. What the ref and the registry show:

1. **Position in the plan:** 73 crates; `baracuda-cuda-emit 0.34.11` is line 57 and `baracuda-cuda-seam
   0.1.0-alpha.66` line 58, so the seam crate goes up immediately after emit. `plan` and `plan --check-externals`
   exit 0 (the latter: "every dependency outside the set is on the registry"). The 72-crate plan
   (`plan --exclude baracuda-cuda-seam --check-externals`) is the same list minus line 58, exit 0.
2. **Nothing in the set needs it:** no other member's `Cargo.toml` names `baracuda-cuda-seam` (only the workspace table
   and its own); `baracuda-cuda-emit`'s mention is in its `description` text. Excluding it strands no crate.
3. **What it needs:** `baracuda-cuda-emit` (in the set) and `fuel-kernel-seam`, `fuel-kernel-seam-types` `^0.31.0`
   (caret, registry versions). Both fuel crates are on the registry at `0.31.0` and `0.32.0`
   (index, read today); the registry has `baracuda-cuda-seam` up to `0.1.0-alpha.25` only. `plan --only
   baracuda-cuda-seam --check-externals` alone reports `MISSING EXTERNAL: baracuda-cuda-emit ^0.34.11` (registry
   stops at `0.24.0`), so it can only go in a run that includes emit.
4. **Who consumes it:** fuel `59bcbd3d` takes `baracuda-cuda-seam = { version = "=0.1.0-alpha.15" }` (workspace
   `Cargo.toml` line 327; `fuel-dispatch`, optional feature `jit-synth`), an exact pin, so publishing `alpha.66`
   changes nothing in fuel's lockfile or build.
5. **A mismatch the rule's wording does not cover:** `alpha.66` requires `fuel-kernel-seam{,-types} ^0.31.0`. Fuel's
   `main` is `0.32.0` (all three seam crates now on the registry). A caret `^0.31.0` does not match `0.32.0`, so a graph
   holding fuel `0.32.0` and `baracuda-cuda-seam alpha.66` has two copies of `fuel-kernel-seam-types`; baracuda's own
   comment in `crates/baracuda-cuda-seam/Cargo.toml` says that case is a type error (E0308). Not built here; that is
   baracuda's comment about its own graph, read, not reproduced.
6. **Tagging:** a run with `--exclude` is never tagged by `publish --tag`. If the 72-crate run is the one that
   publishes the set, the project `v<version>` is not made by it.

### Options

| | what happens | evidence | cost / risk |
|---|---|---|---|
| **A. Drop the exclusion; publish all 73** | one run, cuda-seam right after emit; tag possible (`--tag-version` still needed: two streams) | points 1 to 3 | publishes `alpha.66` requiring fuel `^0.31.0`: fuel consumers on `0.32.0` get the two-copies mismatch of point 5; yanking does not unpublish |
| **B. Keep the exclusion; publish 72** | exactly the rule as recorded; plan exit 0 | points 1, 2 | the seam crate stays at `alpha.25` on the registry; no project tag from that run (point 6); the rule's trigger ("fuel 0.24 consumed") is a version fuel passed long ago, so the trigger needs restating |
| **C. Baracuda moves cuda-seam to fuel `^0.32.0` first (its own PR), then drop the exclusion** | one 73-crate run of the new ref, seam crate aligned with fuel `main` | points 1 to 5; not tried: needs a baracuda change and its own `plan` at the new ref | one more baracuda PR before the release; the fuel `0.32.0` seam trio is on the registry now, so `--check-externals` would pass |

**Recommendation: C** if the aim is that baracuda's published seam crate works with fuel `main`; **B** if the aim is
only to do exactly what was recorded and decide later. A is the one that can publish something that does not fit the
current fuel. This is the lane's reading of the files above, not a PM decision.

### Not done

- No `publish` dry run (builds all crates); no build of a graph with fuel `0.32.0` and `alpha.66` to see the E0308.
- The rule was not re-asked of CireSnave or the baracuda lane; whether `baracuda` has already planned C is not known here.
