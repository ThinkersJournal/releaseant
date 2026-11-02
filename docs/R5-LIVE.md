# R5 slice 2: `--require-green` and `--retries` run live

Status: evidence for the PM (ROADMAP R5, `[TASK]` of 2026-11-02). Written by the releaseant lane.
Tool: `releaseant 0.0.49` (a debug build of `origin/main` `01ae1001b6a9406e1a590e33f5ddda71148ce574`),
`gh 2.94.0`, the real forge `ThinkersJournal/releaseant`. No fake `gh`, no fake registry.

## 0. Dry runs on baracuda and Fuel (2026-11-02, ahead of tonight's publishes; no `--execute`, no writes)

Tool `releaseant 0.0.50` (debug build of `main` `1588e91`), `CARGO_BUILD_JOBS=2`, one verify at a time. Commands:
`releaseant plan|plan --registry|verify --repo C:/Projects/<repo> --sha <origin/main>`; baracuda's verify with `DOCS_RS=1`.
Plans and the baracuda output: `C:\Projects\r5run` (not committed).

| | baracuda | fuel |
|---|---|---|
| ref (`origin/main` at 00:35 UTC) | `87957c5754ab3f698a0b5a1f27e0847251621f5a` (alpha.79) | `3f6ec6a49386acbe71cc71539b4f28e3d5f62368` |
| crates in the plan | **73** (71 at `0.1.0-alpha.79`, 2 at `0.40.0`) | **43**, all `0.36.0` |
| order starts | baracuda-build, baracuda-cuda-parse, baracuda-cuda-vocab | fuel-conv, fuel-cpu-kernels, fuel-ir |
| `plan --registry` | all 73 "not on registry" | all 43 "not on registry" |
| `plan` | exit 0 | exit 0 |
| `verify` | exit 0, **73 of 73 verified, 8.5 min** (00:42:27 to 00:50:53 UTC; my own cargo builds ran alongside, so an upper bound for this box) | **exit 1, 43 of 43 FAILED in 2 min** (00:50:54 to 00:52:51 UTC), see below |

The PM's figures for tonight (alpha.80, Fuel 0.36.2, "100+" and 45 crates) were estimates; main did not have those
versions at this ref, so the numbers above are for the SHAs named.

**Fuel is blocked on a yanked dependency.** Every one of the 43 `cargo check`s failed the same way:
`failed to select a version for the requirement baracuda-kernels-sys = "^0.0.1-alpha.158": version 0.0.1-alpha.158 is
yanked`. `Cargo.toml` line 354 of `3f6ec6a4` pins that version for `fuel-cuda-backend`. The crates.io API lists 73
versions of `baracuda-kernels-sys` and none is un-yanked (the newest, `0.1.0-alpha.25`, is yanked too). `releaseant
plan --repo C:/Projects/fuel --sha 3f6ec6a4... --check-externals` reports it in seconds and exits 1 (`MISSING EXTERNAL:
fuel-cuda-backend needs baracuda-kernels-sys ^0.0.1-alpha.158: every published version is yanked`); the same check on
baracuda exits 0. So Fuel cannot be verified or published until baracuda has republished `baracuda-kernels-sys` and
Fuel's `main` depends on that version; **run `plan --check-externals` first, it costs seconds and verify costs 105
minutes.** Fuel's verify time at this ref is therefore not measured.

**Time estimates for the real publishes, with what each rests on:**
- baracuda: about 9 minutes before the first upload (the verify above), then 73 uploads. `cargo publish` rebuilds each
  crate; Fuel's median was 23 s per crate (0.35.2), so about 28 minutes if baracuda's crates are alike (not measured for
  baracuda). About 40 minutes without a rate-limit stop.
- fuel (not measurable at this ref, see above; the 0.35.2 run is the only data): 225.8 minutes = 104.9 before the first upload (package and `cargo check` of 43 crates) + 119.7
  of uploads (88.8 of them `fuel-cuda-backend`) + 1.1 other. The `phase` ledger events will show where the 88.8 go.
- crates.io's new-version rate limit: R4's six uploads never met one; the real baracuda run did (section 3b: a burst of
  at least 46 uploads, then about one a minute). A 429 is
  waited out inside the run, for the time cargo prints ("try again after <date>") plus 5 s, spending the run's
  `--max-wait` budget (default 3600 s for the whole run); past it the run stops resumable. `--max-wait 21600` was
  advised. With 0.0.51 a resume after that stop no longer repeats the verification.

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

## 3. The real baracuda publish (2026-11-02), read from its ledger

