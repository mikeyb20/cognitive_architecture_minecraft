//! Held-out terrain (spec §27 step 15; plan §5.0 transfer; #58): a world
//! with no trees, so the shelter recipe the teacher shows (chop a log,
//! craft planks, build) cannot be followed, but the shelter test can still
//! be met: the agent holds a wooden pickaxe, and four cobblestone build a
//! hut that passes.

use crate::craft;
use crate::map::Tile;
use crate::player::ScriptedPlayer;
use crate::rules::Rules;
use crate::world::SimWorld;

use super::trips::{replace, setup};
use super::{DAY, Setup};

/// The goal the player names at the start (the sim teacher's goal).
pub const HELD_OUT_GOAL: &str = host_api::ontology::SHELTER_GOAL;

/// Held-out terrain: the seed's map with every tree turned to stone (the
/// same tiles stay blocked, so paths are unchanged), a wooden pickaxe in
/// hand, and the player asking for a shelter at tick 0.
pub fn held_out(rules: Rules, seed: u64) -> Setup {
    let base = SimWorld::new(rules.clone(), seed);
    let start = base.body.pos;
    let mut map = base.map.clone();
    replace(&mut map, |t| matches!(t, Tile::Tree { .. }), Tile::Stone);
    let mut world = SimWorld::with_map(rules, seed, map, start);
    world.mobs = base.mobs;
    craft::give(&mut world.body.inv, "wooden_pickaxe", 1);
    let mut s = setup("held_out", world, DAY);
    s.player = ScriptedPlayer::commanding(vec![(0, HELD_OUT_GOAL.into())]);
    s
}
