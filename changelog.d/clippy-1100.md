kind: clippy on Rust 1.100
---
- Fix: `cli::run` no longer carries `#[must_use]`; `ExitCode` is `#[must_use]` itself since Rust 1.100, so clippy `double_must_use` failed `-D warnings`. `cargo clippy --all-targets --all-features -- -D warnings` is clean on rustc 1.100.0 (it failed with exactly this one error before). No behaviour change.
