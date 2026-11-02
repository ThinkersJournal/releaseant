# R5: adopting ReleaseAnt (design note and dry-run evidence)

Status: proposal for the PM. **No file in any other repository is edited by this note or by the lane.** Every
claim names the file and ref it was read at. Nothing was published, yanked or tagged to produce this note.

## What exists today

| Where | What | Ref |
|---|---|---|
| PM `tools/workspace-publish.py` | publishes a workspace: lists members with `cargo metadata`, asks the crates.io API which already have the version, runs `cargo publish --workspace --allow-dirty` in a clean `git archive`, waits out a 429 until its "try again after" time, reads every crate back (exists, not yanked) | read 2026-10-29 |
| PM `tools/tag-release.sh` | tags the release (replaced by `releaseant tag`, 0.0.14) | read 2026-10-29 |
| PM `%TEMP%\fu-pub.py`, `fz-pub.py`, `bk2-pub.py` | the older per-project loops (DFS order, fixed 90 s on 429, `--no-verify`) | read 2026-10-23, `docs/SURVEY.md` |
| baracuda `scripts/publish.ps1` | the retry-pass loop, plus two project-specific preflights (licence files identical; `MAJOR.MINOR` of the `baracuda-cuda-emit/-parse` crates equals their `unpopped` requirement) | baracuda `origin/main` `3d97927a`, read 2026-10-23 |
| fuel `scripts/check-dev-dep-publish-cycles.py`, `scripts/verify-packaged-crates.py` | CI guards, not publish tools | fuel `origin/main` `5aaad145`, read 2026-10-23 |

## What each does that ReleaseAnt does, does differently, or does not do

