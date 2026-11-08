//! Scenarios that put distance between the agent and what it needs (spec
//! §20.1 "Sim scenarios this needs", §21): far food, dusk errand, hostile at
//! the food, expedition, and incubation (00 §9.4.1 claim 6).
//!
//! They share one layout: the agent starts on a lane (row `LANE_Z`) with a
//! cleared road east along it, and the scenario places sites on the lane at
//! set distances. Every distance and level here is proposed for the sim pilot.

use host_api::Pos;

use crate::craft;
use crate::geom::{offset, square};
use crate::map::{Biome, Map, Tile};
use crate::mob::{Mob, MobKind};
use crate::player::ScriptedPlayer;
use crate::rules::Rules;
use crate::world::SimWorld;

use super::{DAY, Setup};

/// The lane's row and the west-edge start most trips use.
pub const LANE_Z: i32 = 48;
pub const WEST_START: Pos = Pos {
    x: 8,
    y: 0,
    z: LANE_Z,
};
/// Far food: the only food is this far from the start (spec §20.1: 60).
pub const FAR_FOOD: i32 = 60;
/// Dusk errand: day phase at the start and when the command comes, and how
/// far the only trees are.
pub const ERRAND_PHASE: f32 = 0.35;
pub const ERRAND_DISTANCE: i32 = 30;
/// Hostile at the food: food at `GUARDED_FOOD`, a zombie at `GUARD_AT`, both
/// in one dark (cave) cell so the zombie stays by day; the agent starts
/// hungry at `HUNGRY_START` food.
pub const GUARDED_FOOD: i32 = 12;
pub const GUARD_AT: i32 = 8;
pub const HUNGRY_START: f32 = 0.3;
/// Expedition: the only iron ore is this far out (spec §20.1: 80). The agent
/// starts with a stone pickaxe and `EXPEDITION_FOOD` food, with food only
/// near the start, so it has to pack some.
pub const EXPEDITION: i32 = 80;
pub const EXPEDITION_FOOD: f32 = 0.4;
/// Incubation: the only trees and the only stone are this far west and east
/// of a centre start, out of sight of each other (view radius 8).
pub const INCUBATION_SPLIT: i32 = 20;
/// Pigs per food site, and the site's half-width in tiles.
pub const SITE_PIGS: usize = 3;
pub const SITE_RADIUS: i32 = 2;

/// Builds one scenario from rules and a seed.
pub type Builder = fn(Rules, u64) -> Setup;

/// The trip scenarios by name (all listed in `NAMES`).
pub const TRIPS: [(&str, Builder); 5] = [
    ("far_food", far_food),
    ("dusk_errand", dusk_errand),
    ("hostile_at_food", hostile_at_food),
    ("expedition", expedition),
    ("incubation", incubation),
];

/// A lane position `d` tiles east of `from`.
pub fn east(from: Pos, d: i32) -> Pos {
    offset(from, d, 0)
}

/// Far food (spec §20.1): all food sits `FAR_FOOD` tiles east.
pub fn far_food(rules: Rules, seed: u64) -> Setup {
    let mut map = lane_map(&rules, seed, WEST_START, FAR_FOOD);
    clear(&mut map, |t| t == Tile::Wheat);
    let site = east(WEST_START, FAR_FOOD);
    food_site(&mut map, site);
    let mut world = SimWorld::with_map(rules, seed, map, WEST_START);
    add_pigs(&mut world, site);
    setup("far_food", world, 3 * DAY)
}

/// Dusk errand (spec §20.1): near sunset the player asks for a log, and the
/// only trees are `ERRAND_DISTANCE` tiles east.
pub fn dusk_errand(rules: Rules, seed: u64) -> Setup {
    let mut map = lane_map(&rules, seed, WEST_START, ERRAND_DISTANCE);
    clear(&mut map, |t| matches!(t, Tile::Tree { .. }));
    let chops = rules.tree_chops;
    patch(
        &mut map,
        east(WEST_START, ERRAND_DISTANCE),
        Tile::Tree { chops_left: chops },
    );
    let mut world = SimWorld::with_map(rules, seed, map, WEST_START);
    let start = (ERRAND_PHASE * DAY as f32) as u64;
    world.tick = start;
    let mut s = setup("dusk_errand", world, 2 * DAY);
    s.player = ScriptedPlayer::commanding(vec![(start, "oak_log".into())]);
    s
}

