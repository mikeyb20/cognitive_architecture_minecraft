//! The Rust side of the Minecraft host (spec §22). The TypeScript bot is
//! the body: it reports what it sees and runs skills. This crate keeps the
//! decisions about that data next to the shared tech tree and command queue:
//! where things were seen ([`places`]) and what can be done now
//! ([`candidates`]). Every arm and the brain see the same candidates.

pub mod candidates;
pub mod places;

pub use candidates::{McParams, candidates};
pub use places::{PlaceParams, Places};

use host_api::Pos;

/// Straight-line distance in blocks.
pub(crate) fn dist(a: Pos, b: Pos) -> f32 {
    let d = |u: i32, v: i32| (u - v) as f32;
    (d(a.x, b.x).powi(2) + d(a.y, b.y).powi(2) + d(a.z, b.z).powi(2)).sqrt()
}
