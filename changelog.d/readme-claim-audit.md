kind: docs
---
README claim-to-command audit (pm-ants task, 2026-11-19): `tag --crate` now shows the `--sha` it requires, and the README says a run that changes the plan (`--exclude`, `--only`) at a commit that already has a ledger run is refused, with the two ways out. Verified by running the README hello-world and 20 further documented commands on a built 0.0.84 binary (dry runs only, empty CARGO_HOME, no token), and by the refusal and both workarounds on a fresh demo workspace.

EasyAnts card: `easyants-card.toml` at the repo root (format: easyants docs/CARD-FORMAT.md), with `examples/hello.sh`, the README hello-world commands that the audit ran; a test keeps the example equal to the README.
