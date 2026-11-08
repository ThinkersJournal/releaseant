# Runner tests

The tests that need Docker and `crane` are **not** `#[ignore]`d. `tests/crane_runner.rs` sits behind the
`runner-tests` cargo feature, and three things keep that from being a way to hide it (the pattern is
Baracuda's `docs/DEVICE-TESTS.md`):

1. **Built on every PR.** CI's clippy and doc jobs use `--all-features`, so the test is compiled and linted
   on the hosted runners; the hosted `cargo test` job runs without the feature (nothing else here has
   features), so it does not try to run it there.
2. **Actually run.** `.github/workflows/runner-tests.yml` runs
   `cargo test --locked --features runner-tests --test crane_runner` on the self-hosted runner, on push to
   `main`, on a daily schedule and on manual dispatch. Never on a pull request (public repository: fork
   code must not run on that machine), and it names no secret (CIRESNAVE-EXPECTATIONS 5.1h). A run that
   passes anything but exactly one test is a failure; a missing Docker or `crane` makes the test fail
   with a message, not skip.
3. **A stale or missing run is red.** `.github/workflows/runner-tests-detector.yml` fails if the last
   successful run is older than 7 days or absent.

**Status (2026-11-08): the runner lane is NOT running.** No self-hosted runner is registered for this
repository: the PM reports that no organisation-level runners exist and the default runner group does
not allow public repositories, so a group has to be set up first (a PM decision). Both workflows are gated
on the repository variable `RUNNER_TESTS_ENABLED == 'true'`, which is unset, so their jobs are `skipped`.
Until it is set and a run succeeds, nothing in CI has run the real-crane check, and the only evidence for
it is the by-hand run of 2026-11-08 recorded in `docs/ARTIFACT-MEASUREMENTS.md`, which is **local-only**:
do not cite it as CI evidence. `tests/crane_runner.rs` itself was run once, locally, on 2026-11-08 (crane
0.22.1, Docker 29.5.3 on the PM's Windows box): 1 passed in 36.7 s, and no container was left behind. Two controls
were run: with `CRANE` pointing at nothing it FAILS with the message above (it does not skip), and with the
tag-held check in `src/images.rs` switched off it fails at the Held assertion. None of that is a CI run.

## What the runner must provide

Docker with a daemon that can pull `registry:2` and publish a localhost port; `crane` (0.22.1 is the
version the measurements used) via `CRANE` or the PATH; a Rust toolchain. The `runs-on` labels in the
workflow (`self-hosted, windows, releaseant`) are a draft: the PM sets them when the runner group exists.

## Run locally

```text
cargo test --features runner-tests --test crane_runner -- --nocapture
```

It starts its own `registry:2` container on a free localhost port and removes it afterwards.
