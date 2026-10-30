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
