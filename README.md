# ReleaseAnt

Ordered, verified, resumable publishing of Rust crates and workspaces to crates.io, with read-back.

**Status: early, pre-1.0.** Published on crates.io since 0.0.44; a breaking change bumps the second number. The hello-world
below is run in CI on Linux, Windows and macOS, and strangers' trials of it (4 of 4) passed, all on Windows; expect rough edges
elsewhere and report them.

## Hello

You have a git repository of crates and want them published in dependency order. After installing (below), from inside the repository: three commands, the last one a dry run that uploads nothing.

ReleaseAnt works from a commit, never from your working tree, so it needs the commit's full SHA. Run it from
inside the repository (or add `--repo PATH`):

<!-- hello:begin -->
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

### The workspace those commands ran on

Two crates, in directories of those names (`mkdir -p releaseant-demo-core/src releaseant-demo-app/src`), each with an empty
`src/lib.rs` and a `README.md` (any text), committed in git (`git init`, `git add -A`,
`git commit`; if git asks who you are, set `user.name` and `user.email` first):

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
<!-- hello:end -->

The ledger is a plain file, one JSON event per line, appended before each action. A dry run writes it too, so
`.releaseant/` shows up as untracked in `git status`. Commit it if you want the record in git, or add it to
`.gitignore`; ReleaseAnt does not mind either way.

## Before it uploads anything: the metadata check

Before anything is uploaded, `plan` checks every crate's publish
metadata. What crates.io refuses is an **error** and stops the plan: no `description`, neither `license` nor
`license-file`, more than five `keywords` or one it does not accept, a category that is not one of its slugs, a
`readme` or `license-file` that is not there. What it accepts without is a **warning**, listed and not stopping
anything: no `repository`, README, `keywords`, `categories` or `rust-version`; name the ones you want enforced with
`--require-metadata readme,keywords,rust-version`, or commit them once in a `releaseant.toml` at the repository
root (`[metadata]`, `require = ["readme", "keywords", "rust-version"]`), which is read from the commit being
published and adds to the flag. Every problem of every crate is listed at once, so a set never dies
half-published at the crate that lacked one. ReleaseAnt also needs a dependency on a crate of the workspace
to name a `version` as well as a `path` (the demo's `releaseant-demo-app` does).

### A dependency that needs a newer Rust than the crate declares

A crate that says `rust-version = "1.70"` promises it builds on 1.70; if a dependency it cannot do without needs 1.85,
the promise is false and nothing says so until a user on 1.70 tries. `plan --check-rust-version` (and `publish
--check-rust-version`, which asks before the ledger is opened or anything is built) reads the `rust_version` the
index records for every published version and compares. It is an error (`RUST-VERSION`, exit 4) when no live version
that satisfies the requirement fits, or when a crate of the same set declares a newer one. It is only a `note` when the
newest match is too new but an older match fits, since a resolver that respects `rust-version` will find that one, and
for a dependency limited to a target. Dev and optional dependencies are skipped, a version that declares nothing is taken
to fit, and a crate that declares no `rust-version` promises nothing and is skipped (`--require-metadata rust-version`
makes that a requirement). Off unless you ask; it only reads the index.

### A crate name someone else owns

crates.io refuses a new crate whose name is taken by another owner only at upload, in the middle of a set.
`plan --check-names` asks first. For every crate whose planned version is not on the registry yet it reads the
sparse index (no such crate: free) and, for a name that exists, crates.io's owners list. Name whose crates are
yours with `--owner LOGIN` (repeatable; a user such as `ciresnave` or a team such as `github:org:team`) or
`[names]` `owners = ["ciresnave"]` in `releaseant.toml`; a name with at least one of your owners is fine, a name with
none is `NAME TAKEN` and the command exits 4. `publish --check-names` (same flags) does the same before anything is built or
uploaded, and is off unless you ask. A lookup that fails is `UNKNOWN` and refuses the same way, never
"free". Without any owner named it stops with exit 2 rather than call every existing name taken. It only reads, and
waits a second between owner requests, as crates.io asks.

## Install

From crates.io (Rust 1.88 or newer):

```console
cargo install releaseant
```

Or from a clone or an unpacked package (the build takes a few minutes; `cargo install` puts the binary in
`~/.cargo/bin`, which rustup already put on your `PATH`):

```console
git clone https://github.com/ThinkersJournal/releaseant
cd releaseant            # or the unpacked package's directory
cargo install --path .
```

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
  version. To go on, the release owner (whoever decides what goes up) adds one line to the ledger (`.releaseant/ledger.jsonl`) after checking that the
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

Two guards for an unattended release, both off by default. `--require-green` waits for the CI runs on the commit to
finish (`gh run list`; `--github OWNER/NAME` if origin is not a GitHub URL, `--ci-wait` seconds, default 2700) and
refuses unless every one succeeded: exit status 3 if a run failed or was cancelled, 4 if CI gave no answer in time. No
runs at all is not green. Which checks count: by default the ones branch protection of `main` requires
(`--protection-branch`), read from the forge, so a self-hosted run that does not gate the merge cannot hold a release
up; `--require-checks A,B` names them instead. Names are JOB names, as branch protection lists them, not workflow
names: Fuel's workflow `checkpoint-tests` has the job `Qwen3.5-0.8B Gated DeltaNet oracles (self-hosted)`, and
`--require-checks checkpoint-tests` reads as absent (exit 4), not as that workflow's result.
Either way it fails closed: a named check that is absent or unfinished is exit 4, one that failed is exit 3, every
other run is ignored. If protection requires nothing or cannot be read, every run on the commit is judged, as before,
and a line printed first says which rule ran. `--retries N` (with `--execute`; `--retry-pause` seconds, default 120) runs again after any
failure, up to N more times, and stops only when the index, asked afresh, lists every crate of the plan at its version
and not yanked; the run's own word is not enough. A version someone else put on the registry (held) or a checksum
that differs stops it at once, because running again cannot fix those.

A resume or a `--retries` run does not build again what the ledger already shows as verified at the same commit
in the last 12 hours (`--reuse-verified-hours N`; 0 verifies everything every time): the crate is recorded as `verified`
with `carried over, not built again: run <id> at <time>`, and the progress line says how many were carried over.
The first run at a commit always builds every crate.

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
