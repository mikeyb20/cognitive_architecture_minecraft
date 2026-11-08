//! Sim world (spec §21; decisions #32, #34): a seeded gridworld with Minecraft
//! rules in miniature, the same host interface as the Mineflayer bot, and a
//! Craftium-style reset/step API. Built in step 3 (spec §27).

pub mod craft;
pub mod geom;
pub mod map;
pub mod mob;
pub mod planner;
pub mod rules;
pub mod world;

pub use host_api;
pub use rules::Rules;
pub use world::{Death, SimWorld};
