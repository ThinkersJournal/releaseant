kind: CI: move the CodeRipper portfolio workflow pin to 0.47.5
---
The advisory `portfolio` job in `ci.yml` now calls the shared workflow at coderipper `c72a6e03479d914de8ea8a73609ba73e93a0de92` (installs CodeRipper 0.47.5) instead of `b45eb41`. The repository has no `.coderipper.toml` and no old rule names in any file, so `tools/migrate-rule-ids.py --dry-run` finds nothing to rename (it exits 2: no config to read). No code change.
