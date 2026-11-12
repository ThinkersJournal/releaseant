kind: real crane check on a hosted runner
---
- CI: `runner-tests.yml` runs the real-crane check on `ubuntu-latest` (crane 0.22.1 installed from its release, sha256-pinned) instead of waiting for a self-hosted Windows runner; corrected three statements that hosted runners offer no Docker. The forge job is unchanged. No Rust change.
