# Publish plan for releaseant itself (not run)

Prepared 2026-10-31 for the PM. **Nothing in this file has been executed**: no `--execute`, no upload, no tag. crates.io
holds only the `0.0.0` placeholder of `releaseant` (index read 2026-10-31: one line, `vers 0.0.0`, not yanked, published
2026-10-22). Unpublished since then: 0.0.1 to the current version in `Cargo.toml`.

## What is ready (each read at a named ref)
- The owed patent re-sweep is done: eighth pass in `docs/PATENT-SWEEP.md` (wording "outside as read", INCOMPLETE).
- Dry run of the tool on its own repository at `eed29cd988c6b8ff28e297b0ad181c3035cfab18` (0.0.40), release build of the
  same commit, `--ledger` in a scratch file: `plan --registry --check-externals` printed `releaseant 0.0.40 (not on
  registry)` and "every dependency outside the set is on the registry"; `publish --tag` (dry run) packaged 1 crate, verified
  it (the extracted `.crate` built), printed `releaseant: would upload` and `would tag v0.0.40 -> eed29cd9...`; exit 0.
- Metadata at that ref: `license = "MIT OR Apache-2.0"`, `repository`, `readme`, `keywords`, `categories`, `rust-version =
  1.88`; both licence files and the README are in the tree.

## What to publish
One crate, at the merge commit of the PR that carries this file (its version is the PM's to allocate; the PR's version is
the one that goes up). Publishing the commit just before it, `eed29cd9` (0.0.40), would also work but leaves this file and
the sweep record out of the published crate.

## Steps (the PM, after "release")
1. `git fetch`, then take the full SHA of `origin/main` once the PR is merged: `SHA=$(git ls-remote origin refs/heads/main | cut -f1)`.
   A real upload refuses a commit that is not origin's tip, so publish right after the merge, before main moves.
2. Dry run first, from a build of the same commit (`cargo build --release`): `releaseant publish --sha $SHA --tag` (no
   `--execute`). Expect `releaseant: would upload`, `would tag v<version>`, exit 0, no `HELD` (an unpublished version).
3. `cargo login` or `CARGO_REGISTRY_TOKEN` for the account that owns the `releaseant` placeholder; ReleaseAnt never reads
   the token.
4. `releaseant publish --sha $SHA --execute --tag`. One crate: no ordering, no rate-limit wait expected (a first version of an
   existing name is not a new-crate burst). It reads the index back and compares the checksum, then creates and pushes the
   annotated `v<version>` and reads it back.
5. Afterwards: `releaseant readback --sha $SHA` shows `present (<sha256>)`; `git ls-remote --tags origin v<version>` shows the
   tag on `$SHA`. Say where it was read.
6. If cargo is interrupted: re-run the identical command (it asks the registry first). If the output says `HELD`, someone
   published this version outside the tool: follow `docs/HAND-PUBLISH.md` steps 7 to 9 (the PM adds the ledger line).

## Not known, said plainly
- The first real publish of ReleaseAnt by ReleaseAnt has not happened; the dry run above is the nearest evidence.
- The `0.0.0` placeholder's owner and the token's rights were not checked (a token cannot be read here).
- Windows only: every run of the tool so far was on this machine.
- (The crate page text was checked by the ease gate below.)

## Ease gate (EXPECTATIONS 6.24, CireSnave 2026-10-28), done 2026-10-31 on branch `ease-gate`

PM `[TASK]` of 2026-10-31: ReleaseAnt is developer-facing, so ease is a release gate. Version at gate. Read at the PR head
named in the `[READY]`; the numbers below are of the package built from that tree.

| gate item | result |
|---|---|
| README opens with a hello-world a stranger can copy | **done.** `## Hello` is now the first section: the SHA line and two commands (`plan`, then a dry-run `publish`) with their real output, then the demo workspace they ran on. The earlier README opened with the install and a long metadata paragraph before the commands; the paragraph moved to "Before it uploads anything: the metadata check". Honest size: three commands to type, about fifteen lines with the output, not ten |
| The example is compiled and **run** in CI on Windows and Linux | **done.** `tests/readme.rs` builds the demo workspace from the README's own `toml` blocks, commits it, and runs every `$ ` line with the real binary against the output the README prints. It already ran in the `test` matrix (ubuntu, windows, macos). It now also has a named CI step, `README hello-world (compiled and run)`, so a red README is visible as such. Not seen green on GitHub until the PR's checks finish: this lane ran it only on Windows (9 of 9 in `tests/readme.rs`) |
| What the package contains is right | **done.** `Cargo.toml` had no `include`, so the package shipped `HANDOFF.md`, `CLAUDE.md`, `.github/`, `docs/` and the ROADMAP; it now ships `src/`, `tests/`, `README.md`, `CHANGELOG.md`, `SECURITY.md` and both licences, 9 files plus source and tests |
| The package builds from its tarball, kept by CI | **done.** `cargo package --locked` verifies the tarball builds (run locally on Windows: ok), and a new CI job `package (ubuntu-latest)` runs it |
| Fresh-eyes trial of the **packaged** form, 3 of 3 | **3 of 3 PASS, by the trial agents' own reports** (Sonnet subagents, one at a time, each given only the unpacked `.crate` and its `README.md`, told not to read `src/` or `tests/`; each ran `cargo install --path .` from the unpacked package, built the demo workspace from the README, and ran the README's commands). See below |
| Placeholder's owner | `cargo owner --list releaseant` (read 2026-10-31) printed one name: `ciresnave (CireSnave)`. A user owner, no team. **That command does not show whether the token a publisher uses may publish**; no token was read or tested. |

### The three trials (each in a fresh directory with its own install root and target dir)

| run | package | result | what the agent said (their words, condensed) |
|---|---|---|---|
| 1 | README before the two small fixes below | PASS, output matched, exit codes 0, 0, 2 | install 1m18s; hit nothing in the README; remarked that the per-crate `README.md` content is not described and a fresh machine would need a git identity. (A `command not found` on its first try came from the sandbox's Windows-style `PATH`, not from the README.) |
| 2 | README with "any text" and the git-identity note | PASS, output matched | install 1m29s; its one stumble was `mkdir` failing because the member directories did not exist: the README did not say to create them |
| 3 | README with the `mkdir -p` line and "or the unpacked package's directory" | PASS, output matched | install 1m14s; nothing blocked; said the "not ready for developer use" banner makes a reader hesitate, "Rust 1.88 or newer" has no how-to-check, and "the PM adds one line" in the Publishing section is an internal term |

Changes made from the trials: the README now says the demo directories are made with `mkdir -p ...`, that each crate's
`README.md` holds any text, to set `user.name`/`user.email` if git asks, to `cd` into the unpacked package's directory, and
"the PM" became "the release owner (whoever decides what goes up)" (that last wording came after trial 3, so trial 3 ran on
a README that still said "the PM"; `tests/readme.rs` passes on the final text). The "not ready for developer use" status line
is **kept** and is true until the first release; flipping it is part of the publish. Trials 1 and 2 ran on READMEs a few
lines different from the final one; no trial ran on the final README's exact text. The trials were on Windows with git
configured; a machine without a git identity was not tried. Transcripts are the agents' reports, not stored here.

What a fresh stranger could still hit and this does not cover: `cargo install releaseant` (not possible until published),
Linux and macOS installs (CI builds and tests them; no stranger trial was run there), and the first real publish.
