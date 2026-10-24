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

| `rate_limited.txt` | the tail of a `cargo publish --locked` that crates.io answered with 429, cargo 1.99.1 | **relayed by the PM** in a peer message (`[FINDING] real-429-sample`, 2026-10-24T01:27Z), "verbatim"; this lane did not see the full output, so only these two lines are kept | 2026-10-24T01:26Z |

The 429 pattern matches the outer shape of that line, not the middle sentence, because crates.io has more
than one limit ("new crates" and "updates to existing crates" are known to exist; only the second has a
sample). A variant with different middle text is a **guess** until a sample exists; the test for it says so.