| Behaviour | `workspace-publish.py` | baracuda `publish.ps1` | ReleaseAnt 0.0.14 |
|---|---|---|---|
| Publish set from `cargo metadata`, `publish = false` out | yes | yes | yes |
| Order | cargo's own `--workspace` order | retry passes, up to 12 | Kahn over the edge rule, a cycle is an error naming its members |
| Members at a version other than the target | left out | n/a | planned at their own versions (baracuda's two streams in one run) |
| Skip what is published | API, **any error = not published** | regex on cargo text | index lookup, **a failed lookup stops the run** |
| 429 | waits until the server's time (min 30 s) | 65 s or advised | the server's time, within `--max-wait`, then stops resumable |
| Read-back | exists and not yanked | none | index poll plus checksum compared with what was uploaded |
| Record | stdout | text log | append-only ledger, event written before the action |
| Verification before upload | none | none (`--no-verify`) | each `.crate` extracted and built; named exceptions, consumer-checked after |
| Source | clean archive, `--allow-dirty` | working tree, `--allow-dirty` | `git archive` of a full SHA |
| Dependency outside the set not on the registry | the PM excludes by hand | same | refused before anything is written |
| Leave a crate out | `--exclude` | none | `--exclude NAME --exclude-reason` (0.0.9), reason in the ledger |
| Tag the release | separate script | none | `releaseant tag`, `publish --tag` (0.0.14) |
| Licence files identical per crate | no | **yes** | **no** (a baracuda rule) |
| `MAJOR.MINOR` of the emit/parse crates equals `unpopped` | no | **yes** | **no** (a baracuda rule) |
| Publish metadata checked before upload | no | no | **yes** (0.0.16): errors for what crates.io refuses (`description`, `license`, bad keywords or categories), warnings for README, keywords, categories, `rust-version`, `repository`; `--require-metadata` makes chosen warnings errors; see open item 1 |
| Dry run | no | `-DryRun` | default |

## Dry-run evidence: Unpopped and PersistAnt

Run 2026-10-29 with `releaseant 0.0.14` (a debug build of `eb86c73`), read-only: `releaseant plan` and
`releaseant readback` against each repository's own `git archive`, and the crates.io API for publication times.
The tagged releases are the ones the script published.

| | tag | tag commit | `origin/main` now |
|---|---|---|---|
| Unpopped | `v0.32.1` | `b2360f68e0812ac097a9b71f74502b0a31e41257` | `4855daaae34ca5353ebfdc0aaa36a0a92e404af4` (0.32.27) |
| PersistAnt | `v0.12.3` | `b629c5c11ccf883aa121c9a3ea6dbf9031f446ae` | `cecab52ef2d18f38956c80ef4d76f16e35ac2b13` (0.12.5) |

**The plan at the tag and at current `main` is the same set in the same order** for both (exit 0, empty stderr):

- Unpopped, 4 crates: `unpopped-vocab`, `unpopped`, `unpopped-cpu-c`, `unpopped-slang`.
- PersistAnt, 9 crates: `persistant-core`, `persistant-blob`, `persistant-macros`, `persistant-relational`,
  `persistant-kv`, `persistant-opendal`, `persistant-sqlx`, `persistant`, `persistant-migrate`.

**The order the script actually published in** (the `created_at` crates.io holds for each version):

- Unpopped: identical to the plan, 4 of 4, published within 10 seconds (04:21:53 to 04:22:03 UTC).
- PersistAnt: the plan with one swap. The script published `persistant-core`, `persistant-macros`,
  `persistant-blob`, ...; the plan has `blob` before `macros`. They are independent: `persistant-macros` depends
  on no member of the set, `persistant-blob` depends only on `persistant-core`, so either order is valid and
  the plan breaks the tie by name. Checked independently: for **both** the plan's order and the order crates.io
  shows, a script over `cargo metadata` of the tagged archive finds **0** versioned dependencies (any kind;
  path-only dev-dependencies are not edges, DESIGN section 0) that come after their dependent. All 9 were
  published within 10 seconds (13:14:20 to 13:14:29 UTC). I see no rate-limit wait in these timestamps; a
  refused upload leaves no timestamp, so rounds that failed are invisible here.

**Read-back against the real registry** (`releaseant readback --sha <tag commit>`): all 4 Unpopped and all 9
PersistAnt crates are present at the tagged version, with their SHA-256; the first 12 hex digits of each match
the checksum the crates.io API returns. This exercises the read-back path against 13 real crates; it checks
presence and the index's checksum, not that the tag's archive rebuilds to the same bytes (not claimed).

**Not done:** `releaseant publish` as a dry run on these sets (it builds every crate: PersistAnt has 9 including
the sqlx one, and builds are one at a time on a loaded box); a comparison with what `cargo publish --workspace`
would order today (needs packaging); a real publish. A dry-run `publish` on the first real target (below) is the
cheap form of the missing step.

## Adoption plan

**1. First real use (PM): a single-crate Ant, `releaseant publish --execute --tag`**, which closes the one
untested path (the tag after a real upload). Suggested sequence, all but the last step dry:
1. `releaseant plan --sha $SHA` (one line) and `releaseant publish --sha $SHA --tag`: a dry run that builds the
   crate, asks the index, and checks the tag could be made ("would tag v...").
2. For an Ant we ship now, make the quality fields errors: commit a `releaseant.toml` at the repository root
   (`[metadata]`, `require = ["keywords", "rust-version", "readme"]`; read from the commit, 0.0.18) so every
   publish of that line enforces it, or pass `--require-metadata keywords,rust-version,readme` on `plan` and
   `publish` (the flag adds to the file). Without either those are warnings; `description` and `license` are
   errors always (0.0.16).
3. `releaseant publish --sha $SHA --execute --tag --tag-name <project>`.
4. After: `releaseant readback --sha $SHA`; `git ls-remote --tags origin v<ver>^{}` equals `$SHA`; the ledger
   (`.releaseant/ledger.jsonl`) ends with a `tagged` event. If the publish succeeded but the tag did not, the
   output says `PUBLISHED but NOT TAGGED` and the command to finish; running the same command again is safe.

**2. Unpopped and PersistAnt next** (one version, 4 and 9 crates): the publish becomes
`releaseant publish --repo C:\Projects\<name> --sha <merge sha> --execute --tag`; the script and the tag script are
then not needed for them. Same rate-limit wait budget as before (`--max-wait`).

**3. baracuda** (73 crates at the last look, two version streams, an exception).
- Keep as a separate preflight run before ReleaseAnt: the licence guard and the emit/parse `MAJOR.MINOR` check
  from `publish.ps1` (baracuda rules, not publishing mechanics). Proposed `scripts/preflight.ps1` in baracuda.
- `baracuda-cuda-seam` is in the set (PM, 2026-10-31, replacing the 2026-10-24 rule that kept it out until fuel 0.24 was consumed):
  no `--exclude`. At baracuda `ed1699af`: `plan --check-externals` = 73 crates, exit 0, the seam crate at line 58 right after
  `baracuda-cuda-emit`, requiring `fuel-kernel-seam{,-types}` `0.32.0` (docs/R5-ADOPTION-DRYRUN.md, update (2)). An `--exclude` run
  is never tagged.
- Two streams (`0.32.0` and `0.1.0-alpha.44` then): one run plans both. **Which one the tag names is the PM's
  rule**; the tool asks for `--tag-version` and never guesses (the PM's practice so far: the alpha stream).
- CUDA crates: `DOCS_RS=1` in the shell, as `bk2-pub.py` does, or `--no-verify-crate NAME --reason "..."`.
- After one clean real run the baracuda lane deletes `scripts/publish.ps1` (its PR, not this note).

**4. fuel** (43 crates at one version, `0.30.1` at `6c1b56c`): `releaseant publish --repo C:\Projects\fuel --sha
<sha> --execute --tag --max-wait <seconds>`. Keep both CI guards (dev-dep cycles; packaged-crate scan). A first
publish of a wave is paced by crates.io's new-crate limit, which `docs/release-0.13.0-wave.md` puts at about 7
hours for about 45 new crates; ReleaseAnt stops resumable when its wait budget is spent, so run it again. The
limit itself was not reached in the R4 rehearsal and is still unmeasured here.

## Replacing `publish-loop.sh` and `publish-when-green.sh` (0.0.49, slice 2 of R5)

Both PM scripts (`C:\Projects\tools\publish-loop.sh`, `C:\Projects\.lane-state\publish-when-green.sh`) are one tool
call now. Measured live against the rehearsal crates: `docs/R5-LIVE.md`. Proposal only: those scripts belong to the PM
and this repository does not edit them.

| the script did | releaseant does | differences to know |
|---|---|---|
| `publish-loop.sh <repo> <sha> <ver> <crate>...`: run `publish --execute` until the registry shows every listed crate at `<ver>` (`ATTEMPTS`, default 20; `PAUSE`, default 120 s) | `releaseant publish --repo <repo> --sha <sha> --execute --retries 19 --retry-pause 120` | The crate list and version come from the plan at `<sha>`; to publish a subset use `--only a --only b`. Confirmation reads the sparse index (not the crates.io web API), asks every crate of the run's plan, and a yanked version or a failed lookup is "not there". A version someone else put on the registry (`HELD`) or a checksum mismatch stops the loop at once instead of looping to the attempt limit. Exit 0 only when confirmed |
| then tag with `tools/tag-release.sh` | add `--tag` (and `--tag-name <project>`) | The tag is made only after the run is complete, the same ordering the script's header insists on |
| `publish-when-green.sh <slug> <repo> <sha> <ver> <crate>...`: wait for main CI on `<sha>` (90 looks, 30 s apart), refuse on red (exit 3), 4 if CI never finishes | add `--require-green --github <slug> --ci-wait 2700` | CI runs are matched by commit (`gh run list --commit`), not by `--branch main` plus sha. A red run is refused as soon as it is seen, without waiting for the other runs. No runs at all is exit 4, never green. Exit 2 and 1 keep their usual meanings |

The whole of the old pair, as one command:

```bash
releaseant publish --repo C:/Projects/<name> --sha <full sha> --execute --tag --tag-name <name> \
  --require-green --github ThinkersJournal/<name> --retries 19 --retry-pause 120
```

`--require-green` is checked first (before anything is built); `--retries` needs `--execute`. Not done here, and not
in the repository's scope: Fuel and baracuda (held, board 307), and rewriting the PM's scripts into one-line wrappers.

## Notes for callers: a real multi-crate publish, step by step (0.0.51)

For a lane or the PM running `releaseant` instead of a hand-written loop. Each line says what it rests on.

1. **`releaseant plan --repo R --sha S --check-externals` first.** It costs seconds and exits 1 on a dependency outside
   the set that the registry cannot satisfy. Measured 2026-11-02: it reported Fuel's yanked
   `baracuda-kernels-sys ^0.0.1-alpha.158` at once, where `verify` spent 2 minutes failing 43 crates (and would have
   cost 105 minutes on a set that got further). `docs/R5-LIVE.md`.
2. **Publish in dependency order across repositories.** The tool orders crates inside one repository only. A repository
   whose crates need another repository's new version (Fuel needs baracuda's `baracuda-kernels-sys`) waits until that
   version is on the registry and the pin in its `Cargo.toml` has moved; `plan --check-externals` is the test.
3. **The command:**

   ```bash
   releaseant publish --repo C:/Projects/<name> --sha <full sha> --execute --tag --tag-name <name> \
     --require-green --github ThinkersJournal/<name> \
     --max-wait 21600 --retries 5 --retry-pause 120
   ```

   **For baracuda, `export DOCS_RS=1` in the shell first** (as `bk2-pub.py` does): without it `nvcc` compiles the CUDA
   crates during verification. Seen 2026-11-02: a baracuda publish started without it was aborted by the PM
   (`C:\Projects\.lane-state\baracuda-pub\*.aborted-nvcc.*`); with it the 73-crate verify took 8.5 minutes.
   `--sha` must be the tip of origin's default branch (a real upload refuses anything else; `--allow-not-tip` is the
   override). `--require-green` is checked first and exits 3 on a red commit, 4 if CI gave no answer in time.
