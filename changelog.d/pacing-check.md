kind: docs: retry pacing checked against the measured crates.io rate
---
`docs/R5-LIVE.md` section 10 gains a pacing check of `src/run.rs` against the rate measured on 2026-11-25 (server `Retry-After` plus 5 s is the right shape; the per-run `--max-wait` default of 3600 s is the number that is tight for a 70-crate workspace). `docs/ADOPTION.md` no longer says the rate is not measured. No code change.

Also: the `portfolio.yml` reusable-workflow pin in `.github/workflows/ci.yml` moves from dc45e36 to 467fd259d6d062cba4c5683d310f1a6f2b66e763 (coderipper#375, CODERIPPER_VERSION 0.48.1), as pm-ants asked.
