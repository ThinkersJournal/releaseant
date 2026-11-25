kind: docs: retry pacing checked against the measured crates.io rate
---
`docs/R5-LIVE.md` section 10 gains a pacing check of `src/run.rs` against the rate measured on 2026-11-25 (server `Retry-After` plus 5 s is the right shape; the per-run `--max-wait` default of 3600 s is the number that is tight for a 70-crate workspace). `docs/ADOPTION.md` no longer says the rate is not measured. No code change.
