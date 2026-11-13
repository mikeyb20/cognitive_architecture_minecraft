//! Sim world (spec §21; decisions #32, #34): a seeded gridworld with Minecraft
//! rules in miniature, the same host interface as the Mineflayer bot, and a
//! Craftium-style reset/step API. Built in step 3 (spec §27).

pub mod geom;
pub mod map;
pub mod mob;
pub mod player;
pub mod rules;
pub mod scenario;
mod skills;
pub mod world;

pub use host_api;
/// The tech tree and its planner live in the contract (debt D1) so that
/// Arm A and the code tools run in any world; re-exported here.
pub use host_api::{craft, planner};
pub use rules::Rules;
pub use world::{Death, SimWorld};
