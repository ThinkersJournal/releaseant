# ReleaseAnt

Ordered, verified, resumable publishing of Rust crates and workspaces to crates.io, with read-back.

**Status: not ready for developer use** until its first release: not yet published (the crates.io name
holds an empty 0.0.0 placeholder).

## Install

Not on crates.io yet. From a clone (Rust 1.88 or newer; the build takes a few minutes; `cargo install` puts the
binary in `~/.cargo/bin`, which rustup already put on your `PATH`):

```console
git clone https://github.com/ThinkersJournal/releaseant
cd releaseant
cargo install --path .
```

## Hello

Say you have this workspace of two crates, each with an empty `src/lib.rs` and a `README.md`, committed in git
(`git init`, `git add -A`, `git commit`). Before anything is uploaded, `plan` checks every crate's publish
metadata. What crates.io refuses is an **error** and stops the plan: no `description`, neither `license` nor
`license-file`, more than five `keywords` or one it does not accept, a category that is not one of its slugs, a
`readme` or `license-file` that is not there. What it accepts without is a **warning**, listed and not stopping
anything: no `repository`, README, `keywords`, `categories` or `rust-version`; name the ones you want enforced with
`--require-metadata readme,keywords,rust-version`, or commit them once in a `releaseant.toml` at the repository
root (`[metadata]`, `require = ["readme", "keywords", "rust-version"]`), which is read from the commit being
published and adds to the flag. Every problem of every crate is listed at once, so a set never dies
half-published at the crate that lacked one. ReleaseAnt also needs a dependency on a crate of the workspace
to name a `version` as well as a `path`:

<!-- hello:begin -->
```toml
# Cargo.toml
[workspace]
members = ["releaseant-demo-core", "releaseant-demo-app"]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT OR Apache-2.0"
repository = "https://github.com/example/releaseant-demo"
rust-version = "1.70"
keywords = ["demo"]
categories = ["development-tools"]
```

```toml
# releaseant-demo-core/Cargo.toml
[package]
name = "releaseant-demo-core"
description = "A demo crate"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true
keywords.workspace = true
categories.workspace = true
```

```toml
# releaseant-demo-app/Cargo.toml
[package]
name = "releaseant-demo-app"
description = "A demo crate that uses the other one"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
rust-version.workspace = true
keywords.workspace = true
categories.workspace = true

[dependencies]
releaseant-demo-core = { path = "../releaseant-demo-core", version = "0.1.0" }
```

ReleaseAnt works from a commit, never from your working tree, so it needs the commit's full SHA. Run it from
inside the repository (or add `--repo PATH`):

```console
$ SHA=$(git rev-parse HEAD)
$ releaseant plan --sha $SHA
releaseant-demo-core 0.1.0
releaseant-demo-app 0.1.0
$ releaseant publish --sha $SHA        # a dry run: nothing is uploaded
[00:00] packaging 2 crates
[00:01] verifying releaseant-demo-core (1 of 2)
[00:02] verifying releaseant-demo-app (2 of 2)
[00:03] verified releaseant-demo-core 0.1.0
[00:03] verified releaseant-demo-app 0.1.0
[00:03] run planned
[00:03] run finished: complete
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
`.releaseant/` shows up as untracked in `git status`. Commit it if you want the record in git, or add it to
`.gitignore`; ReleaseAnt does not mind either way.

## Publishing for real

```console
releaseant publish --sha $SHA --execute
```

- cargo finds your crates.io token (`cargo login`, or `CARGO_REGISTRY_TOKEN`); ReleaseAnt never reads or stores it.
- If crates.io rate-limits you, it waits the time the server asks for (up to `--max-wait` seconds, default 3600)
  and then stops with the run resumable. **Run the same command again to resume**: it skips what this tool already
  uploaded from this commit and continues. A version that is on the index but that the ledger does not show this
  tool uploading from this commit is reported as `HELD` and the run stops (someone else's publish, or a lost
  ledger); `--accept-held` does not exist yet.
  A crate published by hand (for example with `cargo publish -p`) will hold a later run for that crate and
  version. To go on, the PM adds one line to the ledger (`.releaseant/ledger.jsonl`) after checking that the
  registry's copy is ours (compare its index checksum with a `cargo package` of that commit), naming the commit
  being published:
  `{"ts":"<UTC>","run":"manual","sha":"<40-hex>","crate":"NAME","version":"X.Y.Z","event":"visible","detail":"published by hand; checked by <who>: <proof>"}`.
  Nobody else adds that line.
- After each upload it reads the index back and compares the checksum with what it uploaded.
  `releaseant readback --sha $SHA` asks the same question later.
- `releaseant yank NAME VERSION --reason kind:text --ledger FILE` yanks a version it published, with the reason
  recorded (dry run unless `--execute`).

`publish --execute` refuses a commit that is not the tip of origin's default branch (asked of origin itself, so an
unreachable origin is a refusal too); `--allow-not-tip` overrides it. A dry run is not checked.

Tag the release in git in the same action: add `--tag` (`releaseant publish --sha $SHA --execute --tag`). Once every
crate is on the registry it creates an annotated `v<version>` on exactly that commit, pushes only that tag to
`origin` and reads it back. It refuses a commit that is on no origin branch and never moves an existing tag; a
second run finds it already tagged. When the crates are at several versions, say which one with
`--tag-version 0.32.0`; a run with `--exclude` is not tagged. A run with `--only` has no project tag (it would claim more than was published): each selected crate that is on the registry gets its own annotated `<crate>-v<version>` (`releaseant tag --crate NAME --version V` does the same on its own), and the project's `v<version>` comes from the full run that finishes the set. Without `--execute` it only checks that the tag
could be made. `releaseant tag --sha $SHA --version 0.1.0 [--execute]` does the same on its own.

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
- Publish metadata the registry refuses (see Hello) stops the run before anything is written, with every problem
  listed; the quality fields it accepts without are warnings, and `--require-metadata` makes them errors.

More, in the clone: `docs/DESIGN.md` (how it works and why), `ROADMAP.md`, `CHANGELOG.md`.

Part of the ant family of shared building blocks. Licensed under MIT OR Apache-2.0.
