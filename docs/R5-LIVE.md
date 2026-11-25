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
| **a host suspend (the laptop slept)** | **984** | **28.0 %** | see 3d |
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
  99 s, excluding the suspend), about **one upload a minute**. Before it, 46 in 16.75 min was above that rate.
- 46 is what a bucket of 30 that refills at one a minute would allow in 16.75 min (30 + 16.75). That is a reading of
  this one ledger, **not** checked against crates.io's documentation, which I did not consult.

### 3c. Could any build be dropped (R8)? The numbers; the decision is the PM's

A crate is compiled up to three times: `cargo package`'s archive step (115 s for the set), releaseant's `cargo check`
of the tarballs with siblings patched in (537 s), and `cargo publish`'s own verification build with siblings from the
registry (1001 s on accepted attempts, **plus 229 s on the 20 attempts the registry then refused**). Builds total 1882 s,
31.4 min, 45 % of the run; the rest is the registry's pace and the host suspend.

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

### 3d. The stall: the laptop slept

The 20th and last 429 was `baracuda-flashinfer` at 02:39:53Z, `waiting 26s`. The next `upload_started` is stamped
02:56:43Z, **16 min 50 s later**: 984 s more than the sleep. The ledger and the transcript cannot explain it
(`[52:38]` to `[69:34]` in the transcript). **Explained by the PM from the Windows log** (Kernel-Power events 506, 507,
42 and 107 and a system time change): `Critical Battery Trigger Met` at 02:39:57Z, the machine entered sleep at
02:40:00Z and resumed at 02:40:04Z by a clock jump to 02:56:28Z. So the 26 s sleep spanned a host suspend: the stall
is the machine's, not the tool's pacing and not the registry's, and the tool carried on correctly when the machine woke
(the retry succeeded at once). It is 28 % of the upload phase. The lesson for a long publish is the power setting, not
a code change: a run of an hour or more needs a machine that cannot sleep or run flat.

### 3e. What stays unmeasured

- **The verbatim text of this run's 429s.** The ledger stores only the parsed `retry_after`, not cargo's message, so the
  wording of these 20 was not captured. The only verbatim 429 on file is the PM's sample of 2026-10-24
  (`tests/fixtures/rate_limited.txt`, "too many updates to existing crates"); it is not known whether these 20 said the
  same. A following release logs the message in a `classified` event so the next run keeps it.
- A kill between upload and read-back (this run was not killed).
- A lagging index: cargo's own wait for availability took at most 7.8 s here and every crate was `visible` at once, but
  the ledger has no event for read-back retries, so a short lag would not show.
- A real transient network failure; Fuel's run (section 4) had none either.

## 4. The real Fuel publish (2026-11-02), read from its ledger

