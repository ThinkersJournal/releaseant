# First real use: publishing the next ConfigurAnt batch (runbook)

For the PM, who runs it; the releaseant lane is on call and reads the result. Prepared 2026-10-30 (UTC) from a
full dry run of the same set (below). **Nothing here uploads or tags anything unless `--execute` is on the
command line.** ReleaseAnt does not read or store the registry token; cargo finds it.

What is observed and what is not: lines marked *(rehearsal)* are what the dry run printed on 2026-10-30 against
configurant `origin/main` `c7cf82d698cb76eac8519b472f7edfe9d8eb3c31` (0.5.19) with `releaseant 0.0.16`.
Lines marked *(design)* come from `docs/DESIGN.md` or the code and have **not** been seen against the real
registry. The upload, read-back and tag steps of a real run have never run end to end; this is the first time.

## The batch

Five crates, new versions of crates that already exist on crates.io (checked in the index: all five have 0.5.16
as their latest, so none is a first-ever publish):

```
configurant 0.5.19            published in this order (rehearsal, plan):
configurant-scan 0.5.19         configurant, configurant-scan, configurant-gui,
configurant-gui 0.5.19          configurant-secrets, configurant-tui
configurant-secrets 0.5.19
configurant-tui 0.5.19
```

One version means one tag: `v0.5.19`, no `--tag-version` needed. Use the real release commit's SHA in place of
`$SHA` below (the rehearsal used current `origin/main`; the version you publish is whatever is in that commit).

## 1. Before you start

Run these in the order shown; each says what it should print. Commands are Git Bash; in PowerShell set
`$SHA = git -C C:\Projects\configurant rev-parse origin/main` and use `$SHA` the same way.

| # | Check | Command | Expect |
|---|---|---|---|
| 1 | The release commit is on origin | `git -C C:/Projects/configurant fetch origin` then `SHA=$(git -C C:/Projects/configurant rev-parse origin/main)` | a 40-hex SHA; it must be the commit you mean to publish. `releaseant tag` refuses a commit on no origin branch |
| 2 | The set and its order | `releaseant plan --repo C:/Projects/configurant --sha $SHA` | the 5 crates above, one version, exit 0. Any `error:` stops here (nothing was written) |
| 3 | Nothing is published yet | `releaseant readback --repo C:/Projects/configurant --sha $SHA` | `ABSENT` for all five *(rehearsal)*. If any says `present`, someone already published: stop |
| 4 | Metadata | `releaseant plan ... --require-metadata keywords,rust-version,readme` | the same 5 lines and **nothing on stderr** *(rehearsal: no warnings for this set)*. A `warning:` or `error:` line says which crate and what to add |
| 5 | The token | `cargo login`, or `CARGO_REGISTRY_TOKEN` in the environment | cargo finds it; ReleaseAnt never looks. A missing token would show up as `INTERRUPTED: unrecognised upload result for configurant: ...` carrying cargo's own message, before anything is uploaded *(design: not seen)* |
| 6 | Git can push the tag | the configurant checkout's `origin` push credentials and `user.name`/`user.email` | the tag uses the repository's own identity (`git -C C:/Projects/configurant config user.name`). Do not `gh auth switch` (portfolio rule) |
| 7 | The machine | no other cargo build running; `CARGO_BUILD_JOBS=2` | the verification builds dominate the time (below) |

Where the ledger goes: by default `<repo>/.releaseant/ledger.jsonl`, which puts an untracked directory into the
shared configurant checkout. The rehearsal used `--ledger` with a path **outside** every repository and left the
checkout untouched; use the same path for the real run and for any resume (a resume must find the same file).
Suggested: `--ledger C:/Projects/.release-ledgers/configurant-0.5.19.jsonl` (create the directory first).

## 2. The rehearsal (done; repeat it if the commit changes)

```
releaseant publish --repo C:/Projects/configurant --sha $SHA --tag \
  --require-metadata keywords,rust-version,readme --ledger <ledger path>
```

