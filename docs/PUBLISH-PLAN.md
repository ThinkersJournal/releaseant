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
| The example is compiled and **run** in CI on Windows and Linux | **done.** `tests/readme.rs` builds the demo workspace from the README's own `toml` blocks, commits it, and runs every `$ ` line with the real binary against the output the README prints. It already ran in the `test` matrix (ubuntu, windows, macos). It now also has a named CI step, `README hello-world (compiled and run)`, so a red README is visible as such. **Seen green on GitHub** at head `a7f06b4` (run 41217810693): in the named step `running 1 test ... ok` on ubuntu-latest, windows-latest and macos-latest, and the same test inside the full `cargo test` of each leg |
| What the package contains is right | **done.** `Cargo.toml` had no `include`, so the package shipped `HANDOFF.md`, `CLAUDE.md`, `.github/`, `docs/` and the ROADMAP; it now ships `src/`, `tests/`, `README.md`, `CHANGELOG.md`, `SECURITY.md` and both licences, 9 files plus source and tests |
| The package builds from its tarball, kept by CI | **done.** `cargo package --locked` verifies the tarball builds (Windows locally: ok), and the new CI job `package (ubuntu-latest)` runs it: success at head `a7f06b4`. All 15 checks of that run were green (fmt, clippy, doc, deny, package, test x3, CodeQL, portfolio) |
| Fresh-eyes trial of the **packaged** form, 3 of 3, **on the final text** | **3 of 3 PASS on the README as shipped (0.0.42, README identical to the one in the head named in the `[READY]`), by the trial agents' own reports** (fresh Sonnet subagents, one at a time, each given only the unpacked `.crate` and its `README.md`, told not to read `src/` or `tests/`; each ran `cargo install --path .` from the unpacked package, built the demo workspace from the README, and ran the README's commands). Three earlier runs on draft READMEs are listed below as history, not as the gate |
| Placeholder's owner | `cargo owner --list releaseant` (read 2026-10-31) printed one name: `ciresnave (CireSnave)`. A user owner, no team. **The token's right to publish is UNTESTED.** That command lists owners; it does not show whether the token a publisher will use may publish, and no token was read or tried here. The first `--execute` is the first proof. |

### The trials (each in a fresh directory with its own install root and target dir)

**On the final text (the gate).** Package `releaseant-0.0.42.crate`, sha256 `a7e1bc5d5fc9cca957c159533751b63593e5857cae4e21ad2ad4afdfa09d4da6`, built from commit `662f5c5` with `cargo package --locked`.

| run | result | install | what the agent said (condensed) |
|---|---|---|---|
| F1 | PASS: output matched line for line, `plan --sha main` refused, exit 0, 0, 2 | 3m44s | nothing blocked; the README does not say how to make the empty files (it used `: >` and `echo`); the Hello section does not mention the network (it does, further down); the status banner above a working Hello is "mildly confusing" |
| F2 | PASS, output matched | 1m12s | nothing blocked; could not test the `~/.cargo/bin` claim (it overrode the install root); the README does not say what the network is needed for in Hello; Windows `LF will be replaced by CRLF` git warnings may make a reader hesitate |
| F3 | PASS, output matched | 1m39s | nothing blocked; "Rust 1.88 or newer" appears only under the crates.io install heading; the CRLF warnings are not mentioned |

Not fixed on purpose, so that the text the trials passed is the text that ships: a line saying the dry run needs the network in the Hello section, how to create the empty files, the Windows CRLF note, and the banner. Each is a minor remark by an agent that reached the result; changing the README would mean running the three again.
Git identity was configured on the box in F1 and F2 (F3 set one itself), so the README's "if git asks" line is not tested by the final runs.

**Earlier drafts (history, not the gate).** Three runs on READMEs a few lines different from the final one, all PASS; they produced the fixes now in the README: the `mkdir -p` line, "any text" for each crate's `README.md`, the git-identity note, `cd` into "the unpacked package's directory", and "the release owner" for "the PM" (trial 3 of those called "the PM" jargon).

The trials were on Windows. Not run: a Linux or macOS stranger trial (CI builds and tests the README example on both), a machine without a git identity, and `cargo install releaseant` (impossible until published). The status line "not ready for developer use" is kept until the first release; flipping it is part of the publish. Transcripts are the agents' reports, not stored here.