Source: `docs/rehearsal/fuel-2026-11-02-ledger.jsonl`, `...-out.txt` and `...-out.waiting-ci.txt`, copied unedited (checked
byte-identical with `cmp`) from `C:/Projects/.lane-state/fuel-pub/` (run by the PM). Run
`9dc33d61-1793593061-21540-0`, commit `9dc33d611d5380210b429de5c63e41b03524439c`, 43 crates at `0.36.4`, **one run,
finished `complete`, 46 min 24 s** (2784 s) by the transcript's clock. Flags as the PM gave them: `--retries 8
--retry-pause 90 --max-wait 21600 --reuse-verified-hours 12`, `DOCS_RS=1`, releaseant 0.0.53 (not readable from the
ledger). 563 ledger rows: 389 `phase`, 43 each of `verified`, `upload_started`, `upload_finished` and `visible`, one
`planned`, one `finished`; **no `waiting` row and no `classified` row**. All numbers are computed from those rows; the
scripts are not committed.

### 4a. Was there a 429? No

All 43 `upload_finished` rows are `Accepted`. The run met no rate limit, so no attempt was repeated and the largest gap
between one crate finishing and the next starting was 1 s. The 43 accepts took 29.5 min (first 04:34:38Z, last
05:04:07Z): the registry never set the pace here.

- All 43 were new versions of crates already on crates.io (each crate has earlier versions in the crates.io API). I did
  not check whether baracuda's were the same kind.
- Fuel's fastest 30 consecutive accepts took 972 s (16.2 min); overall 1.46 accepts a minute. Baracuda took 46 in
  16.75 min before its first 429 (section 3b), so Fuel never went as fast.
- Replaying the 43 accept times through the bucket reading of 3b (30 tokens, one back a minute) leaves at least 13.6
  tokens at every point. That is **consistent with no 429, and it is not a test of that reading**: 43 slower uploads
  cannot tell it from other rules. No crates.io rate is quoted here as documented.
- Independent check: crates.io's own `created_at` for `0.36.4` of each of the 43 crates is within -3 to +2 s of the
  ledger's `upload_finished` time (43 of 43), two clocks that do not share a method.

### 4b. Where the 46 min 24 s went

| part | seconds | share of the run | what it is |
|---|---|---|---|
| before the first upload | 1007 (16:47) | 36.2 % | `cargo package` of the set 49.0 s + releaseant's `cargo check` of the 43 tarballs 936.2 s (the ledger's `verify all` says 939.7) + about 22 s other |
| the upload phase | 1778 | 63.9 % | first `upload_started` to the last `upload_finished`, split next |

The upload phase, 1778 s, adds up exactly:

| part of the upload phase | seconds | share | detail |
|---|---|---|---|
| cargo's own verification build | 1514.9 | 85.2 % | inside `cargo publish`, from `Verifying` to `Finished` |
| cargo's packaging | 74.1 | 4.2 % | `Packaging` to `Verifying` |
| the upload and the wait for availability | 101.6 | 5.7 % | `Uploading` to `Published`: median 2.3 s a crate, max 4.5 |
| other inside cargo | 68.4 | 3.8 % | |
| gaps between crates | 19 | 1.1 % | |

**Builds are 2500 s, 41.7 min, 89.8 % of the run** (package 49 + releaseant's check 936 + cargo's verify 1515); the
registry's own work, the upload and its availability, is 101.6 s, 3.6 %. Baracuda's builds were 45 % of its run
because its rate limit and a suspend took the rest (3c); with neither, Fuel's time is nearly all compiling. No
attempt was repeated, so there is no rebuilt-after-429 cost here and R8 option A would have saved nothing on this run.
`--reuse-verified-hours 12` did not engage: all 43 `verified` rows are fresh (there was no earlier ledger).

Per crate (43 accepted attempts, seconds): wall start to finish median 24, p90 108, max 153; releaseant's pre-upload
`cargo check` median 11.0, max 238.1.

### 4c. Which crates dominated

- Upload wall time: `fuel-examples` 153, `fuel-datasets` 139, `fuel-tensor` 138, `fuel-lazy-examples` 134,
  `fuel-tensor-tools` 108. Those five are 672 s, 38.2 % of the 1759 s of attempts.
- Pre-upload `cargo check`: `fuel-datasets` 238.1 s (25.4 % of the 936.2 s), `fuel-examples` 84.5,
  `fuel-vulkan-backend` 51.3, `fuel-tensor` 51.1, `fuel-lazy-examples` 41.2. The first two are 34.5 %.
- `fuel-datasets` is the largest in both passes: 238.1 + 139 = 377 s, 13.5 % of the whole run. The ledger does not say
  why it is slow, and this section does not guess.
- `fuel-cuda-backend`, the crate that took 88.8 of 119.7 upload minutes at 0.35.2 (an earlier measurement, not
  re-read here), took 63 s wall here (32.7 s pre-check). The run as a whole took 29.5 min of upload against 119.7 at
  0.35.2; the cause of the difference is not in this ledger.

### 4d. No stall

Section 3d's suspend does not appear. The largest gap between attempts is 1 s, there is no sleep, and the transcript's
one long step (`[02:26]` to `[06:24]`, 4 min) is `fuel-datasets` being checked, which the ledger records as 238.1 s.

### 4e. `--require-green` against a real repository

The PM dropped `--require-green` for this run. The transcript kept from the attempt (`...-out.waiting-ci.txt`) is one
line, `waiting for CI on ciresnave/fuel 9dc33d61`: the guard waited and nothing came after it. Read from the forge at
2026-11-02T05:16Z (`gh run list -R ciresnave/fuel --commit 9dc33d61...`), the commit has 11 runs: 9 `completed` with
conclusion `success` (`Continuous integration`, `lavapipe-vulkan`, two `Secret Leaks`, five Dependabot updates) and 2
`queued` with no conclusion, **created 03:57:18Z, so 79 min queued at that reading**:

| run | its one job | status |
|---|---|---|
| `checkpoint-tests` | `Qwen3.5-0.8B Gated DeltaNet oracles (self-hosted)` | queued |
| `cuda-metal-compile-check` | `cargo check (cuda, jit) on the self-hosted runner` | queued |

`src/ci.rs` counts every run on the commit and returns `Green` only when none is unfinished, so two runs that wait for
a self-hosted runner end every `--require-green` in exit 4 until the runner appears or `--ci-wait` runs out. That is
the designed "no answer is not green", but it makes the guard unusable on a repository with a self-hosted job that
does not gate the merge. Fuel's `main` protection (`enforcement_level` `non_admins`) requires six checks: `Clippy`,
`Rustfmt`, `trufflehog`, `Check (ubuntu-latest)`, `Test Suite (ubuntu-latest)`, `Packaged crates`. Neither queued run's
name nor its job's name is among them.

The options, for the PM to decide (not implemented):

1. **Ignore runs that never started within the wait.** Smallest change, and it fails open: a required check that is
   queued because its runner is down would count as green. Not recommended.
2. **`--require-checks <name>,...`: judge only the named checks.** Any named check that is missing or unfinished is "no
   answer", any that failed is red, the rest of the runs are ignored. Fails closed, explicit, and the default (every
   run) is unchanged. It needs the check names, so the probe moves from `gh run list` (workflow-run names) to the
   check-runs of the commit, whose names are what branch protection lists.
3. **Take the names from the branch protection of the base branch** (`required_status_checks.contexts`, read-only) when
   `--require-checks` is not given, falling back to today's behaviour when none are configured. The right default for a
   gate, since it is the list the forge itself enforces; it needs read access to protection settings, which `gh`'s
   login may not have on every repository.

Recommendation: 2, then 3 if the PM wants the default to follow the forge. Either way, a never-started required check
must stay "no answer".

### 4f. What stays unmeasured

- **The verbatim wording of a real 429 in a ledger.** Fuel met none, and ran 0.0.53, which has no `classified` event.
  The `classified` event of 0.0.54 has still not met the real registry.
- A kill between upload and read-back (this run was not killed).
- A lagging index: cargo's wait for availability took at most 4.5 s and every crate was `visible` at once.
- A real transient failure: neither real run had one, so `--retries` has only the forced failure of 2b behind it on
  the real registry.
- Where crates.io's limit lies. Baracuda reached it at the 47th attempt in 16.75 min; Fuel's 43 uploads did not (nor did the second run's, section 5).

## 5. The second real Fuel publish (2026-11-02, 0.36.7), against run 1

Source: `docs/rehearsal/fuel-2026-11-02-run2-ledger.jsonl`, `...-run2-out.txt` and `...-run2-out.empty-names.txt`, copied
unedited (`cmp` identical) from `C:/Projects/.lane-state/fuel-pub2/` (run by the PM). Run
`79b2a83c-1793608405-180060-0`, commit `79b2a83c`, 43 crates at `0.36.7`, **one run, finished `complete`, 61 min 34 s**
(3694 s) by the transcript's clock, built with releaseant 0.0.56 and `DOCS_RS=1`. The PM reports other lanes were loading
the box heavily (2 GB of 64 free); run 1 (section 4) was on a quiet box. 563 ledger rows, the same count and the same
kinds as run 1: 389 `phase`, 43 each of `verified`, `upload_started`, `upload_finished`, `visible`, one `planned`, one
`finished`; **no `waiting` and no `classified` row**. Both runs were analysed with the same script on the same fields,
so the numbers below are comparable; the scripts are not committed.

### 5a. Was there a 429? No, again

All 43 `upload_finished` rows are `Accepted`, the largest gap between attempts was 1 s, and nothing waited. Run 2 was
slower, so it asked the registry less often: 43 accepts in 39 min 5 s (1.07 a minute against 1.42 in run 1), the fastest
30 consecutive in 1436 s (972 s in run 1). Two runs without a 429 at 43 uploads, both slower than baracuda's burst,
still say nothing about where the limit lies (3b has the only 429s measured).

### 5b. Where the 15 min 10 s went

| part | run 1 (0.36.4, quiet) | run 2 (0.36.7, loaded) | change |
|---|---|---|---|
| whole run | 2784 s | 3694 s | **+910 s (+32.7 %)** (the parts are rounded to the second) |
| before the first upload | 1007 s | 1334 s | +327 s |
| `cargo package` of the set | 49.0 s | 53.8 s | +4.8 s |
| releaseant's `cargo check` x43 | 936.2 s | 1260.0 s | +323.8 s (1.35x) |
| the upload phase | 1778 s | 2359 s | +581 s |
| cargo's own verification build | 1514.9 s | 2060.8 s | +545.9 s (1.36x) |
| cargo's packaging | 74.1 s | 81.7 s | +7.6 s |
| **upload and wait for availability** | **101.6 s** | **100.8 s** | **-0.8 s** |

Builds (package, check, cargo's verify) are 3374.6 s, 91.4 % of run 2, against 89.8 % of run 1. **Of the 910 s, 874.5
(96 %) is builds. The registry's part did not move**: the median upload plus availability is 2.3 s in both runs and the
longest 4.5 s against 4.0 s. The slowdown is on this side of the network.

It was not uniform. The median crate took 1.35x as long to `cargo check` and 1.38x as long to publish, but the ratio
ranges from 0.22x to 3.59x for the check, and `fuel-tensor` (0.9x) and `fuel-mkl-cpu-backend` (1.0x) were no slower. The
five crates that grew most, in seconds of check plus publish, account for 600 of the 910:

| crate | check, run 1 to 2 | publish wall, run 1 to 2 |
|---|---|---|
| `fuel-cuda-backend` | 32.7 to 64.2 s | 63 to **322 s** (5.1x) |
| `fuel-loaders` | 19.0 to 34.3 s | 35 to 118 s |
| `fuel-aocl-cpu-backend` | 23.2 to 83.3 s | 47 to 71 s |
| `fuel-cpu-backend` | 26.7 to 31.5 s | 53 to 114 s |
| `fuel-datasets` | 238.1 to 239.9 s | 139 to 199 s |

`fuel-cuda-backend` alone is 259 s of the 910 (28 %). Its cargo status lines locate it: `Compiling baracuda-build
v0.1.0-alpha.80` to `Finished` took 54.9 s in run 1 and **296.6 s** in run 2. The dependency is the same version in both
runs, so that crate's extra time is not a change in what was compiled. `fuel-datasets`, the slowest check in both runs,
did not slow down at all (238.1 s to 239.9 s) but its publish build did (135.2 s to 191.5 s).

### 5c. What this does and does not establish

- **Established:** zero 429s again; the registry's share of the time unchanged to within a second; the extra time is
  entirely local builds, and uneven across crates.
- **Not established: the cause.** The load on the box is the PM's report, not a figure in either ledger (the ledger
  records no CPU or memory). The sources also differ (0.36.4 and 0.36.7 are different commits), so a code change is
  not excluded for any crate, only for `fuel-cuda-backend`'s `baracuda-build` step, whose version did not change. "Load"
  is the explanation consistent with a same-version dependency taking 5.4x longer and with other crates not slowing at
  all; it is not measured here.
- **For planning:** a Fuel publish took 46 to 62 min on this box, so `--max-wait 21600` was far more than needed, and
  a machine that cannot sleep matters for a run of an hour.

### 5d. The guards, as the PM ran them

- **`--require-green` with the default rule (0.0.56).** The transcript's first lines are `judging the 6 check(s) that
  branch protection of main requires: Clippy, Rustfmt, trufflehog, Check (ubuntu-latest), Test Suite (ubuntu-latest),
  Packaged crates` and `CI is green on 79b2a83c`. This is the case section 4e was written for, in a real publish: the
  two queued self-hosted runs (not required) did not hold it up.
- **An empty `--require-checks` was judged as one check with no name (fixed in 0.0.59).** `...-run2-out.empty-names.txt` is `judging the 1
  check(s) named by --require-checks: ` and nothing after it: the flag was given an empty value (an unset shell variable, most
  likely) and the guard looked for a check called `""`, which never exists, so it could only wait out `--ci-wait` and fail with exit
  4. That failed closed but late and without a reason. Now an empty entry (`""`, `a,,b`, or only spaces) is refused before anything
  waits: exit 2, `--require-checks: entry N is empty`. No names at all still means the branch-protection rule.

### 5e. What stays unmeasured

As in 4f: a real 429's wording (none met in 43 + 43 uploads), a kill between upload and read-back, a lagging index (the
longest availability wait was 4.0 s), a real transient failure. New: nothing in the ledger says how loaded the machine
was, so a future run that wants this comparison needs the load recorded beside it.

## 6. The httpant 0.9.0 publish (2026-11-23), reconstructed from the registry: there is no run ledger

**Read this first.** This section is not read from a ledger. The PM ran `workspace-publish.py` without redirecting its
output, saw only its last lines, and kept no ledger and no out file (PM, 2026-11-23, `[FINDING]` httpant-0.9.0-ledger).
Nothing here is a measurement of the tool: no per-crate timing, no `phase` events, no check of whether a 429 was met.
What is below is the registry's own record, which this lane re-read on 2026-11-23, plus the PM's own account of the
tail of the run. The next multi-crate publish keeps its out file and ledger (the PM's word), and gets a section like 3
to 5.

**What the PM reported** (the last lines of the run, not re-verified by this lane): 4 crates to reach 0.9.0; round 1:
0 published, 4 to go; round 2: 4 published, 0 to go; `READ BACK 4/4`. Published from a clean archive of commit
`0b0fcca6f3000a3e91573089afaf414df5afd3f1`. `httpant-conformance` and `httpant-bench` are `publish = false` and were not
part of it.

**What the registry says**, read with `GET https://crates.io/api/v1/crates/<crate>/0.9.0` on 2026-11-23 (field
`version.created_at`, `crate_size`, `checksum`; all four `yanked: false`, `rust_version` `1.89`):

