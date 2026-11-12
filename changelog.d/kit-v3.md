kind: refresh the fragment tools (kit v3)
---
- CI/tooling: `tools/stamp-version.py` and `tools/check-fragment.py` refreshed from the portfolio kit (refactored after Codacy flagged the first copies: bandit subprocess, cyclomatic complexity); no change in behaviour. Verified on a scratch clone: `--dry-run` then a real stamp 0.0.76 to 99.99.99 changed `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md`, and no 0.0.76 remained in any `Cargo.toml`; `check-fragment.py` ok. No Rust change.
