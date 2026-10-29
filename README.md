# ReleaseAnt

Ordered, verified, resumable publishing of Rust crates and workspaces to crates.io, with read-back.

**Status: not ready for developer use** until its first release: not yet published (the crates.io name
holds an empty 0.0.0 placeholder).

## Install

Not on crates.io yet. From a clone (Rust 1.88 or newer; `cargo install` puts the binary in `~/.cargo/bin`,
which rustup already put on your `PATH`):

```console
git clone https://github.com/ThinkersJournal/releaseant
cd releaseant
cargo install --path .
```

## Hello

You have a git workspace where `releaseant-demo-app` depends on `releaseant-demo-core`, and it is committed.
Each crate has the fields crates.io requires (`description` and `license`), and the dependency names a
version as well as a path (`releaseant-demo-core = { path = "../releaseant-demo-core", version = "0.1.0" }`).
ReleaseAnt works from a commit, never from your working tree, so it needs the commit's full SHA. Run it from
inside the repository (or add `--repo PATH`):

<!-- hello:begin -->
```console
$ SHA=$(git rev-parse HEAD)
$ releaseant plan --sha $SHA
releaseant-demo-core 0.1.0
releaseant-demo-app 0.1.0
$ releaseant publish --sha $SHA        # a dry run: nothing is uploaded
releaseant-demo-core: would upload
releaseant-demo-app: would upload
dry run: nothing was uploaded (use --execute)
ledger: ./.releaseant/ledger.jsonl
```

`plan` is the order dependencies must go up in. `publish` packages the commit, builds every packaged
`.crate` the way a consumer would, and asks crates.io what is already there, but uploads nothing until you
add `--execute`. It needs network (the crates.io index) and builds every crate, so expect about the time
`cargo build` takes. A branch name is refused, and says how to get the SHA:

```console
$ releaseant plan --sha main
error: not a full 40-hex commit SHA: "main" (get one with `git rev-parse main`)
```
<!-- hello:end -->

The ledger is a plain file, one JSON event per line, appended before each action. A dry run writes it too, so
`.releaseant/` shows up as untracked in `git status`.

## Publishing for real

```console
releaseant publish --sha $SHA --execute
```

- cargo finds your crates.io token (`cargo login`, or `CARGO_REGISTRY_TOKEN`); ReleaseAnt never reads or stores it.
- If crates.io rate-limits you, it waits the time the server asks for (up to `--max-wait` seconds, default 3600)
  and then stops with the run resumable. **Run the same command again to resume**: it skips what is already on the
  index and continues.
- After each upload it reads the index back and compares the checksum with what it uploaded.
  `releaseant readback --sha $SHA` asks the same question later.
- `releaseant yank NAME VERSION --reason kind:text --ledger FILE` yanks a version it published, with the reason
  recorded (dry run unless `--execute`).

Leave a crate out on purpose, with the reason recorded in the ledger (crates that need it still need it on the
registry): `--exclude NAME --exclude-reason "waits for the other project's release"`. Skip the pre-upload build for
one crate that cannot build on this machine: `--no-verify-crate NAME --reason "needs CUDA"` (it is checked as a
consumer after upload instead).

## What it checks that a hand-written loop usually does not

- Dependencies go first, including versioned dev-dependencies; a cycle is an error that names its members.
- The files come from `git archive` of the SHA, not your working tree.
- Each package is extracted and built before anything is uploaded.
- A dependency outside the set that is not on the registry stops the run before anything is written.
- A registry lookup that fails stops the run; it is never read as "not published".

More, in the clone: `docs/DESIGN.md` (how it works and why), `ROADMAP.md`, `CHANGELOG.md`.

Part of the ant family of shared building blocks. Licensed under MIT OR Apache-2.0.
