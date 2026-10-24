# R4: real-registry rehearsal

Ruling (PM, 2026-10-24): **go, with limits.** Three placeholder crates only, names
`releaseant-rehearsal-core`, `-mid`, `-top` (chain `core <- mid <- top`), version `0.0.0` then `0.0.1`
for the resume case, licence `MIT OR Apache-2.0`, description "rehearsal crate for releaseant, no
functionality". The lane never sees a token: it wrote the workspace and the script, the PM runs the
script with `CARGO_REGISTRY_TOKEN` set from the vault and returns the full output.

## Preconditions checked (2026-10-24, by the lane)

- The three names are free. Positive control: `https://crates.io/api/v1/crates/semver` and the sparse
  index path for `semver` both return 200; each of the three names returns 404 on both.
- The rehearsal repository is `C:\Projects\releaseant-rehearsal` (local only, not pushed): commit
  `513371646eb3eb0c5523bcc5f6c2e03729d21c8c` = version 0.0.0, commit
  `dc8bd420dcba8b683dfd2dcdea1015332265f66b` = version 0.0.1 (all three crates, `mid` requires `core`
  and `top` requires `mid` at the same version, with a path as well).
- With the CLI from PR 5 (`c88ed4b2`), dry-run only, on both commits: `plan` printed core, mid, top;
  `verify` built the three extracted tarballs; `publish` printed `would upload` for each; `readback`
  printed `ABSENT` for each. Nothing was uploaded.

## Run it

Build a binary from a `main` that contains the CLI (merge PR 5 first), then, in a shell where
`CARGO_REGISTRY_TOKEN` is set (for example through `with-secret`):

```bash
RELEASEANT_BIN=/path/to/releaseant docs/rehearsal/rehearse.sh 2>&1 | tee rehearsal-all.txt
```

Return `rehearsal-all.txt` whole. The script never prints the token and only checks that the variable
is non-empty.

## What the lane predicts (so a surprise is visible)

| Step | Expected |
|---|---|
| 1 readback before | `ABSENT` x3, exit 1 |
| 2 run A | core, mid, top each `published and read back`, exit 0; no 429 (3 new crates is inside the burst) |
| 3 readback | present x3 with 64-hex checksums, exit 0 |
| 4 run B1 | killed after core 0.0.1 is read back; `interrupted by this script: yes`; mid/top may or may not already be uploaded (the kill races the next upload) |
| 5 readback | core present; mid/top present or absent |
| 6 run B2 (resume) | core `already published`; any of mid/top not yet on the index `published and read back`; exit 0; no crate uploaded twice (cargo would answer `already exists`) |
| 7 readback | present x3, exit 0 |
| 8 ledger | `planned`, `verified`, `upload_started`, `upload_finished`, `visible` events per crate; the second run at a different SHA has its own `planned`; a `classified` event with `already-published` for core in B2 |

Updates to existing crates (run B) have their own crates.io rate limit; the PM's 2026-10-24 sample was
for that limit. If a 429 appears, `classified`/`waiting` events and the wait taken are in the ledger and the
verbatim cargo text is in the run's output: **that text is what R4 exists to capture** (it extends
`tests/fixtures/` and the classifier's table).

## What stays behind

Published crates cannot be deleted. These three stay on crates.io as 0.0.0 and 0.0.1 placeholders. The
lane does not yank them (yank is not part of this rehearsal).

## After the run

The lane compares the output with the table above, adds any new real samples as fixtures with
provenance, fixes what the run showed, and records the measured limits (burst size, interval, the
retry-after wording of the "new crates" limit if it appeared) in `docs/DESIGN.md` with the date.

## Result (run by the PM, 2026-10-24 01:58-01:59 UTC, releaseant 0.0.7, cargo 1.99.1)

Read from the run's own artifacts (output file and ledger), and cross-checked by the lane against the live
registry afterwards (crates.io API returns 200 for all six `name@version`s):

- Steps 1-7 matched the table above. Run A published and read back all three crates in about 20 s
  (roughly 4 to 5 s per crate, which includes cargo's own wait for availability). The checksums the
  index showed after run A equal the `Accepted` checksums in the ledger for each crate.
- **Interrupt and resume worked.** Run B was killed after `releaseant-rehearsal-core 0.0.1` was read back;
  its log was empty (the tool prints at the end), the ledger ended with `mid` `upload_started` and no
  finish, and the registry confirmed `mid` and `top` ABSENT. The resume reported `core: already published`
  (ledger `classified: already-published`) and published `mid` and `top` with read-back; the final
  read-back showed all three present. Each crate/version was uploaded exactly once.
- **No 429 occurred**, so no rate-limit text was captured and nothing in `tests/fixtures/` changed. The
  "new crates" limit's wording and the rate numbers remain unmeasured; six uploads are not enough to reach
  either limit.
- **Not exercised:** a kill landing *between* an upload and its read-back (here the killed upload had not
  reached the registry); a lagging index; a dependency not yet visible.

Deviation from the script as written, disclosed by the PM: the PM's copy replaced the
`CARGO_REGISTRY_TOKEN` check with a check that `~/.cargo/credentials.toml` exists, because the vault had no
crates.io token and cargo reads that file. The script in the repository is unchanged and still asks for the
environment variable; a lane or CI that has the token only in `credentials.toml` needs the same tweak.

Small findings: the interrupted run's output is empty because progress is printed at the end (a live
progress line per crate would help a human watching); the `verified` ledger events precede the `planned`
event of the same run.
