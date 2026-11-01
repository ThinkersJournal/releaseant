# R5 slice 2: `--require-green` and `--retries` run live

Status: evidence for the PM (ROADMAP R5, `[TASK]` of 2026-11-02). Written by the releaseant lane.
Tool: `releaseant 0.0.49` (a debug build of `origin/main` `01ae1001b6a9406e1a590e33f5ddda71148ce574`),
`gh 2.94.0`, the real forge `ThinkersJournal/releaseant`. No fake `gh`, no fake registry.

## 1. `--require-green`, live (2026-11-02, no registry writes)

Each case ran `releaseant publish --repo C:/Projects/releaseant-rehearsal --sha <sha> --execute --ledger <scratch>
--require-green --github ThinkersJournal/releaseant --ci-wait 0`. The guard runs before anything is built or
uploaded, so no case reached the registry. `<sha>` is a commit on the forge; what the forge says about each, read
with `gh run list -R ThinkersJournal/releaseant --commit <sha> --json status,conclusion`:

| case | `<sha>` | what the forge has for it | predicted | measured |
|---|---|---|---|---|
| red | `0e8cab1a4be001de010c673e8fbd67b48bdd0019` (a `main` commit whose `CI` run failed) | a run with conclusion `failure` | exit 3 | **exit 3** |
| no answer | `dc8bd420dcba8b683dfd2dcdea1015332265f66b` (a commit the forge has no run for) | 0 runs | exit 4 | **exit 4** |
| green (control) | `01ae1001b6a9406e1a590e33f5ddda71148ce574` (`main` after #48) | 2 runs, all finished well | passes the guard | **passed the guard** |

Transcripts, verbatim:

```
=== RED
waiting for CI on ThinkersJournal/releaseant 0e8cab1a
error: CI is RED on 0e8cab1a (a run concluded "failure"): not publishing
exit 3
=== NOANSWER
waiting for CI on ThinkersJournal/releaseant dc8bd420
error: CI gave no answer on dc8bd420 (0 run(s), 0 not finished): not publishing
exit 4
=== GREEN
waiting for CI on ThinkersJournal/releaseant 01ae1001
CI is green on 01ae1001
error: could not read origin's tip (fatal: 'origin' does not appear to be a git repository
fatal: Could not read from remote repository.
 ...); fix that, or pass --allow-not-tip
exit 2
```

The green case is the control: the same command gets past the guard and is stopped by the next check (the
rehearsal repository has no `origin`, so the tip check cannot run), which is why it exits 2 and not 0. It uploaded
nothing. `--ci-wait 0` means one look and no waiting; the polling loop (30 s between looks) is covered by the unit
tests in `src/ci.rs`, not by this transcript.

## 2. `--retries`, live

The write steps were run by the PM (2026-11-01 22:46-22:47 UTC), authenticated by this box's cargo credentials file
(the lane never read or used a token), with the 0.0.49 exe built from `main` `01ae1001`. Only the rehearsal crates
`releaseant-rehearsal-core`, `-mid`, `-top` were written. Raw files: `docs/rehearsal/r5-run1-out.txt`,
`docs/rehearsal/r5-run1-ledger.jsonl`.

### 2a. `--retries 3 --retry-pause 20`, the three crates at 0.0.2 (commit `0f0e26de489ec1d4b9945104ab8ba5b7b3aa938b`)

| | predicted | measured |
|---|---|---|
| runs made | 1 (a lagging index would add runs) | **1**: `the registry shows every crate after 1 run(s)` |
| outcomes | `published and read back` x3, exit 0 | as predicted |
| lag | possible | **not seen**: each crate was read back visible at once; a lagging index cannot be forced |

So 2a shows the confirming path of the loop against the real index (it asked `versions` for the three crates and found
them at 0.0.2, not yanked); it does not show a retry. The retry is 2b.

First real `phase` events (R8): for `releaseant-rehearsal-core`, offsets in seconds from the start of `cargo publish`:
Packaging +1.4, Verifying +1.4, first Compiling +5.9, Finished +9.5, Uploading +9.5, Uploaded +10.2, Published +10.7.
For these tiny crates the verification build is about 8 of the 10.7 s and the upload under 1 s. Three crates are not
a measure of Fuel's 88.8-minute `fuel-cuda-backend`.

### 2b. A real retry (forced failure)

Run by the PM (2026-11-01 22:49-22:50 UTC), 0.0.49 exe, commit `7fd102bde38dcdacd74a38974347ebdafcead560` (the three
crates at 0.0.3, absent from crates.io before: 404 x3; 200 x3 after). `CARGO` pointed at
`docs/rehearsal/fail-first-publish.cmd`, which fails the first `cargo publish` before anything is sent and passes
every other call to the real cargo. The failure is injected at the cargo boundary: it is a stand-in for a transient
failure, not a measured one. The upload, crates.io and the index read-back are real. Command:
`releaseant publish --repo C:/Projects/releaseant-rehearsal --sha 7fd102bd... --execute --allow-not-tip --retries 3
--retry-pause 20 --ledger ledger2.jsonl`. Raw files: `docs/rehearsal/r5-run2-out.txt`, `r5-run2-ledger.jsonl`.

| step | predicted | measured |
|---|---|---|
| attempt 1 | stops at the first upload, nothing on the registry | `uploading releaseant-rehearsal-core 0.0.3` then `classified: unknown: error: simulated failure ...`; ledger run `...-0`: `upload_finished Unknown(...)`, `finished unrecognised upload result` |
| the loop | a line for attempt 1, a 20 s pause, a second run | `attempt 1 of 4: ...` printed; second run (ledger run `...-1`) started 33 s after the first ended (20 s pause plus about 13 s of verification) |
| attempt 2 | uploads core, mid, top, each read back | `Accepted` + `visible` x3, `finished complete`, none uploaded twice |
| end | confirmed by the index, exit 0 | `the registry shows every crate after 2 run(s)` |

Attempt 1's line in that transcript reads `attempt 1 of 4: not yet on the registry: ["unrecognised upload result ...\r\n"]`: right outcome, poor wording (a run that stopped is not "not yet on the registry", and the debug form shows the line ending as `\r\n`). Fixed after this run: the line now reads `attempt 1 of 4: the run did not complete: <reason>` (`a_stopped_run_is_reported_as_not_complete`).

What stays unmeasured: a real transient network failure, and a lagging index (neither can be forced on crates.io).
The loop's reaction to both is the same code path as the injected failure (a run that does not end with the index
showing every crate) and is covered by the unit tests in `src/retry.rs`.