| crate (dependency order) | `created_at` (UTC) | `crate_size` | checksum (sha256) |
|---|---|---|---|
| `httpant-core` | 2026-11-23T01:56:00.04Z | 24804 B | `c53182aecc984518d6cb148e2d58ff1be276b2b5521ef8bedfc0e70c6e53f726` |
| `httpant-client` | 2026-11-23T01:56:02.14Z | 33664 B | `80bdea1927ca6d129f79f10572db0d0fd6aae0a5c216e8d3570a8b1921e49424` |
| `httpant-server` | 2026-11-23T01:56:04.12Z | 101614 B | `744f68a0aac89a3b6404c57a701b406d35746341c6926a878733bd6855309bfd` |
| `httpant` | 2026-11-23T01:56:06.31Z | 59575 B | `e05a521c4b68e1a47b27f04915c3bfabee58d1c5f20ac30588a97fb784a3e919` |

The tag `v0.9.0` on `ThinkersJournal/httpant` is annotated and peels to `0b0fcca6f3000a3e91573089afaf414df5afd3f1`
(`git ls-remote --tags`), the commit the PM named.

**What this does and does not establish.**
- All four crates are on the registry at 0.9.0, not yanked, in dependency order, about 2 s apart (2.10, 1.98, 2.19 s).
  That is consistent with the PM's `READ BACK 4/4` and says the order was right.
