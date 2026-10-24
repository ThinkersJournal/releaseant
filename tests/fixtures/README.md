# Captured cargo output

Real output as cargo printed it (git may normalise line endings), kept so `classify` is tested against what cargo actually printed.
A pattern with no sample here does not ship (DESIGN section 4); it classifies as `Unknown`.

| File | What it is | Source | Captured |
|---|---|---|---|
| `published_ok.txt` | `cargo publish` of baracuda-core v0.0.1-alpha.152, exit 0 | PM's `%TEMP%/ba-pub-baracuda-core.log` | 2026-10-23 08:59 (file mtime) |
| `already_exists.txt` | `cargo publish` of an already-published version, exit 101 | PM's `%TEMP%/ba-pub-baracuda-nvrtc.log` | 2026-10-23 09:03 (file mtime) |
| `dep_missing.txt` | `cargo package` (not publish) with a versioned dependency absent from the index, exit 101, cargo 1.99.1 | this repo's lane, a throwaway workspace with never-published names | 2026-10-24 |

`dep_missing.txt` comes from `cargo package`; `cargo publish` runs the same preparation step, but
that equivalence is an assumption until R4 captures a real `cargo publish` of it.

No sample of a crates.io rate limit (429) exists yet: none of the PM's logs kept the text. Until R4
captures one, a rate-limited `cargo publish` classifies as `Unknown` and stops the run (resumable).
