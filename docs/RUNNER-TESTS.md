# Runner tests

The tests that need Docker and `crane` are **not** `#[ignore]`d. `tests/crane_runner.rs` sits behind the
`runner-tests` cargo feature, and three things keep that from being a way to hide it (the pattern is
Baracuda's `docs/DEVICE-TESTS.md`):

1. **Built on every PR.** CI's clippy and doc jobs use `--all-features`, so the test is compiled and linted
   on the hosted runners; the hosted `cargo test` job runs without the feature (nothing else here has
   features), so it does not try to run it there.
2. **Actually run.** `.github/workflows/runner-tests.yml` runs
   `cargo test --locked --features runner-tests --test crane_runner` on a GitHub-hosted `ubuntu-latest` runner, on push to
   `main`, on a daily schedule and on manual dispatch. Never on a pull request (public repository: fork
   code must not run on that machine), and it names no secret (CIRESNAVE-EXPECTATIONS 5.1h). A run that
   passes anything but exactly one test is a failure; a missing Docker or `crane` makes the test fail
   with a message, not skip.
3. **A stale or missing run is red.** `.github/workflows/runner-tests-detector.yml` fails if the last
   successful run is older than 7 days or absent.

**Status (2026-11-12): the crane check runs on a hosted runner.** The PM ruled (CireSnave, 2026-11-12) that no
self-hosted ReleaseAnt runner is needed for it. The first run of the hosted job (workflow_dispatch from branch
`crane-hosted`, run 43286862678, ubuntu-24.04 image 20261111.370.1) installed crane 0.22.1 from its release (sha256 checked,
`OK`), found Docker 28.0.4 preinstalled, and ran `test real_crane_stages_promotes_holds_and_stops_against_a_real_registry ... ok`
(the cargo step took about 22 s with the build, the test about 9 s) with no Docker Hub pull limit hit. That is one run of one commit, not a trend. Earlier
statements here and in `tests/crane_runner.rs` and `ci.yml` that hosted runners offer no Docker were wrong for ubuntu-latest
(true for the windows and macos hosted runners, which have no Linux containers, per GitHub's runner documentation; not measured here) and are replaced.

**The forge job is still not running.** It needs a `gh` login that can write the private rehearsal repository, and the only candidate, the WSL runner, has
none: a probe of it (labels self-hosted, Linux, X64, wsl; account `ghr-linux`, same run) found `gh: command not found`.
Its `runs-on` is still the old draft labels, which no runner carries, and it stays gated on the repository variable
`RUNNER_TESTS_ENABLED`, which is unset, so it is `skipped`. Until it runs, nothing in CI has exercised the real `GhForge`;
the only evidence is the by-hand run in `docs/ARTIFACT-MEASUREMENTS.md`, which is **local-only**.

## What each job needs

`crane` job (`tests/crane_runner.rs`): Docker able to pull `registry:2` and publish a localhost port; `crane` via `CRANE` or the
PATH (the hosted job installs 0.22.1, sha256-pinned); a Rust toolchain. No credential.

`forge` job (`tests/forge_runner.rs`): a `gh` signed in with write access to the private rehearsal repository
`ciresnave-bot/releaseant-artifact-rehearsal` (or `RELEASEANT_FORGE_REPO` set there). The workflow names no secret. Like the crane
job it fails unless exactly one test ran and passed. Run it by hand with
`cargo test --features runner-tests --test forge_runner -- --nocapture`.

## Run locally

```text
cargo test --features runner-tests --test crane_runner -- --nocapture
```

It starts its own `registry:2` container on a free localhost port and removes it afterwards.