4. **Rate limits (HTTP 429) are waited out inside a run**, not by `--retries`: the time cargo prints ("try again after
   <date>") plus 5 seconds, taken from the run's `--max-wait` budget, which is **per run and 3600 s by default**. A
   run that runs out of budget stops resumable, naming the crate. 21600 (6 h) is generous; the real rate of
   crates.io's new-version limit is **not measured** (R4's six uploads never met one), so no number here is a
   promise.
5. **A stop or failure is resumed, not restarted.** `--retries N` runs `publish` again up to N more times, and the
   loop ends only when the index, asked afresh, lists every crate of the run's plan at its version and not yanked
   (exit 0), or at a held version or checksum mismatch (stops at once, exit 1, needs a person), or when the attempts
   run out (exit 1). The same command run by hand later resumes the same way: the ledger knows what was uploaded.
6. **A resume does not repeat the verification** (0.0.51): a crate the ledger shows as `verified` at the same commit
   and version in the last 12 hours (`--reuse-verified-hours N`, 0 = always rebuild) is recorded as `carried over,
   not built again: run <id> at <time>`. The first run at a commit always builds every crate, and that is the long
   pole (Fuel at 0.35.2: 105 of 226 minutes before the first upload).
7. **Exit statuses:** 0 done (and tagged, with `--tag`); 1 the run ran and something is not published (look at the
   per-crate lines and the ledger); 2 an error before or around the run; 3 CI red; 4 CI gave no answer.