- The 2 s spacing is the gap between registry creation times. It is not the time `cargo publish` took, and it is not
  a rate-limit pause (after the limit, section 3b saw about one upload a minute).
- "Two rounds, round 1 published nothing" is the PM's reading of the script's output; the registry cannot confirm or
  explain it. Why round 1 published none of the four is not known from anything this lane holds.
- The checksums are the registry's. Comparing them to a build of `0b0fcca6` would show whether the uploaded archives match
  the commit; this lane did not build it, so that comparison is not made here.
- Unmeasured, as in 5e, and still so: a real 429's wording, a kill between upload and read-back, a lagging index.

## 7. The aocl 0.2.2 publish (2026-11-24), from cargo's own output and the registry

**Read this first.** The file is **not a releaseant ledger**. `C:/Projects/.lane-state/publish-aocl-0.2.2.out` is the
raw stdout and stderr of one multi-crate `cargo publish` (the PM, via pm-kernels, 2026-11-24: "one cargo invocation for 17
crates", from a clean archive of `main` `69fee1bd`, `DOCS_RS=1`). It has no `phase`, `waiting` or `classified` events and no
`--require-green`, so none of the section 3 to 5 measurements (where the time went per phase, builds against registry
time) can be made from it. What it does have is cargo's status lines and one wall-clock header and footer. This lane read
all 224 lines, then re-read the registry itself (method below). The out file is not copied into the repo.

**Lines relied on.** Line 1: `publish start 01:24:24Z sha 69fee1bd28c52f4cb303b53300a528e204042b20 version 0.2.2` and the
17 crate names. Line 224: `exit=0 end 01:25:16Z`. Lines 3 to 52: 17 `Packaging` (17 `Packaged`). Lines 53 to 180: 17
`Verifying` and 17 `Finished`. Lines 181 to 220: 17 `Uploaded`. Lines 187, 204, 221: the three `waiting for` notes. Lines
189, 206, 223: the three `Published` lines. No line contains `429`, `retry` or `failed` (grep over the whole file: 0 hits; control: the same file gives 17 `Uploaded`
and 14 `warning:` lines, which are present). Line 222 is cargo's `press ctrl-c to skip waiting` hint, not a wait event.

**What the run did.**

| round | crates | `waiting for` note | `Published` line |
|---|---|---|---|
| 1 | `aocl-build`, `aocl-error`, `aocl-types` (3) | line 187, "14 remaining" | 189 |
| 2 | the seven `-sys` crates (7) | line 204, "7 remaining" | 206 |
| 3 | `aocl-compression`, `-crypto`, `-data-analytics`, `-scalapack`, `-securerng`, `-sparse`, `-utils` (7) | line 221 | 223 |

3 + 7 + 7 = 17, uploaded in dependency layers; cargo waited for each layer to be visible before starting the next, and
each wait ended in a `Published` line. Exit 0. Whole run **52 s** by the file's own clock (01:24:24Z to 01:25:16Z). In the
file's order the packaging and the 17 verification builds come first and the uploads after all of them. Their registry
timestamps start at 01:25:01, 37 s after the start header, so the builds took about that much of the 52 s; the file has no
timestamp per line, so the split inside it is not measured. (The `Finished ... in N s` figures rise from 10.7 to 37.3 s
down the file, which reads as one running clock, not 17 separate build times; not established.)

**A 429? None in this output.** All 17 uploads went through with no wait and no retry text, at the same short pace as
section 4a and 5a: the registry's `created_at` times (below) span 01:25:01.4 to 01:25:13.3, **17 versions in 11.9 s**.
That is fewer than the 46 uploads baracuda had accepted before its first 429 (section 3b), so it is no evidence about where the
limit lies; it is a statement about this output, not about the registry's rule.

**What the registry says**, read by this lane on 2026-11-24 with `GET https://crates.io/api/v1/crates/<crate>/0.2.2` (fields
`version.created_at`, `crate_size`, `yanked`, `checksum`). All 17 answered, all `yanked: false`. This is a method
different from the out file (the registry's record, not cargo's), so agreement between the two is evidence:

| round | crate | `created_at` (UTC, 2026-11-24) | `crate_size` |
|---|---|---|---|
| 1 | `aocl-build` | 01:25:01.44 | 13057 B |
| 1 | `aocl-error` | 01:25:01.96 | 6967 B |
| 1 | `aocl-types` | 01:25:02.36 | 7803 B |
| 2 | `aocl-compression-sys` | 01:25:04.00 | 16756 B |
| 2 | `aocl-crypto-sys` | 01:25:04.65 | 13884 B |
| 2 | `aocl-data-analytics-sys` | 01:25:05.28 | 16448 B |
| 2 | `aocl-scalapack-sys` | 01:25:05.87 | 10768 B |
| 2 | `aocl-securerng-sys` | 01:25:06.40 | 9026 B |
| 2 | `aocl-sparse-sys` | 01:25:07.00 | 14625 B |
| 2 | `aocl-utils-sys` | 01:25:07.61 | 13535 B |
| 3 | `aocl-compression` | 01:25:09.65 | 11650 B |
| 3 | `aocl-crypto` | 01:25:10.09 | 19988 B |
| 3 | `aocl-data-analytics` | 01:25:11.01 | 30149 B |
| 3 | `aocl-scalapack` | 01:25:11.65 | 21122 B |
| 3 | `aocl-securerng` | 01:25:12.21 | 9710 B |
| 3 | `aocl-sparse` | 01:25:12.74 | 34664 B |
| 3 | `aocl-utils` | 01:25:13.31 | 11900 B |

Checksums (sha256) are in the API response; they are not tabulated here and were not compared to a build of `69fee1bd`
(this lane did not build it). Cargo reports the packaged sizes compressed (for example `aocl-build` 12.8 KiB, line 5) and the
registry `crate_size` is the same archive (13057 B = 12.75 KiB), consistent with the upload being what was packaged.

The three rounds line up with the creation times: round 1's last crate (01:25:02.4) precedes round 2's first
(01:25:04.0), and round 2's last (01:25:07.6) precedes round 3's first (01:25:09.6). Each gap is about 1.6 to 2.0 s, which
is the visibility wait cargo reported. The order inside a round is not the order on the packaging lines (line 3 onward);
it is the upload order.

**Tag.** `git ls-remote --tags https://github.com/ThinkersJournal/aocl 'v0.2.2*'` gives `refs/tags/v0.2.2` `ad10a448...`
(a tag object, so annotated) and `refs/tags/v0.2.2^{}` `69fee1bd28c52f4cb303b53300a528e204042b20`, the commit the
header names.

**Controls.** The PM re-read 17 of 17 and reports held `aocl-math-sys` 0.0.0 and `aocl` 0.1.0 as controls (the PM's
account, not re-run here). This lane's own read: 17 crates, 17 hits, each with a `created_at` on 2026-11-24 inside the run's
window (01:24:24Z to 01:25:16Z); a crate not in the run would have been absent or earlier, and one 404 would have
printed an error (none did). **Not run here:** a read of a held crate as a control. The 17-of-17 is therefore this lane's
read of the named 17 only; "nothing else was published" rests on the PM's controls.

**A finding that is not releaseant's.** Seven `-sys` crates print `warning: unused dependency libc` at verification
(lines 59, 79, 96, 116, 133, 150, 167): `aocl-compression-sys`, `-crypto-sys`, `-data-analytics-sys`, `-scalapack-sys`,
`-securerng-sys`, `-sparse-sys`, `-utils-sys`. It is cargo's `unused_dependencies` lint (set to `warn`) on a `[dependencies.libc]`
table, harmless to the publish (exit 0) and aocl's to fix (a dependency that is unused ships to every consumer). It
matters here only because a `--require-green` or a clean-build check that denies warnings would have stopped this run.

**What this adds, and what it does not.**
- Added: one more real multi-crate publish with no 429, the first with the layers (3, 7, 7) visible in the output, and the
  first where cargo's own multi-crate publish did the upload and the wait (sections 3 to 5 are releaseant's own).
