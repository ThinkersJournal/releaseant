//! ReleaseAnt: ordered, verified, resumable publishing of Rust crates and workspaces to crates.io, with read-back.
//!
//! This is the core library (ROADMAP R2): the publish set and order ([`plan`]), the append-only
//! [`ledger`], and the run loop ([`run`]) over a [`registry::Registry`] trait, so every behaviour is
//! testable against a fake. The design is `docs/DESIGN.md`; nothing here talks to cargo or crates.io
//! yet (R3).

pub mod ledger;
pub mod metadata;
pub mod plan;
pub mod registry;
pub mod run;
