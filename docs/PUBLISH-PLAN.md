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
- The crate page text (`README.md`) was not re-read for the public audience in this plan.
