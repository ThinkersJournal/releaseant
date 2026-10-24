# ReleaseAnt R0 survey: what the hand-written publish tooling already does

Measured 2026-10-23. Every source was read with `git show <sha>:<path>` at the `origin/main` SHA below (never a working tree), except the PM's two scripts, which are not tracked anywhere (see "Sources").

## Sources

| Source | Ref | Read |
|---|---|---|
| `scripts/publish.ps1` (baracuda) | `3d97927a6c2a0a884d64488e200b99beb70f1dcd` | whole file |
| `scripts/check-dev-dep-publish-cycles.py` (fuel) | `5aaad145fb9277d87663b6384f46aadb83d5c3f7` | whole file |
| `scripts/verify-packaged-crates.py` (fuel) | same | whole file |
| `docs/release-0.13.0-wave.md` (fuel) | same | whole file (the "Publish feasibility" section is the only publish content) |
| `.github/workflows/rust-ci.yml` (fuel), lines near 864-897 | same | grep only: shows how the two fuel scripts are called in CI |
| `.github/workflows/release.yml`, `scripts/test-release.ps1` (auth-framework) | `9abbced8270655414e3e717227763d0f1c2fa4b7` | whole files |
| `tools/pub-one.cmd`, `tools/claim-retry.sh` (PM) | **no ref**: files in `C:\Projects\tools`, not tracked by the portfolio history repo (`ls-files` for both paths prints nothing; the same query lists `tools/sync-checkouts.sh`, which is tracked) | whole files; mtimes 2026-10-21 18:32 and 18:43 |
| `fu-pub.py`, `bk2-pub.py`, `fz-pub.py` (PM, `%TEMP%`; named by the PM's welcome notice) | **no ref**: untracked scratch files, mtimes 2026-10-23 10:31, 17:11, 17:41; `fu-pub.py` and `fz-pub.py` differ from each other only in the two metadata/dir file names | whole files |
| `bk2-pub.log`, `fz-pub.log` (PM runs in progress) | read 2026-10-24 00:45 UTC; **runs unfinished**, last lines `429 wait baracuda-nvshmem-sys` and `429 wait fuel-backend-contract` | whole files; used only for observed behaviour, not as a count of anything |
| `/tmp/claim-retry.sh` | mtime 2026-10-22 10:14 | `diff` against `tools/claim-retry.sh`: identical |

### Two things the roadmap row promised that the sources do not contain

1. **There is no auth-framework crates.io publish loop at that SHA.** `release.yml` builds binaries, builds a Docker image and makes a GitHub release; it never runs `cargo publish`. `scripts/test-release.ps1` is a pre-flight of greps (feature names present in Cargo.toml, target names present in the workflow). So auth-framework contributes the *negative* finding that its crates.io publishing is manual and undocumented in the repo. Positive control for the null: the same read of baracuda's `publish.ps1` shows `cargo publish` at one call site.
2. **`PORTFOLIO-ROADMAP.md` describes no fuel or auth-framework publish loop.** Its only publish content is the date-stamped plans (the fuel 0.11.0 publish is "deferred lane work"; auth-framework's 0.5.0-rc18..rc26 yank is an item in a plan). The fuel loop is the PM's `fu-pub.py`/`fz-pub.py`, pointed out by the PM's welcome notice. Fuel's repo has no publish script at that SHA (`git ls-tree` filtered for `publish|release|crates` finds the wave doc and the two scripts only).

R5 ("retire the hand-written loops in fuel and baracuda") therefore means: the loops to retire are baracuda's `scripts/publish.ps1` and the PM's `*-pub.py` scripts; fuel's two CI scripts are guards to keep or fold in; fuel's repo has no publish script of its own.

## What each source does

### PM `fu-pub.py` / `bk2-pub.py` / `fz-pub.py` (the loops actually publishing now)
- Metadata from a saved `cargo metadata` JSON; set = packages with `publish != []`.
- **Computed topological order** by DFS over normal and build deps plus *versioned* dev-deps only; path-only dev-deps are dropped, with the comment "dev-deps with path are dropped on publish". This is fuel's rule (the CI guard above) applied in code. It is not a tested fact about cargo (see Open question 1).
- **Cycle handling is silent.** `seen` is set before recursing and the `stack` parameter is never used, so a cycle yields some order and no report (trap 6 is not met).
- Skip a version already on the registry via `GET /api/v1/crates/<n>/<v>`; **any exception (timeout, 5xx, DNS) counts as "not published"** and falls through to an upload attempt.
- Upload with `cargo publish -p <n> --locked --no-verify` (`fu-pub.py`, `fz-pub.py`), or `-p <n>@<v> --locked` with `DOCS_RS=1` and verification ON (`bk2-pub.py`, which also drops `baracuda-cuda-seam` from the set by name).
- Success = exit 0 **or** the text `already exists`. Rate limit = the text `429`, `Too Many` or **`'rate' in out.lower()`**, which also matches any error whose text contains "rate" (e.g. "generate", "separate"), so such a failure would sleep 90 s and retry 12 times instead of stopping. Read from the code, not run.
- 12 tries per crate at a fixed 90 s; any other failure ends the whole run (`sys.exit(1)`), so a resume is a re-run (which re-checks the API once per crate).
- Publishes from a directory that is not a git repository (`git rev-parse --show-toplevel` fails there). **How that directory was made is not recorded in any file read**, so trap 1 is unverified for these runs.
- Nothing reads back after upload; the log is stdout lines `PUBLISHED`/`SKIP`/`429 wait`/`FAIL`, no ledger, no timestamps.
- Observed in the (unfinished) logs: 429s hit roughly every other crate from about the 36th upload (`bk2-pub.log`), and `fuel-ir` was published after one 429 (`fz-pub.log`). One run's lines only; not a rate model.

### baracuda `scripts/publish.ps1` (the retry-based loop)
- Publish set from `cargo metadata --no-deps`: `publish` null or non-empty allowlist is in, `publish = []` (i.e. `publish = false`) is out.
- Per-crate `cargo publish -p NAME@VERSION --no-verify --allow-dirty` (name-version because a bare `-p NAME` is ambiguous when the graph holds a registry copy).
- **No topological sort.** Order comes from retrying: a crate whose dependency is not on the index ("failed to select a version", "no matching package", "cannot find") is deferred to the next pass. Up to 12 passes; a pass with no progress sleeps 30 s.
- Failure classification by regex on cargo's text: already uploaded (= success, skipped), rate limit / 429 / "published too many" (sleep the "after N seconds" value + 5, default 65), network (sleep 30, up to 4 attempts), anything else (defer, log).
- Pacing after 28 uploads (61 s sleep each).
- Preflight guards that abort before any upload: licence files identical to the root copies in every publishable crate; lockstep version consistency (warns only); exception-line crates (`baracuda-cuda-emit`, `baracuda-cuda-parse`) must have MAJOR.MINOR equal to their `unpopped` requirement (throws, "a published version can never be taken back").
- `-DryRun` lists the set and uploads nothing.
- Log file per run in `target/`.
- Its header comment records two traps: dev-dependencies count for the publish set (baracuda-nvrtc dev-depends on baracuda-driver, which depends on baracuda-cuda-sys), and `--no-verify` is used because the CUDA crates cannot build without the CUDA environment.

### fuel `check-dev-dep-publish-cycles.py` (CI guard)
- Fails if a workspace crate has a dev-dependency on another workspace member that carries a version (`req != "*"`). Reason in its docstring: `cargo publish` strips a path-only dev-dep but keeps one with a version, which must then resolve on crates.io, which breaks any order computed from normal/build deps alone and is an unpublishable cycle when the target reaches the package.
- This is a *prevention* of trap 2 rather than a computation of the set.

### fuel `verify-packaged-crates.py` (CI guard, the closest thing to "verify before upload")
- Input: a directory of `.crate` files from `cargo package`.
- `scan`: every `include_str!`/`include_bytes!`/`include!`/`#[path]` target and every `CARGO_MANIFEST_DIR/..` read must be inside the same tarball. Escapes in `src/` or `build.rs` fail with no allowlist; escapes in `tests/`, `benches/`, `examples/` need a ratchet allowlist (unlisted fails, stale entry fails). Hidden dot-files count as escapes (packaged from git, not from an archive).
- `build`: extract every `.crate` into a throwaway workspace, point crates at each other with `[patch.crates-io]`, `cargo check --workspace`. It prints its own named hole (CUDA crates not verified, excluded crates not built).
- `selftest`: planted-bad controls for every rule, each must be flagged. This is the pattern ReleaseAnt's test plan should copy (a negative control per trap).
- Safe extraction (refuses absolute paths, `..`, links) because the tool does not trust tarballs.
- Gap: `build` patches *sibling* crates by path, so it does not prove the set resolves against the registry in publish order. That is what the real run does, and what a rehearsal (R4) must prove.

### fuel `docs/release-0.13.0-wave.md` (publication facts, not tooling)
- ~45 workspace crates, most never published; crates.io's new-crate limit "a burst of 5, then ~1 per 10 minutes" gives about 7 hours, so a publish needs a scheduled window, and the PM runs it.
- Name collisions with other owners on crates.io (`fuel-core`, `fuel-compression`) found only after the fact via the crates.io API. A pre-flight "is this name mine?" check is a behaviour no script has.
- Cross-project ordering: baracuda's work waits on fuel's wave, so a publish set can have *external* prerequisites (a version not yet on the index).

### PM `tools/pub-one.cmd` + `tools/claim-retry.sh` (placeholder claiming)
- `pub-one.cmd`: clear `RUSTC_WRAPPER`, `cd` to the crate directory, `cargo publish --allow-dirty` (no `--no-verify`).
- `claim-retry.sh`: for each name, up to 30 tries: skip if `GET /api/v1/crates/<name>/0.0.0` already shows the version (**already-published check against the registry API**), else publish; success is detected by grepping `Published` in output; a `429` sleeps 600 s and retries; any other error sleeps 30 s and retries. Sleeps 15 s after a success.
- Hard-coded to a session scratchpad path, to a default list of names, and to 0.0.0 placeholders.
- Detects outcomes by matching human-readable text from cargo, not exit codes or structured output.

### auth-framework (negative finding, see above)
`test-release.ps1` is the pre-flight pattern in its weakest form: it checks that strings exist in files. It would pass a workflow that does nothing. ReleaseAnt's checks should run the thing, not grep for its name.

## Behaviours shared by two or more sources

| # | Behaviour | In | SCOPE trap |
|---|---|---|---|
| S1 | Publish set from `cargo metadata`, excluding `publish = false` | baracuda; PM scripts; fuel's guards read the same metadata | 6 |
| S2 | Dev-dependencies change what must be on the index first (versioned dev-deps count, path-only are stripped) | baracuda (comment + nvrtc example); fuel (CI guard); PM scripts (in code) | 2 |
| S3 | Skip a version that is already published (idempotent re-run) | baracuda (regex on cargo output); PM claim script and PM `*-pub.py` (registry API) | 4 |
| S4 | Rate-limit handling: sleep and retry, never abort | baracuda (65 s or server-advised); PM claim script (600 s); PM `*-pub.py` (90 s fixed) | 4 |
| S5 | Classify failures by matching cargo's human text (one match, `'rate' in out.lower()`, is far too broad) | baracuda; PM (all) | 4 |
| S6 | A bounded retry count per crate | baracuda (4 attempts, 12 passes); PM claim script (30); PM `*-pub.py` (12) | 4 |
| S7 | Extract-and-check packaged tarballs | fuel only (CI), not part of any publish loop | 1 (nearest) |
| S8 | `--no-verify`, unrecorded | baracuda (every upload); `fu-pub.py`, `fz-pub.py` (every upload). Not `bk2-pub.py` | 5 |
| S9 | A dry-run mode | baracuda | (SCOPE: default) |
| S10 | Preflight that aborts before any upload | baracuda (licences, version line) | 5/8 (nearest) |

## Behaviours SCOPE wants that NO surveyed source has

Each of these starts from nothing and needs its own test with a negative control.

- **Trap 1** (publish from `git archive <merge sha>` in a clean dir): baracuda and pub-one.cmd use `--allow-dirty` on a working tree. The PM `*-pub.py` scripts publish from a non-git directory of unrecorded origin: no script *archives*, and none records which SHA it published.
- **Trap 3** (caret on a prerelease does not match an older prerelease): nothing checks it.
- **Trap 6, cycles reported**: order exists in the PM scripts (DFS) and is replaced by retry in baracuda; neither reports a cycle. The PM scripts ignore it silently; baracuda would exhaust 12 passes and print "Failed: N" without naming one.
- **Trap 7** (read-back from sparse index and API with a timeout naming the missing crate): nothing reads back after upload. The PM script reads the API *before* upload only. The baracuda "published" decision is cargo's exit code alone.
- **Trap 8** (yank policy): no source yanks or mentions a policy.
- A **ledger** (durable record of what was published, when, from which SHA): baracuda writes a free-text log; the PM script keeps nothing.
- **Recording** of any `--no-verify` exception (S8): not recorded anywhere.

## Open questions for R1

1. **Do dev-deps need to be on the index for a single-crate `cargo publish`?** baracuda's header says both "single-crate publish doesn't enforce dev-dependency availability, which lets types-derive publish before types" and "cargo package needs a versioned dev-dependency on the index too". These two statements conflict as written, and fuel's script states the second. Not tested in R0. R1 must settle it with an experiment against a throwaway set (the R4 rehearsal) and then fix SCOPE trap 2's "computed from `cargo package` resolution" wording accordingly. Until then the design must not assume either.
2. **Deferral by retry (baracuda) vs computed order**: retry hides a cycle and costs up to 12 passes of 30 s sleeps; an order computed from resolved metadata can report a cycle immediately. But the baracuda DEV-dep cycle (`baracuda-types` <-> `baracuda-types-derive`) shows a real set can contain a cycle that *does* publish, because path-only dev-deps are stripped. The design needs a rule for which edges count (S2) and what "cycle" means then.
3. **Structured output over text matching** (S5): does `cargo publish` expose anything better than regexes on messages? If not, the design must pin the message patterns it relies on and test them against real captured output.
4. **crates.io limits**: the burst figures (5 new then 1 per 10 min; 28 pacing in baracuda) are quoted by people, not measured by us here. R1 should take the current numbers from crates.io's documentation and the 429 body (`Retry-After`) rather than from these scripts.
5. **Name ownership pre-flight** (fuel collisions): include in `plan`? (`GET /api/v1/crates/<name>` owners.)
6. **Authentication**: all sources assume a token already in the environment; the portfolio is looking at crates.io Trusted Publishing (`PORTFOLIO-ROADMAP.md` C3). A publish tool should not own token handling; R1 should state that.

## What R0 did not do
- Did not run any of the scripts.
- Did not read the PM's other scratch scripts (`ba-pub*.sh`, `bk-pub.sh` beyond its first 20 lines, `unp-pub.sh`, and about 20 more `*-pub*` entries in `%TEMP%`), nor fuel's/baracuda's CI workflows beyond the three call-site lines in `rust-ci.yml`.
- Did not contact crates.io.