Source: `docs/rehearsal/baracuda-2026-11-02-ledger.jsonl` and `...-out.txt`, copied unedited from
`C:\Projects\.lane-state\baracuda-pub\` (run by the PM with the release exe). Run `d9b28b6f-1793584035-33952-0`,
commit `d9b28b6f07905c7dbeb5d2959ad25c36945b6516`, 73 crates at `0.1.0-alpha.80` (two at `0.40.0`), **one run, finished
`complete`, 69 min 46 s** by the transcript's own clock (the first ledger row is stamped when verification ended, so the
ledger alone undercounts the start). `DOCS_RS=1` set. The earlier run started without it was aborted (`nvcc` compiled
CUDA during verify) and wrote nothing. 1133 ledger rows: 779 `phase`, 93 `upload_started`/`upload_finished`, 73
`verified`, 73 `visible`, 20 `waiting`. All numbers below are computed from those rows; the scripts are not committed.

### 3a. Where the 69 min 46 s went

| part | seconds | share of the run | what it is |
|---|---|---|---|
| before the first upload | 661 (11:01) | 15.8 % | `cargo package` of the set 114.9 s + releaseant's `cargo check` of the 73 tarballs 536.6 s + about 10 s other |
| the upload phase | 3516 | 84.2 % | first `upload_started` to the last `upload_finished`, split next |

The upload phase, 3516 s, adds up exactly:

| part of the upload phase | seconds | share | detail |
|---|---|---|---|
| the 73 accepted `cargo publish` attempts | 1537 | 43.7 % | inside cargo: its own verification build 1001, packaging 173, upload 80, wait for availability 119, other 164 |
| the 20 rate-limited attempts | 319 | 9.1 % | each repackaged and rebuilt before cargo was told no: 229 s of build repeated |
| the 20 `waiting` pauses | 633 | 18.0 % | releaseant's own sleeps, each exactly the server's `retry_after` + 5 s |
| **one unexplained stall** | **984** | **28.0 %** | see 3d |
| gaps between crates | 43 | 1.2 % | |

Per-crate distributions (73 accepted attempts, seconds): wall start to finish median 17, p90 48, max 88
(`baracuda-cuda-parse`); cargo's own verification build median 9.2, p90 33.7, max 83.8; the upload itself median 1.0,
max 3.3; cargo's wait for the crate to become available median 1.6, max 7.8. releaseant's pre-upload `cargo check` per
crate: median 5.2, p90 12.4, max 48.9 (`baracuda-cuda-parse`), then `baracuda-cutlass-kernels-sys` 33.8 and
`baracuda-forge` 26.2.

### 3b. What the `waiting` events were: crates.io's rate limit, 20 times

**The task text said no 429 was seen. The ledger shows 20.** Every `waiting` event follows an `upload_finished` of
`RateLimited { retry_after: Some(..) }`, a class that the classifier produces only from cargo's HTTP 429 message
(`src/classify.rs`), and in all 20 cases `waiting` = `retry_after` + 5 s (the margin in `Options`). `retry_after` was 1
to 41 s, median 30. Each of 20 different crates was told once and succeeded on its next attempt.

- The first 429 came at 02:15:02Z, on the **47th attempt: 46 uploads had been accepted before it**, in 16.75 min
  (21.6 s per crate, limited by building, not by the registry).
- After it, the registry set the pace: consecutive accepted uploads were a median **59 s** apart (n = 25, range 13 to
  99 s, excluding the stall), about **one upload a minute**. Before it, 46 in 16.75 min was above that rate.
- 46 is what a bucket of 30 that refills at one a minute would allow in 16.75 min (30 + 16.75). That is a reading of
  this one ledger, **not** checked against crates.io's documentation, which I did not consult.

### 3c. Could any build be dropped (R8)? The numbers; the decision is the PM's

A crate is compiled up to three times: `cargo package`'s archive step (115 s for the set), releaseant's `cargo check`
of the tarballs with siblings patched in (537 s), and `cargo publish`'s own verification build with siblings from the
registry (1001 s on accepted attempts, **plus 229 s on the 20 attempts the registry then refused**). Builds total 1882 s,
31.4 min, 45 % of the run; the rest is the registry's pace and the stall.

- **The registry's pace hides the builds in the throttled phase.** From the 47th attempt on, a crate took about a minute
  whatever it built; dropping a build there shortens the waiting, not the run. In the burst phase (46 crates, 994 s)
  the inner build was 658 s, two thirds of it: that is where a build is on the critical path.
- **Option A, the cheapest and safest: after a 429, retry without repeating `cargo publish`'s verification.** The same
  tarball built seconds earlier. Saves at most the 229 s rebuilt here (3.3 min), and drops no check that has not
  already passed on identical bytes. Not implemented: the PM's R8 ruling is not to drop a check.
- **Option B: drop releaseant's own `cargo check` pass (537 s, 8.9 min) or cargo's inner verify (1001 s).** Each is the
  other's near-duplicate, but they are not the same build (siblings patched vs resolved from the registry), and either
  removal is a check dropped: not proposed.
- Fuel (43 crates) is a different shape: 0.35.2 spent 88.8 of 119.7 upload minutes in one crate; this ledger says
  nothing about it.

### 3d. The stall

The 20th and last 429 was `baracuda-flashinfer` at 02:39:53Z, `waiting 26s`. The next `upload_started` is stamped
02:56:43Z, **16 min 50 s later**: 984 s more than the sleep. Nothing in the ledger or the transcript explains it
(`[52:38]` to `[69:34]` in the transcript). A sleeping or suspended host and a stalled process look the same from here;
the Windows power log for 02:40 to 02:57 UTC would tell. It is 28 % of the upload phase and is not the tool's pacing.

### 3e. What stays unmeasured

- **The verbatim text of a crates.io "new versions" 429.** The ledger stores only the parsed `retry_after`, not cargo's
  message, so the wording (and whether it says "new crates" or "updates to existing crates") was not captured.
  Capturing it needs a small code change (log the text in a `classified` event); not made.
- A kill between upload and read-back (this run was not killed).
- A lagging index: cargo's own wait for availability took at most 7.8 s here and every crate was `visible` at once, but
  the ledger has no event for read-back retries, so a short lag would not show.
- A real transient network failure; Fuel's run.