Printed *(rehearsal)*, exit 0, nothing on stderr:

```
configurant: would upload
configurant-scan: would upload
configurant-gui: would upload
configurant-secrets: would upload
configurant-tui: would upload
dry run: nothing was uploaded (use --execute)
ledger: <ledger path>
would tag v0.5.19 -> c7cf82d698cb76eac8519b472f7edfe9d8eb3c31 (dry run: nothing was created; use --execute)
```

It took **753 seconds (12.5 minutes)** of wall time on a machine where other lanes were also building (two to
three cargo processes besides ours); nearly all of it is the pre-upload build of each extracted crate. On a quiet
machine expect less. A real run adds the uploads, the index waits and the read-back; **that time is not
measured**. The dry run's ledger had 7 events: `planned`, five `verified`, `finished complete`.

## 3. The real run

Same command with `--execute`:

```
releaseant publish --repo C:/Projects/configurant --sha $SHA --execute --tag \
  --require-metadata keywords,rust-version,readme --ledger <ledger path>
```

Only add `--max-wait <seconds>` if you expect rate limiting (default 3600). New versions of existing crates
should not hit crates.io's new-crate limit; the real limits have not been measured by this tool (R4 did not
reach them).

## 4. Reading the output

Per crate, one line `name: <outcome>` *(design: `describe` in `src/main.rs`)*:

| Outcome | Meaning | What to do |
|---|---|---|
| `published and read back` | uploaded, the index shows it, and the index's checksum equals the SHA-256 of the file uploaded | nothing |
| `already published` | the registry already had this version | nothing (this is what a resume prints for finished crates) |
| `INTERRUPTED: unrecognised upload result for X: <cargo's output>` | cargo refused the upload for a reason the tool has no sample for (a missing token, name taken, validation, size, a registry outage...). The tool assumes nothing, retries nothing and **stops the run**; the crates after X show `not attempted` | read the quoted cargo output in full; section 6 |
| `FAILED: <why>` | only after repeated transient errors, or if cargo cannot be started | read `<why>`; section 6 |
| `CHECKSUM MISMATCH (uploaded A, index B)` | the registry holds different bytes than we sent | **stop**; do not re-run blindly; section 6 |
| `MISSING from the index after upload` | uploaded but never became visible within the deadline | wait a few minutes, then re-run the same command (it asks the registry first) |
| `INTERRUPTED: max-wait exceeded while publishing X` | a rate-limit wait would exceed `--max-wait` | re-run the same command later (or with a larger `--max-wait`) |
| `skipped (blocked by X)` / `not attempted` | an earlier crate failed or the run stopped | after fixing X, re-run |
| `run stopped (resumable): <why>` | a rate limit exceeded `--max-wait`, or the registry could not be asked | exit 1; **re-run the same command**: every run is a resume |

After the crates: a `consumer check` line only for crates you skipped verification for (none here), then
`ledger: <path>`, then the tag line (section 5).

## 5. After it finishes: what to read back

All of these should hold; the first four are what "done" means.

1. Exit code 0, and **five** lines `...: published and read back`.
2. The tag line: `tagged v0.5.19 -> <SHA> (read back from origin)`.
3. `releaseant readback --repo C:/Projects/configurant --sha $SHA` prints `present (<sha256>)` for all five.
4. `git -C C:/Projects/configurant ls-remote --tags origin v0.5.19^{}` prints the same `<SHA>` you published from.
5. The ledger's last events: `visible` for each crate, `finished complete`, then `tagged`
   (`python -c "import json,sys;[print(json.loads(l)['event'],json.loads(l).get('crate','')) for l in open(sys.argv[1])]" <ledger path>`).
6. Optional spot check against the registry's own record: `https://crates.io/api/v1/crates/configurant/0.5.19`
   shows a `checksum` that starts the same as the one `readback` printed.

## 6. When something goes wrong