8. **Read the ledger, not the log.** `.releaseant/ledger.jsonl` (or `--ledger FILE`): `phase` events say where the
   time went, `waiting` events the rate-limit waits, `verified` events with `carried over` the saved builds.
   Copy it somewhere durable if the machine is not.

## Open items and risks

1. **Publish metadata is checked before upload (0.0.16, ROADMAP R6), split by who refuses (PM, 2026-10-29).**
   Found 2026-10-29 by mutating the README's sample: deleting `description` from a crate had left `plan` and the
   dry-run `publish` green (cargo package only warns). Now every planned crate is checked and all problems are
   listed at once. **Errors** (stop before anything is written): no `description`, neither `license` nor
   `license-file`, more than five keywords or one crates.io refuses, a category that is not a crates.io slug, a
   named README or licence file that is missing. **Warnings** (listed on stderr, exit 0): no `repository`,
   README, `keywords`, `categories` or `rust-version`; `--require-metadata FIELD,...` turns chosen ones into
   errors. The rules are the Cargo reference's; I did not test what crates.io answers to a missing `description`.
   **Run at `origin/main` on 2026-10-29: every real set plans with zero errors.** Warnings: fuel 43 crates
   (84: 43 `rust-version`, 35 README, 3 keywords, 3 categories), baracuda 73 crates (80: 67, 5, 4, 4),
   PersistAnt 9 crates (8, README), Unpopped 4 crates (2, README). The new single-crate Ants (AlarmAnt,
   WebHostAnt, secretant, mcpant, fireline, webservant, lightbulb) warn on `keywords`, `categories` and
   `rust-version` (httpant also README); `capant` and `configurant` have none. With the flag, AlarmAnt fails
   naming `keywords` and `rust-version` and says what to add.
2. **Token handling.** ReleaseAnt does not read or store a token; cargo finds `CARGO_REGISTRY_TOKEN` or
   `credentials.toml`. R4 ran from `credentials.toml` (the vault has no crates.io token, disclosed). Trusted
   Publishing (`PORTFOLIO-ROADMAP.md` C3) would run it in CI and need the ledger somewhere durable. Not designed.
3. **Name collisions** (fuel's `fuel-core`, `fuel-compression`) are found only at upload. `plan --check-names` is in
   the design and not built.
4. **A kill between upload and read-back, a lagging index, a real transient failure and the "new crates" 429 wording**
   have not been seen against the real registry; they are covered by fake-registry tests only. A retry after an
   injected `cargo publish` failure was run live (2026-11-02, `docs/R5-LIVE.md`).
5. **Windows only for real runs.** The rehearsal and every real read here were on this machine; CI runs the tests
   on Linux and macOS too. A real tag push from a lane uses the repository's own git identity and credentials.
6. **`--no-verify` is not gone, it is explicit.** A set whose crates all need `--no-verify-crate` is no safer
   than today's loop except for the record and the consumer check.
