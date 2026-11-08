//! The brain (spec Part B). Modules mirror the sections of `01-definitions.md`,
//! so every rule can be found from its definition (spec §1).
//!
//! Rules for this crate:
//! - No IO. Deterministic given a seed.
//! - Every rule cites its 01 or 00 section in a doc comment.
//! - Anything 01 leaves open is a config value or a swappable trait, never a
//!   silent definition change (DEF CHANGE proposals go through 02-status).

pub mod activation;
pub mod agent;
pub mod body;
pub mod control;
pub mod derived;
pub mod direction;
pub mod memory;
pub mod modes;

/// Version of `01-definitions.md` this code implements (logged in run headers, spec §25).
pub const DEFS_VERSION: &str = "01 v3";
/// Version of the paper draft `00-framework-original.md` used for intent.
pub const PAPER_VERSION: &str = "00 v0.1";
/// Version of `06-sim-spec.md` this code follows.
pub const SPEC_VERSION: &str = "0.2";