| Symptom | State | Recovery |
|---|---|---|
| `error: publish metadata is incomplete ... nothing was uploaded` | nothing written | fix the named `Cargo.toml` fields, commit, use the new SHA |
| `error: dependencies outside the set are not on the registry` | nothing written | publish the missing dependency first (or `--exclude NAME --exclude-reason ...` if it is meant to wait) |
| `error: verification failed; nothing was uploaded` | nothing uploaded | fix the crate, new commit and SHA. Do not skip with `--no-verify-crate` for a crate that fails to build |
| `error: the ledger has a run at this commit with a different plan` | a resume with a different crate set (e.g. a new `--exclude`) | use the same flags as the first run, or a new ledger file for a deliberate different run |
| `run stopped (resumable)` after a rate limit or `max-wait`, or `MISSING` | partly published, ledger records how far | **re-run the identical command**. Already published crates print `already published`; the rest continue |
| `INTERRUPTED: unrecognised upload result for N: ...` | crates before N are published; N and the ones after it are not (`not attempted`). The upload of N itself may or may not have happened: the re-run asks the registry first | read cargo's text. If it is fixable without changing the source (a missing token, a network problem), fix it and **re-run the identical command**. If the source must change, the set is **half-published**: tell the lane before doing anything, and do not publish the rest by hand |
| `CHECKSUM MISMATCH` | the registry holds other bytes than ours | stop; send the lane the whole output and the ledger. A published version cannot be deleted; `releaseant yank` (below) is the only remedy and it is your decision |
| `PUBLISHED but not tagged: <why>` (exit 1) | **all five are on crates.io; only the tag failed** | see below |

### `PUBLISHED but not tagged`

The crates are fine; do not publish again. The line ends with the exact command to finish. Read `<why>`:

| `<why>` says | Cause | Fix, then run `releaseant tag --repo C:/Projects/configurant --sha $SHA --version 0.5.19 --execute` |
|---|---|---|
| `is on no origin branch; push it first` | the SHA is not on any origin branch | `git push origin <branch>` so origin has the commit |
| `already points at X on origin, not Y; tags are never moved` | `v0.5.19` exists on another commit (or a lightweight tag does) | **do not move it.** Decide with the lane which commit the release is; if the existing tag is wrong, that is a PM decision outside this tool |
| `already points at X on local, not Y` | a stale local tag of that name in the shared checkout | `git -C C:/Projects/configurant tag -d v0.5.19` (local only), then re-run |
| `git failed: ...` on the push | credentials, network or a protected tag rule | fix it; the tag was created locally and is reused by the re-run |
| `pushed v0.5.19 but origin shows ...` | the read-back disagrees with the push | stop; send the lane the output |
| `the crates are at several versions` | not expected for this batch | pass `--tag-version 0.5.19` |

Re-running the whole `publish ... --execute --tag` command also works: it finds everything already published and
only tags. A second `tag` run on a tagged commit prints `already tagged v0.5.19 -> <SHA>`.

### Yanking (only if a published version is wrong)

```
releaseant yank NAME 0.5.19 --reason <kind>:<text> --ledger <ledger path>            # dry run
releaseant yank NAME 0.5.19 --reason <kind>:<text> --ledger <ledger path> --execute
```

`kind` is one of `security`, `broken-build`, `wrong-contents`, `licence`, `other`. It refuses a version the ledger
does not show this tool published. A yank is the PM's decision; the tool never does it on its own.

## 7. What to send the lane (on call)

The complete stdout and stderr of the run (not a summary), the ledger file, and the output of the section 5
checks. The lane will read the ledger's events against the registry and say what state the set is in.

## 8. Not covered, because it has not been seen

- A real upload, real read-back and a real tag push through this tool: this is the first.
- crates.io's rate limits for these crates: unmeasured; the classification of a real 429 is from a captured
  sample in R4 (`tests/fixtures/rate_limited.txt`), not from this batch.
- A kill between an upload and its read-back, and a lagging index: covered by fake-registry tests only.
- Windows only: every real run so far was on this machine.
