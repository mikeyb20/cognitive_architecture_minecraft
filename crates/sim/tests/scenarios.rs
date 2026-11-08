//! Trip scenarios and incubation (spec §20.1, §21): each puts what the
//! agent needs where the scenario says, and only there.

use host_api::Pos;
use sim::geom::{cheb, offset};
use sim::map::{Biome, Tile};
use sim::mob::MobKind;
use sim::scenario::trips::{
    ERRAND_DISTANCE, ERRAND_PHASE, EXPEDITION, EXPEDITION_FOOD, EXPEDITION_HOME, FAR_FOOD,
    GUARD_AT, GUARDED_FOOD, HUNGRY_START, INCUBATION_SPLIT, LANE_Z, SITE_PIGS, SITE_RADIUS, TRIPS,
    WEST_START, east,
};
use sim::scenario::{DAY, NAMES, Setup, build};
use sim::{Rules, SimWorld};

const SEEDS: [u64; 4] = [0, 1, 7, 42];

fn make(name: &str, seed: u64) -> Setup {
    build(name, Rules::default(), seed).unwrap()
}

/// Every tile matching `which`.
fn tiles(w: &SimWorld, which: impl Fn(Tile) -> bool) -> Vec<Pos> {
    let n = w.map.size;
    (0..n)
        .flat_map(|z| (0..n).map(move |x| Pos { x, y: 0, z }))
        .filter(|p| which(w.map.get(*p)))
        .collect()
}

/// Some tiles match, and all of them sit in the site around `at`.
fn only_at(w: &SimWorld, at: Pos, which: impl Fn(Tile) -> bool) -> usize {
    let found = tiles(w, which);
    assert!(!found.is_empty());
    for p in &found {
        assert!(cheb(*p, at) <= SITE_RADIUS, "{p:?} is outside the site");
    }
    found.len()
}

fn pigs(w: &SimWorld) -> Vec<Pos> {
    let pigs = w.mobs.iter().filter(|m| m.kind == MobKind::Pig);
    pigs.map(|m| m.pos).collect()
}

/// The lane from `from` is walkable for `len` tiles east.
fn road_open(w: &SimWorld, from: Pos, len: i32) {
    for d in 0..=len {
        let p = east(from, d);
        assert!(w.map.get(p).walkable(), "{p:?} blocks the road");
    }
}

fn is_tree(t: Tile) -> bool {
    matches!(t, Tile::Tree { .. })
}

#[test]
fn every_trip_is_a_named_scenario() {
    for (n, _) in TRIPS {
        assert!(NAMES.contains(&n), "{n}");
        assert_eq!(make(n, 0).name, n);
    }
    assert_eq!(east(WEST_START, 3), Pos { x: 11, y: 0, z: 48 });
    assert_eq!(WEST_START.z, LANE_Z);
}

#[test]
fn far_food_puts_all_food_sixty_tiles_out() {
    assert_eq!(FAR_FOOD, 60);
    for seed in SEEDS {
        let s = make("far_food", seed);
        let w = &s.world;
        let site = east(WEST_START, FAR_FOOD);
        assert_eq!(w.body.pos, WEST_START);
        assert_eq!(
            only_at(w, site, |t| t == Tile::Wheat),
            10,
            "two rows of five"
        );
        let wheat = tiles(w, |t| t == Tile::Wheat);
        assert!(
            wheat.iter().all(|p| p.z < site.z),
            "wheat north, pigs south"
        );
        let pigs = pigs(w);
        assert_eq!(pigs.len(), SITE_PIGS);
        assert!(pigs.iter().all(|p| cheb(*p, site) <= 1 && p.z > site.z));
        assert!(pigs.iter().all(|p| w.map.get(*p).walkable()));
        road_open(w, WEST_START, FAR_FOOD);
        for p in sim::geom::square(WEST_START, 1) {
            assert!(w.map.get(p).walkable(), "a clear start");
        }
        assert_eq!((s.max_ticks, w.tick, w.body.food), (3 * DAY, 0, 1.0));
        assert!(!s.player.present);
    }
}

#[test]
fn dusk_errand_asks_for_a_log_near_sunset() {
    for seed in SEEDS {
        let s = make("dusk_errand", seed);
        let w = &s.world;
        let start = (ERRAND_PHASE * DAY as f32) as u64;
        assert_eq!((w.tick, start), (start, 840));
        assert_eq!(s.player.commands, vec![(start, "oak_log".to_string())]);
        assert!(s.player.present);
        let grove = east(WEST_START, ERRAND_DISTANCE);
        assert_eq!(only_at(w, grove, is_tree), 20, "the lane row stays open");
        let full = Tile::Tree {
            chops_left: Rules::default().tree_chops,
        };
        assert!(tiles(w, is_tree).iter().all(|p| w.map.get(*p) == full));
        road_open(w, WEST_START, ERRAND_DISTANCE + SITE_RADIUS);
        assert_eq!(s.max_ticks, 2 * DAY);
    }
}

