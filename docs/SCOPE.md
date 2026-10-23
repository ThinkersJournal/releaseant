# ReleaseAnt: scope

ReleaseAnt publishes a set of crates in dependency order, verifies each package BEFORE upload (extract the packaged .crate and build it, so a manifest that only works from the repository tree is caught), uploads with rate-limit backoff, and reads every crate back from the registry (sparse index and API) before it reports success. It is resumable: crates already published at the target version are skipped, so an interrupted run continues. Dry-run is the default; a real publish is explicit.

## Rules and traps to design in

Traps found by hand on 2026-10-23 (each is a test to write FIRST, with a negative control):
1. Publish from `git archive <merge sha>` into a clean directory, never from a working tree.
2. A crate's DEV-dependencies (even path-only ones) and OPTIONAL dependencies still have to resolve at package time: baracuda-nvrtc needed baracuda-driver and baracuda-cuda-sys published first although neither is a normal dependency. The publish set must be computed from `cargo package` resolution, not from normal dependencies alone.
3. A caret requirement on a prerelease does not match an older prerelease (^0.0.1-alpha.152 does not match alpha.147).
4. crates.io returns 429 after a burst; a workspace publish that dies midway leaves a partial set. Resume must skip published crates and wait, not abort.
5. `--no-verify` hides breakage: use it only as an explicit, recorded exception, then do a clean `cargo add <crate>@<version>` build check.
6. Order: dependencies first, then dependents, then the umbrella crate; a cycle must be reported, not looped on.
7. Read-back: a crate that `cargo publish` accepted can be absent from the index for a while; poll with a timeout and say which crate is missing.
8. Never yank on a hunch; a yank policy is a rule set with a recorded reason.
