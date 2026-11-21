#!/bin/sh
# Builds the README's two-crate demo workspace in ./releaseant-demo (a new git repository, one commit),
# then run the three hello commands from inside it. Needs git; sets a repo-local identity if you have none.
set -e
mkdir releaseant-demo && cd releaseant-demo
mkdir -p releaseant-demo-core/src releaseant-demo-app/src
cat > Cargo.toml <<'EOF'
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
EOF
cat > releaseant-demo-core/Cargo.toml <<'EOF'
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
EOF
cat > releaseant-demo-app/Cargo.toml <<'EOF'
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
EOF
for d in releaseant-demo-core releaseant-demo-app; do : > $d/src/lib.rs; echo demo > $d/README.md; done
git init -q
git config user.name >/dev/null || git config user.name "ReleaseAnt demo"
git config user.email >/dev/null || git config user.email demo@example.invalid
git add -A && git commit -q -m init
echo "demo ready: cd releaseant-demo, then see examples/hello.sh"