#[test]
fn hostile_at_food_guards_the_only_food_in_the_dark() {
    const { assert!(GUARD_AT < GUARDED_FOOD) };
    for seed in SEEDS {
        let mut s = make("hostile_at_food", seed);
        let site = east(WEST_START, GUARDED_FOOD);
        let guard = east(WEST_START, GUARD_AT);
        let w = &mut s.world;
        assert_eq!(w.body.food, HUNGRY_START);
        assert_eq!(w.map.biome(site), Biome::Cave);
        assert_eq!(w.map.biome(guard), Biome::Cave, "one dark cell");
        assert_ne!(w.map.biome(WEST_START), Biome::Cave);
        only_at(w, site, |t| t == Tile::Wheat);
        assert_eq!(pigs(w).len(), SITE_PIGS);
        let zombies: Vec<Pos> = w
            .mobs
            .iter()
            .filter(|m| m.kind == MobKind::Zombie)
            .map(|m| m.pos)
            .collect();
        assert_eq!(zombies, vec![guard]);
        road_open(w, WEST_START, GUARDED_FOOD);
        assert_eq!(s.max_ticks, DAY);
        // By day it stays in the dark, between the agent and the food.
        use host_api::World;
        let w = &mut s.world;
        w.step_world(200);
        let z: Vec<Pos> = w
            .mobs
            .iter()
            .filter(|m| m.kind.hostile())
            .map(|m| m.pos)
            .collect();
        assert!(!z.is_empty(), "the guard is still there");
        assert!(z.iter().all(|p| w.map.biome(*p) == Biome::Cave));
    }
}

#[test]
fn expedition_needs_packed_food_for_far_iron() {
    assert_eq!(EXPEDITION, 80);
    for seed in SEEDS {
        let s = make("expedition", seed);
        let w = &s.world;
        let ore = east(WEST_START, EXPEDITION);
        only_at(w, ore, |t| t == Tile::IronOre);
        assert_eq!(EXPEDITION_HOME, 5);
        let home = east(WEST_START, EXPEDITION_HOME);
        only_at(w, home, |t| t == Tile::Wheat);
        assert!(pigs(w).iter().all(|p| cheb(*p, home) <= 1));
        assert_eq!(w.body.inv.count("stone_pickaxe"), 1);
        assert_eq!(w.body.food, EXPEDITION_FOOD);
        assert_eq!(s.player.commands, vec![(0, "raw_iron".to_string())]);
        road_open(w, WEST_START, EXPEDITION);
        assert_eq!(s.max_ticks, 3 * DAY);
    }
}

#[test]
fn incubation_splits_wood_and_stone_out_of_sight() {
    let view = Rules::default().view_radius;
    assert!(
        INCUBATION_SPLIT > view + SITE_RADIUS,
        "out of sight of each other"
    );
    for seed in SEEDS {
        let s = make("incubation", seed);
        let w = &s.world;
        let fresh = SimWorld::new(Rules::default(), seed);
        assert_eq!(w.body.pos, fresh.body.pos, "the usual start");
        let west = offset(w.body.pos, -INCUBATION_SPLIT, 0);
        let east_site = east(w.body.pos, INCUBATION_SPLIT);
        only_at(w, west, is_tree);
        only_at(w, east_site, |t| t == Tile::Stone);
        road_open(w, west, 2 * INCUBATION_SPLIT);
        let kept: Vec<Pos> = pigs(w);
        assert!(kept.iter().all(|p| w.map.get(*p).walkable()));
        let walkable_before = fresh
            .mobs
            .iter()
            .filter(|m| m.kind == MobKind::Pig && w.map.get(m.pos).walkable())
            .count();
        assert_eq!(
            kept.len(),
            walkable_before,
            "pigs stay where they can stand"
        );
        assert!(!s.player.present && s.max_ticks == 3 * DAY);
    }
}

#[test]
fn trips_replay_from_the_seed() {
    for (n, _) in TRIPS {
        let (a, b) = (make(n, 5), make(n, 5));
        let (ta, tb) = (tiles(&a.world, |_| true), tiles(&b.world, |_| true));
        assert_eq!(ta.len(), tb.len());
        let same = ta
            .iter()
            .all(|p| a.world.map.get(*p) == b.world.map.get(*p));
        assert!(same, "{n}");
        assert_eq!(pigs(&a.world), pigs(&b.world), "{n}");
    }
}