/// Hostile at the food (spec §20.1, S5): the agent starts hungry and the
/// only food has a zombie between it and the agent, in a dark cell.
pub fn hostile_at_food(rules: Rules, seed: u64) -> Setup {
    let mut map = lane_map(&rules, seed, WEST_START, GUARDED_FOOD);
    clear(&mut map, |t| t == Tile::Wheat);
    let site = east(WEST_START, GUARDED_FOOD);
    let guard = east(WEST_START, GUARD_AT);
    // One 12×12 cell holds both (pinned by a test).
    map.set_biome(site, Biome::Cave);
    food_site(&mut map, site);
    let mut world = SimWorld::with_map(rules, seed, map, WEST_START);
    add_pigs(&mut world, site);
    world.mobs.push(Mob::new(MobKind::Zombie, guard, 0));
    world.body.food = HUNGRY_START;
    setup("hostile_at_food", world, DAY)
}

/// Expedition (spec §20.1, S4): the player asks for iron ore, found only
/// `EXPEDITION` tiles east; food is only near the start.
pub fn expedition(rules: Rules, seed: u64) -> Setup {
    let mut map = lane_map(&rules, seed, WEST_START, EXPEDITION);
    clear(&mut map, |t| matches!(t, Tile::Wheat | Tile::IronOre));
    let home = east(WEST_START, 2 * SITE_RADIUS + 1);
    food_site(&mut map, home);
    patch(&mut map, east(WEST_START, EXPEDITION), Tile::IronOre);
    let mut world = SimWorld::with_map(rules, seed, map, WEST_START);
    add_pigs(&mut world, home);
    craft::give(&mut world.body.inv, "stone_pickaxe", 1);
    world.body.food = EXPEDITION_FOOD;
    let mut s = setup("expedition", world, 3 * DAY);
    s.player = ScriptedPlayer::commanding(vec![(0, "iron_ore".into())]);
    s
}

/// Incubation (00 §9.4.1 claim 6; spec §21): the two things the first tools
/// need, wood and stone, are found only far apart, so the agent meets them
/// in separate episodes (my reading of "two needed facts").
pub fn incubation(rules: Rules, seed: u64) -> Setup {
    let base = SimWorld::new(rules.clone(), seed);
    let start = base.body.pos;
    let mut map = base.map.clone();
    let west = offset(start, -INCUBATION_SPLIT, 0);
    road(&mut map, west, 2 * INCUBATION_SPLIT);
    clear(&mut map, |t| matches!(t, Tile::Tree { .. } | Tile::Stone));
    let chops = rules.tree_chops;
    patch(&mut map, west, Tile::Tree { chops_left: chops });
    patch(&mut map, east(start, INCUBATION_SPLIT), Tile::Stone);
    let pigs = base.mobs.clone();
    let mut world = SimWorld::with_map(rules, seed, map, start);
    world.mobs = pigs
        .into_iter()
        .filter(|m| world.map.get(m.pos).walkable())
        .collect();
    setup("incubation", world, 3 * DAY)
}

fn setup(name: &'static str, world: SimWorld, max_ticks: u64) -> Setup {
    Setup {
        name,
        world,
        player: ScriptedPlayer::idle(),
        max_ticks,
        harass_until: 0,
        horizon: None,
    }
}

/// The seed's map with a clear start and a road `len` tiles east of it.
fn lane_map(rules: &Rules, seed: u64, start: Pos, len: i32) -> Map {
    let mut map = SimWorld::new(rules.clone(), seed).map;
    road(&mut map, start, len);
    map
}

/// Make the 3×3 around `from` and the row east of it for `len` tiles
/// walkable, so every site can be reached.
fn road(map: &mut Map, from: Pos, len: i32) {
    let row = (0..=len).map(|d| east(from, d));
    let spots: Vec<Pos> = square(from, 1).chain(row).collect();
    for p in spots {
        if !map.get(p).walkable() {
            map.set(p, Tile::Grass);
        }
    }
}

/// Turn every tile matching `which` into grass.
fn clear(map: &mut Map, which: impl Fn(Tile) -> bool) {
    for z in 0..map.size {
        for x in 0..map.size {
            let p = Pos { x, y: 0, z };
            if which(map.get(p)) {
                map.set(p, Tile::Grass);
            }
        }
    }
}

/// Fill the square of `SITE_RADIUS` around `at` with `tile`, off the lane
/// row so the road stays open.
fn patch(map: &mut Map, at: Pos, tile: Tile) {
    for p in square(at, SITE_RADIUS) {
        if p.z != at.z {
            map.set(p, tile);
        }
    }
}

/// A food site: wheat on one side of the lane, grass for pigs on the other.
fn food_site(map: &mut Map, at: Pos) {
    for p in square(at, SITE_RADIUS) {
        map.set(p, if p.z < at.z { Tile::Wheat } else { Tile::Grass });
    }
}

/// Put `SITE_PIGS` pigs on the grass side of a site.
fn add_pigs(world: &mut SimWorld, at: Pos) {
    for i in 0..SITE_PIGS as i32 {
        let p = offset(at, i - 1, 1);
        world.mobs.push(Mob::new(MobKind::Pig, p, 0));
    }
}