- Not added: anything about releaseant's guards, ledger or retries; no phase timing; no check that the archives match the
  commit.
- Still unmeasured, as in 5e: a real 429's wording, a kill between upload and read-back, a lagging index (the longest
  gap here, about 2 s, was cargo's own wait, and it ended on its own), a real transient failure.

## 8. The baracuda 0.1.0-alpha.183 publish (2026-11-24), from cargo's own output and the registry

**Read this first.** `C:/Projects/.lane-state/publish-baracuda-alpha183.out` is the raw output of one multi-crate
`cargo publish -p <x14>` run (the PM's account, relayed by pm-ants: 14 crates, `DOCS_RS=1`, tag `v0.1.0-alpha.183` on
`bef4be0a3`). It is **the PM's file, not pm-assist's**; sections 3 to 5 read a releaseant ledger and section 7 read a file
with a start header and an `exit=` footer, and this file differs from both: it is 148 lines of cargo output only, with
**no `publish start` header, no `exit=` footer, no sha line and no crate-list line**. So the invocation, the exit code and
the commit are the PM's statement, not something the file shows. What the file shows is cargo's own status lines. This lane
read all 148 lines, then re-read the registry and the tag itself (method below). The file is not copied into the repo.

**Lines relied on.** Lines 2 to 43: 14 `Packaging` (14 `Packaged`). Lines 44 to 95: 14 `Verifying` and 14 `Finished`. Lines 96
to 147: 14 `Uploaded`, 8 `waiting for` notes (106, 111, 116, 123, 128, 135, 140, 145) and 8 `Published` lines (108, 113, 118, 125,
130, 137, 142, 147). Grep over the whole file for `429`, `retry`, `failed`, `error`, `warning`: **0 hits each**; control: the
same file gives 14 `Uploaded` and 8 `Published`, which are present, and the aocl file of section 7 gave 14 `warning:` lines
under the same grep, so the pattern does find warnings when a run has them. Line 79 is `Blocking waiting for file lock on
package cache` (cargo's own, during the `baracuda-cuda-emit` verify; harmless, not a retry). Line 146 is cargo's `press ctrl-c to
skip waiting` hint, not a wait event.

**What the run did.** 11 crates at `0.1.0-alpha.183` and 3 at `0.56.5` (`baracuda-cuda-parse`, `-cuda-emit`, `-cuda-seam`); 11 + 3 =
14, matching the PM's count. Cargo packaged all 14, verified all 14, and only then uploaded, in 8 dependency layers:

| round | crates | `waiting for` note | `Published` line |
|---|---|---|---|
| 1 | `baracuda-build`, `-cuda-parse`, `-cuda-vocab`, `-seam`, `-types-derive` (5) | line 106, "9 remaining" | 108 |
| 2 | `baracuda-types` | line 111, "8 remaining" | 113 |
| 3 | `baracuda-core` | line 116, "7 remaining" | 118 |
| 4 | `baracuda-cuda-sys`, `-nvrtc-sys` (2) | line 123, "5 remaining" | 125 |
| 5 | `baracuda-driver` | line 128, "4 remaining" | 130 |
| 6 | `baracuda-kernels-types`, `-nvrtc` (2) | line 135, "2 remaining" | 137 |
| 7 | `baracuda-cuda-emit` | line 140, "1 remaining" | 142 |
| 8 | `baracuda-cuda-seam` | line 145 (no remaining count) | 147 |

5 + 1 + 1 + 2 + 1 + 2 + 1 + 1 = 14. Each wait ended in a `Published` line and the next layer started after it. The `Finished ... in
N s` figures rise from 10.4 to 30.9 s down the file, one running clock again, not 14 build times (not established, as in
section 7). The file has no per-line timestamps, so the time split between building and registry waits inside the run is not
measured. The file's modification time is 10:51:34 UTC and the last registry `created_at` below is 10:51:32.9, consistent with a
run that ended about when its last crate appeared.

**A 429? None in this output.** All 14 uploads went through with no wait and no retry text. The registry's `created_at` times
(below) span 10:51:15.2 to 10:51:32.9, **14 versions in 17.6 s**. That is fewer than the 46 uploads baracuda had accepted before
its first 429 in section 3b, so it is no evidence about where the limit lies; it is a statement about this output.

**What the registry says**, read by this lane on 2026-11-24 with `GET https://crates.io/api/v1/crates/<crate>/<version>` (fields
`version.created_at`, `crate_size`, `yanked`). All 14 answered, all `yanked: false`. This is a method different from the out
file (the registry's record, not cargo's), so agreement between the two is evidence:

| round | crate | version | `created_at` (UTC, 2026-11-24) | `crate_size` |
|---|---|---|---|---|
| 1 | `baracuda-build` | 0.1.0-alpha.183 | 10:51:15.24 | 12048 B |
| 1 | `baracuda-cuda-parse` | 0.56.5 | 10:51:16.00 | 15694 B |
| 1 | `baracuda-cuda-vocab` | 0.1.0-alpha.183 | 10:51:16.61 | 19138 B |
| 1 | `baracuda-seam` | 0.1.0-alpha.183 | 10:51:17.30 | 8442 B |
| 1 | `baracuda-types-derive` | 0.1.0-alpha.183 | 10:51:17.79 | 11233 B |
| 2 | `baracuda-types` | 0.1.0-alpha.183 | 10:51:19.33 | 22883 B |
| 3 | `baracuda-core` | 0.1.0-alpha.183 | 10:51:21.41 | 17405 B |
| 4 | `baracuda-cuda-sys` | 0.1.0-alpha.183 | 10:51:23.91 | 106850 B |
| 4 | `baracuda-nvrtc-sys` | 0.1.0-alpha.183 | 10:51:24.92 | 12212 B |
| 5 | `baracuda-driver` | 0.1.0-alpha.183 | 10:51:26.34 | 115749 B |
| 6 | `baracuda-kernels-types` | 0.1.0-alpha.183 | 10:51:28.57 | 23844 B |
| 6 | `baracuda-nvrtc` | 0.1.0-alpha.183 | 10:51:29.19 | 12718 B |
| 7 | `baracuda-cuda-emit` | 0.56.5 | 10:51:30.57 | 316728 B |
| 8 | `baracuda-cuda-seam` | 0.56.5 | 10:51:32.86 | 25062 B |

The upload order matches the table in every round; each round's last crate precedes the next round's first. Packaged sizes agree:
`baracuda-build` is 11.8 KiB compressed on line 4 and 12048 B (11.77 KiB) in the registry; `baracuda-cuda-emit` 309.3 KiB on line
31 and 316728 B (309.3 KiB).

**Tag.** `git ls-remote --tags https://github.com/ThinkersJournal/baracuda 'v0.1.0-alpha.183*'` gives `refs/tags/v0.1.0-alpha.183`
`3fe5304e...` (a tag object, so annotated) and `refs/tags/v0.1.0-alpha.183^{}` `bef4be0a3fd099f9bfa66f3ebd7bc7f053cc7e7f`, the
commit the PM named.

**Controls.** This lane's own read: 14 crates, 14 hits, each with a `created_at` inside the 17.6 s window above. The same query for
a version outside the run, `baracuda-cuda-sys` `0.1.0-alpha.182`, returns `does not have a version`, so the read does tell a
published version from an absent one. **Not run here:** a read of a crate the run was meant to hold back; "nothing else was
published" rests on the PM's controls, which this lane has not seen. The archives were not compared to a build of `bef4be0a3`
(checksums not compared).

**What this adds, and what it does not.**
- Added: one more real multi-crate `cargo publish`, with no 429, the first with layers of width 5, 1, 1, 2, 1, 2, 1, 1 (a
  chain, not three layers as in section 7), and a read of an out file that has no header or footer.
- Not added: anything about releaseant's guards, ledger or retries; no phase timing; no exit code from the file; no check that
  the archives match the commit.
- Still unmeasured, as in 5e: a real 429's wording, a kill between upload and read-back, a lagging index (the longest gap here,
  about 2.5 s between rounds 3 and 4, was cargo's own wait, ending on its own), a real transient failure.
- The onemkl 0.3.8 publish is section 9.

## 9. The onemkl 0.3.8 publish (2026-11-24), from cargo's own output and the registry

**Read this first.** `C:/Projects/.lane-state/publish-onemkl-0.3.8.out` is the PM's raw output of one `cargo publish` of 2 crates
(the PM's account, relayed by pm-ants: exit 0, readback and tag `v0.3.8` read back). Like section 8's file it is cargo output
only: 51 lines, **no start header, no `exit=` footer, no sha line**, so the invocation and the exit code are the PM's statement.
This lane read all 51 lines (the file was read after it was complete; its modification time, 11:01:42 UTC, is 2 s after the last
registry `created_at` below), then re-read the registry and the tag.

**Lines relied on.** Lines 2 to 7: 2 `Packaging` (2 `Packaged`: `onemkl-sys` 10.8 MiB, 309.2 KiB compressed; `onemkl` 1.1 MiB,
208.7 KiB compressed). Lines 8 to 40: 2 `Verifying` and 2 `Finished`. Lines 41 to 51: 2 `Uploaded`, 1 `waiting for` note (line 43)
and 2 `Published` lines (46, 51). Grep over the whole file for `429`, `retry`, `failed`, `error`: **0 hits each**; control: the same
file gives 2 `Uploaded` and 2 `Published`, which are present. Four lines begin `warning`: cargo's `redundant_homepage` manifest lint,
once per crate (lines 9, 20, 24, 35), harmless to the publish and onemkl's to fix (`package.homepage` equals `package.repository`).
Line 45 is `Blocking waiting for file lock on package cache`, as in section 8: cargo's own, not a retry. Line 50 is
cargo's `press ctrl-c to skip waiting` hint.

**What the run did.** Two crates, two layers: `onemkl-sys` first (line 41 to 46, "1 remaining crate" on line 43), then `onemkl`
(lines 47 to 51). Both verified before either uploaded, and this is the first file here where the verify is **long**:
`onemkl-sys` `Finished ... in 4m 55s` (line 22) and `onemkl` `Finished ... in 9m 38s` (line 40). Unlike the baracuda and aocl
files these are not a rising running clock: they are two separate builds, and the second one is longer because it is the
dependent. Line 37 shows how cargo verified `onemkl` before `onemkl-sys` 0.3.8 existed on the registry: `Unpacking onemkl-sys v0.3.8
(registry ... tmp-registry)`, a throwaway local registry holding the just-packaged `onemkl-sys`, and line 39 then compiles
`onemkl-sys v0.3.8` again from it, which is part of the 9m 38s. That is cargo's own mechanism for a multi-crate publish and
needs nothing from releaseant; the registry was never consulted for `onemkl-sys` 0.3.8 at verify time. The two builds
(about 14.5 minutes together) dwarf the uploads (3.7 s apart, below), the opposite of baracuda's 14 crates, where all verifies fit in
about 31 s on the file's own clock.

**A 429? None in this output.** Two uploads, no wait, no retry text. This says nothing about where the limit lies.

**What the registry says**, read by this lane on 2026-11-24 with `GET https://crates.io/api/v1/crates/<crate>/0.3.8`:

| round | crate | `created_at` (UTC, 2026-11-24) | `crate_size` | `yanked` |
|---|---|---|---|---|
| 1 | `onemkl-sys` | 11:01:36.83 | 316574 B | false |
| 2 | `onemkl` | 11:01:40.57 | 213714 B | false |

`onemkl-sys` precedes `onemkl` by 3.7 s, which covers cargo's visibility wait. The packaged sizes agree: 309.2 KiB on line 4 is 316574 B
(309.2 KiB), 208.7 KiB on line 7 is 213714 B (208.7 KiB). The registry also answers for what the run could not have published: **`onemkl`
`0.3.9` returns HTTP 404**, so the read tells a published version from an absent one. (A read of `onemkl` `0.3.7` as a second control returned an empty body
on the first attempt and was not repeated, so it is not used.)

**Tag.** `git ls-remote --tags https://github.com/ThinkersJournal/onemkl 'v0.3.8*'` gives `refs/tags/v0.3.8` `f3c85b5c...` (a tag
object, so annotated) and `refs/tags/v0.3.8^{}` `353ee2e9143b4a6452b5b15471adfa4b4623d33d`. This lane was not given the commit the PM
meant, so it does not say the tag is on the right one; the file has no sha line to check it against.

**What this adds, and what it does not.**
- Added: a two-crate dependent publish whose verify step takes minutes, not seconds, and a window in which cargo's tmp-registry
  overlay is visible in the output (line 37). No 429, again.
- Not added: anything about releaseant's guards, ledger or retries; no exit code from the file; no check that the archives match
  the commit; no read of a held-back crate. The earlier "still unmeasured" list (a real 429's wording, a kill between upload and
  read-back, a lagging index, a real transient failure) is unchanged.

## 10. The baracuda 70-crate publishes of 2026-11-25 (alpha.186, .188, .191, .193, .195): the first real 429s, read from out files and the registry

**Read this first.** Five raw output files in `C:/Projects/.lane-state/` record five publishes of the same 70-crate baracuda workspace on 2026-11-25, each driven by a hand-written retry loop that is **not releaseant** (it sleeps 30 to 77 s after a 429 and re-runs cargo; releaseant's guards, ledger and `--retries` were not involved). They are the PM's and baracuda's files, not this lane's, and none has a `sha` line or an exit code from cargo itself, only the loop's own `ALL AT <version>` and `loop done ok` lines. This lane read each file whole (the alpha.186 first-run file `publish-baracuda-rest-alpha186.out`, 500 lines, and the loop files `publish-baracuda-rest-alpha186-loop.out`, `publish-baracuda188-loop.out`, `publish-baracuda191.out`, `publish-baracuda193.out`, `publish-baracuda195.out`), then re-read the registry for all five versions. The files are not copied into the repo.

**The one verbatim 429.** Only the alpha.186 first-run file has the whole text, at line 499 (cargo's `Caused by:`): `the remote server responded with an error (status 429 Too Many Requests): You have published too many updates to existing crates in a short period of time. Please try again after Wed, 25 Nov 2026 08:29:14 GMT and see https://crates.io/docs/rate-limits for more details.` It is the first real 429 this lane has seen; it closes the "verbatim 429 wording" item of section 5e. Cargo did **not** abort the whole run: 24 crates were uploaded (lines 402 to 449, the `-sys` and leaf layer) and cargo then waited for them and printed `Published` for all 24 (line 452), then uploaded 6 more (`baracuda-cublas`, `-cudf`, `-cudnn`, `-cufft`, `-cufile`, `-cupti`, lines 453 to 464), failed on `baracuda-curand` (line 466) and listed 28 crates `not published yet` (lines 469 to 496, 28 `baracuda` names). 24 + 6 = 30 uploaded in that run; with `curand` and the 28 listed, 29 were left, and 30 + 29 = 59 = 70 minus the 11 the registry shows at 07:13 (below). The loop's first round line agrees (`29 remaining`). The five loop files truncate the message at `Please try again after Wed, 25 Nov 2026 08`, so **their retry-after times are not in the files**; the wording is otherwise the same on every error line (`grep -c 'status 429'` on the files as they stand now: alpha.186 loop 12, .188 17, .191 18, .193 22, .195 11; **control:** the same files give 1 `ALL AT` line each, and `rate-limits`, which only the full text contains, gives 0 hits in each loop file and 1 in the first-run file).

**The files change under a reader.** This lane read `publish-baracuda195.out` twice, 47 minutes apart (18:20Z and 19:08Z). The first read began `round 1 18:00:52Z: 70 remaining` and had reached round 5 (48 remaining); the second began `start 18:37:03Z dry run`, `round 1 18:40:51Z: 30 remaining` and ended `ALL AT 0.1.0-alpha.195`. The loop was **restarted at about 18:37Z and wrote over its own file**, so the first 40 crates' rounds are gone from the file and survive only in the registry (below). Anything read from an out file while its run is live is read at that moment, not for the run.

**What the registry says**, read by this lane on 2026-11-25 with `GET https://crates.io/api/v1/crates?q=baracuda` (75 names) and then `GET https://crates.io/api/v1/crates/<crate>/<version>` once per name per version (field `version.created_at`; 1 request per second, no credentials). For each of the five versions 70 names answered `200` (none yanked) and 5 answered `404`: `baracuda-cuda-emit`, `-cuda-parse`, `-cuda-seam` (versioned `0.56.x`, section 8), `-kernel-vocab` and `-kernelgen`. That is a read by a different method from the out files, and it agrees with their `list 70` line in every file that has one.

| run | first `created_at` (UTC) | last | largest burst | crates after it | over | crates per minute after it |
|---|---|---|---|---|---|---|
| alpha.186 | 07:13:40 | 08:57:28 | 30 (08:28:28 to 08:28:51) | 29 | 28.6 min | 1.01 |
| alpha.188 | 12:55:58 | 13:35:22 | 31 (12:55:58 to 12:56:22) | 39 | 39.0 min | 1.00 |
| alpha.191 | 16:03:47 | 16:44:14 | 30 (16:03:47 to 16:04:11) | 40 | 40.1 min | 1.00 |
| alpha.193 | 16:54:41 | 17:53:20 | 11 (16:54:41 to 16:54:52) | 59 | 58.5 min | 1.01 |
| alpha.195 | 18:03:32 | 19:05:19 | 10 (18:03:32 to 18:03:43) | 60 | 61.6 min | 0.97 |

Computed from the `created_at` fields, a burst being crates less than 20 s apart. After the opening burst every run publishes **2 to 4 crates every 100 to 270 s** (alpha.195 once 557 s, across the loop restart), about **one crate per minute** in all five runs, 227 crates in about 228 minutes taken together.

**What those numbers fit, and what they do not prove.** A bucket of about 30 that refills at one crate per minute explains every opening burst: alpha.188 (31) started 4 h 17 min after alpha.186 ended and alpha.191 (30) 2 h 30 min after alpha.188 ended, both long enough to fill it; alpha.193 started 10 min 27 s after alpha.191 ended and published **11**, alpha.195 started 10 min 12 s after alpha.193 ended and published **10**; alpha.186's second burst (30 at 08:28) came 75 min after its first (11 at 07:13). So the limit is shared across runs and across crates, and not reset by a run starting. **This is a fit to five runs by the lane, not crates.io's published rule** (`https://crates.io/docs/rate-limits` was not read here): the capacity, the refill rate and whether the unit is a crate or a request are inferred, and a bucket of 30 versus 31 is not distinguishable from these data (alpha.188's 31 may include one refill during its own 24-second burst).

**What the loop costs.** The loop's own `round` lines are about 2 to 4 minutes apart although a 429 ends a round in seconds: each round re-runs cargo's package and verify steps before it uploads (the loop files carry no per-line timestamps, so how much of a round is verify is **not measured here**), and the registry shows the uploads themselves bunched at the end of each round. With the limit at about one crate a minute, a 70-crate workspace takes **about 40 to 60 minutes after the opening burst**, not the 4 to 6 minutes the first burst suggests, and the loop spends most of it re-verifying crates it has not yet uploaded. Releaseant's own `--retries` and ledger were not run against this registry; whether they would have waited longer between attempts, or published the same crates sooner, is not measured.

**What this adds, and what it does not.**
- Added: the first verbatim 429 (line 499 of the alpha.186 first-run file), a partial multi-crate run that cargo itself reported accurately (30 published, 30 uploaded-then-published, the rest listed as not published), and a registry timeline of five full 70-crate publishes with the steady rate and the bucket fit above. It is the data section 5e lacked for a real rate limit.
- Not added: the retry-after time in any loop file (truncated), an exit code for any cargo run, any check that the archives match a commit, any tag read, and anything about releaseant's guards or retries. A kill between upload and read-back, a lagging index (the registry gaps here are all inside cargo's own waits, which end on their own) and a real transient failure other than 429 are still unmeasured, as in 5e.
- The portfolio pin and the next publish are for the next section.
