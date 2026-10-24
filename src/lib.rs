//! ReleaseAnt: ordered, verified, resumable publishing of Rust crates and workspaces to crates.io, with read-back.
//!
//! The pure core (R2): the publish set and order ([`plan`]), the append-only [`ledger`], and the run
//! loop ([`run`]) over a [`registry::Registry`] trait, testable against a fake. The process layer
//! (R3): [`source`] (`git archive` of a commit), [`cargo`] (run cargo), [`classify`] (read cargo's
//! output against captured samples) and [`verify`] (build the extracted tarballs). The design is
//! `docs/DESIGN.md`.

pub mod cargo;
pub mod classify;
pub mod ledger;
pub mod metadata;
pub mod plan;
pub mod registry;
pub mod run;
pub mod source;
pub mod verify;
