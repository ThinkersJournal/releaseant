kind: CI: portfolio gate pin moved to dc45e36
---
Moved the `portfolio.yml` reusable-workflow pin in `.github/workflows/ci.yml` from c72a6e0 to dc45e3632ab9f471292458e5ccba5f375084a70a, as the PM asked. `url` is not in `Cargo.lock` (a positive control, `serde`, is), so RUSTSEC-2026-0592 needs no lock change. Verified by the diff only; the advisory result is read from CI by check name once the PR opens.
