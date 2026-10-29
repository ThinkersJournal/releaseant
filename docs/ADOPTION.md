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
| `description` and `license` present in each crate | no | no | **no** (see open item 1) |
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
2. Check by eye that the crate has `description` and `license` (open item 1).
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
- `baracuda-cuda-seam` stays out until fuel 0.24 is consumed (PM, 2026-10-24):
  `--exclude baracuda-cuda-seam --exclude-reason "waits for the fuel release"`. Checked 2026-10-29 at baracuda
  `4600be3`: `plan` = 73 crates; `plan --exclude baracuda-cuda-seam --check-externals` = the same list minus that
  line, exit 0.
- Two streams (`0.32.0` and `0.1.0-alpha.44` then): one run plans both. **Which one the tag names is the PM's
  rule**; the tool asks for `--tag-version` and never guesses (the PM's practice so far: the alpha stream).
- CUDA crates: `DOCS_RS=1` in the shell, as `bk2-pub.py` does, or `--no-verify-crate NAME --reason "..."`.
- After one clean real run the baracuda lane deletes `scripts/publish.ps1` (its PR, not this note).

**4. fuel** (43 crates at one version, `0.30.1` at `6c1b56c`): `releaseant publish --repo C:\Projects\fuel --sha
<sha> --execute --tag --max-wait <seconds>`. Keep both CI guards (dev-dep cycles; packaged-crate scan). A first
publish of a wave is paced by crates.io's new-crate limit, which `docs/release-0.13.0-wave.md` puts at about 7
hours for about 45 new crates; ReleaseAnt stops resumable when its wait budget is spent, so run it again. The
limit itself was not reached in the R4 rehearsal and is still unmeasured here.

## Open items and risks

1. **Missing `description` or `license` is not caught before upload.** Found 2026-10-29 by mutating the README's
   sample: deleting `description` from a crate leaves `plan` and the dry-run `publish` green. The Cargo
   reference lists both as required to publish; I did not test what crates.io answers, so whether the first
   crate of a run would be refused (and the run stop resumable) or accepted is unverified. A pre-flight check
   (before anything is written) belongs on the roadmap; until then step 2 above is by eye.
2. **Token handling.** ReleaseAnt does not read or store a token; cargo finds `CARGO_REGISTRY_TOKEN` or
   `credentials.toml`. R4 ran from `credentials.toml` (the vault has no crates.io token, disclosed). Trusted
   Publishing (`PORTFOLIO-ROADMAP.md` C3) would run it in CI and need the ledger somewhere durable. Not designed.
3. **Name collisions** (fuel's `fuel-core`, `fuel-compression`) are found only at upload. `plan --check-names` is in
   the design and not built.
4. **A kill between upload and read-back, a lagging index, and the "new crates" 429 wording** have not been seen
   against the real registry; they are covered by fake-registry tests only.
5. **Windows only for real runs.** The rehearsal and every real read here were on this machine; CI runs the tests
   on Linux and macOS too. A real tag push from a lane uses the repository's own git identity and credentials.
6. **`--no-verify` is not gone, it is explicit.** A set whose crates all need `--no-verify-crate` is no safer
   than today's loop except for the record and the consumer check.
