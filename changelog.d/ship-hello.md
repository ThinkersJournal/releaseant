kind: `examples/make-demo.sh` ships in the package and builds the README demo workspace
---
The fifth fresh-eyes trial (no git identity) found the demo workspace had to be assembled by hand. `examples/make-demo.sh` writes it (manifests generated from the README blocks), commits with a repo-local identity only when none is set, and `examples/**` joins the package `include`. Verified by a new test in `tests/readme.rs` that runs it with no global git config, compares every manifest to the README and runs `plan` on the result.
