//! Sim world rules, one test per rule (spec §21): time, body, mobs, paths,
//! perception and the world API. Skills are in `actions.rs` and
//! `candidates.rs`.

mod common;

use common::*;
use host_api::{AgentId, Host, SeenKind, Skill, ThingId, World};
use sim::map::{Biome, Map, Tile};
use sim::mob::{Mob, MobKind};
use sim::{Death, Rules, SimWorld};

// ---------- time and body ----------

#[test]
fn day_is_2400_ticks_and_night_is_the_second_half() {
    let mut w = flat();
    assert_eq!(w.day_phase(), 0.0);
    w.step_world(1199);
    assert!(!w.is_night());
    w.step_world(1);
    assert!(w.is_night());
    assert_eq!(w.day_phase(), 0.5);
    w.step_world(1200);
    assert_eq!(w.tick, 2400);
    assert!(!w.is_night());
    assert_eq!(w.day_phase(), 0.0);
}

#[test]
fn food_drops_one_bar_per_day() {
    let mut w = flat();
    w.step_world(1);
    assert_eq!(w.body.food, 1.0 - 1.0 / 2400.0);
    w.step_world(1199);
    assert!((w.body.food - 0.5).abs() < 1e-4, "{}", w.body.food);
}

#[test]
fn starving_costs_health_and_kills_with_a_starved_tag() {
    let mut w = flat_with(Rules {
        food_decay: 0.0,
        ..exact()
    });
    w.body.food = 0.0;
    w.step_world(1);
    assert_eq!(w.body.health, 1.0 - 1.0 / 300.0);
    w.body.health = 0.005;
    w.step_world(5);
    assert_eq!(w.body.health, 0.0);
    assert_eq!(w.body.death, Some(Death::Starved));
    assert!(!w.alive());
    let t = w.tick;
    w.step_world(50);
    assert_eq!(w.tick, t, "a dead world stops");
}

#[test]
fn health_regenerates_only_when_well_fed() {
    let rules = Rules {
        food_decay: 0.0,
        regen: 1.0 / 600.0,
        ..exact()
    };
    let mut w = flat_with(rules.clone());
    w.body.health = 0.5;
    w.body.food = 0.8;
    w.step_world(1);
    assert_eq!(
        w.body.health,
        0.5 + 1.0 / 600.0,
        "food at the threshold regenerates"
    );
    let mut w = flat_with(rules);
    w.body.health = 0.5;
    w.body.food = 0.79;
    w.step_world(10);
    assert_eq!(w.body.health, 0.5);
    w.body.health = 0.9999;
    w.body.food = 1.0;
    w.step_world(10);
    assert_eq!(w.body.health, 1.0, "capped at 1");
}

#[test]
fn food_floor_is_zero() {
    let mut w = flat_with(Rules {
        food_decay: 0.3,
        ..exact()
    });
    w.body.food = 0.5;
    w.step_world(2);
    assert_eq!(w.body.food, 0.0);
}

// ---------- mobs ----------

#[test]
fn night_spawns_zombies_in_a_ring_up_to_the_cap() {
    // A plain big enough for the whole 24–40 ring.
    let mut w = SimWorld::with_map(
        Rules {
            night_spawn: 1.0,
            ..exact()
        },
        1,
        Map::filled(96, Tile::Grass, Biome::Plains),
        p(48, 48),
    );
    w.tick = 1200;
    w.step_world(1);
    assert_eq!(w.mobs.len(), 1);
    let d = sim::geom::cheb(w.mobs[0].pos, w.body.pos);
    assert!((24..=40).contains(&d), "spawned {d} tiles away");
    assert_eq!(w.mobs[0].kind, MobKind::Zombie);
    w.step_world(30);
    assert_eq!(w.mobs.len(), 6, "max_hostiles");
}

#[test]
fn no_spawns_by_day_outside_caves() {
    let mut w = flat_with(Rules {
        night_spawn: 1.0,
        ..exact()
    });
    w.step_world(100);
    assert!(w.mobs.is_empty());
}

#[test]
fn caves_spawn_by_day() {
    let mut map = Map::filled(96, Tile::CaveFloor, Biome::Cave);
    map.set_biome(p(0, 0), Biome::Plains);
    let mut w = SimWorld::with_map(
        Rules {
            cave_spawn: 1.0,
            ..exact()
        },
        1,
        map,
        p(48, 48),
    );
    w.step_world(1);
    assert_eq!(w.mobs.len(), 1);
}

#[test]
fn spawn_gives_up_without_open_ground() {
    let mut map = Map::filled(48, Tile::Stone, Biome::Plains);
    map.set(p(24, 24), Tile::Grass);
    let mut w = SimWorld::with_map(
        Rules {
            night_spawn: 1.0,
            ..exact()
        },
        1,
        map,
        p(24, 24),
    );
    w.tick = 1200;
    w.step_world(20);
    assert!(w.mobs.is_empty());
}

#[test]
fn dawn_clears_zombies_outside_caves_only() {
    let mut map = Map::filled(48, Tile::Grass, Biome::Plains);
    map.set_biome(p(0, 0), Biome::Cave);
    let mut w = SimWorld::with_map(exact(), 1, map, p(30, 30));
    w.tick = 2399;
    zombie(&mut w, 2, 2);
    zombie(&mut w, 40, 40);
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(44, 44), t));
    w.step_world(1);
    assert!(!w.is_night());
    let kinds: Vec<_> = w.mobs.iter().map(|m| m.kind).collect();
    assert_eq!(kinds, vec![MobKind::Zombie, MobKind::Pig]);
    assert!(w.mobs[0].pos.x < 12, "the cave zombie stayed");
}

#[test]
fn zombies_chase_one_tile_per_six_ticks_and_stop_adjacent() {
    let mut w = flat();
    w.tick = 1200;
    zombie(&mut w, 29, 24);
    w.step_world(1);
    assert_eq!(w.mobs[0].pos, p(28, 24));
    w.step_world(5);
    assert_eq!(w.mobs[0].pos, p(28, 24));
    w.step_world(1);
    assert_eq!(w.mobs[0].pos, p(27, 24));
    w.step_world(30);
    assert_eq!(w.mobs[0].pos, p(25, 24), "stops next to the agent");
}

#[test]
fn zombies_take_the_longer_axis_then_go_around() {
    let mut w = flat();
    w.tick = 1200;
    zombie(&mut w, 27, 26);
    w.map.set(p(26, 26), Tile::Stone);
    w.step_world(1);
    assert_eq!(w.mobs[0].pos, p(27, 25), "x blocked, so it steps on z");
}

#[test]
fn zombies_path_around_a_wall_the_greedy_chase_cannot_pass() {
    // #62: a wall at x = 26 from z = 20 to 28 sits between the zombie and
    // the agent at (24, 24). Two greedy steps would stall at (27, 24); the
    // shortest path goes round an end of the wall.
    let mut w = flat();
    w.tick = 1200;
    for z in 20..=28 {
        w.map.set(p(26, z), Tile::Stone);
    }
    zombie(&mut w, 28, 24);
    w.step_world(1);
    assert_eq!(w.mobs[0].pos, p(27, 24), "the greedy step first");
    w.step_world(6);
    assert_eq!(
        w.mobs[0].pos,
        p(27, 25),
        "then along the wall to the nearer end"
    );
    w.step_world(120);
    let m = w.mobs[0].pos;
    assert!(
        (m.x - 24).abs().max((m.z - 24).abs()) <= 1,
        "reached the agent: {m:?}"
    );
}

#[test]
fn a_zombie_backs_out_of_a_pocket_to_chase() {
    // Each pocket opens away from the agent at (24, 24), on -x and on -z,
    // so the first step is one the greedy chase never takes.
    let mut w = flat();
    w.tick = 1200;
    for q in [p(22, 23), p(22, 24), p(22, 25), p(21, 23), p(21, 25)] {
        w.map.set(q, Tile::Stone);
    }
    for q in [p(23, 22), p(24, 22), p(25, 22), p(23, 21), p(25, 21)] {
        w.map.set(q, Tile::Stone);
    }
    zombie(&mut w, 21, 24);
    zombie(&mut w, 24, 21);
    w.step_world(1);
    assert_eq!(w.mobs[0].pos, p(20, 24), "out on -x");
    assert_eq!(w.mobs[1].pos, p(24, 20), "out on -z");
}

#[test]
fn a_walled_in_zombie_stays_put_and_pigs_never_chase() {
    let mut w = flat();
    w.tick = 1200;
    for q in [p(29, 24), p(31, 24), p(30, 23), p(30, 25)] {
        w.map.set(q, Tile::Stone);
    }
    zombie(&mut w, 30, 24);
    zombie(&mut w, 27, 24);
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(24, 30), t));
    w.step_world(60);
    assert_eq!(
        w.mobs[0].pos,
        p(30, 24),
        "no path, so the greedy step is blocked"
    );
    let pig = w.mobs[2].pos;
    assert_ne!(pig, p(24, 25), "the pig wanders: {pig:?}");
}

#[test]
fn zombies_hit_every_20_ticks_unless_sheltered() {
    let mut w = flat();
    w.tick = 1200;
    zombie(&mut w, 25, 24);
    w.step_world(1);
    assert!(close(w.body.health, 0.9));
    w.step_world(19);
    assert!(close(w.body.health, 0.9));
    w.step_world(1);
    assert!(close(w.body.health, 0.8));
    let mut w = flat();
    w.tick = 1200;
    w.map.set(p(24, 24), Tile::Shelter);
    zombie(&mut w, 25, 24);
    w.step_world(200);
    assert_eq!(w.body.health, 1.0);
    assert_eq!(w.mobs[0].pos, p(25, 24));
}

#[test]
fn mobs_kill_with_a_mob_tag() {
    let mut w = flat();
    w.tick = 1200;
    w.body.health = 0.1;
    zombie(&mut w, 25, 24);
    w.step_world(1);
    assert_eq!(w.body.death, Some(Death::Mob));
    assert_eq!(w.body.health, 0.0);
}

#[test]
fn zombies_never_enter_a_shelter_or_leave_a_cave_by_day() {
    let mut map = Map::filled(48, Tile::Grass, Biome::Plains);
    map.set_biome(p(0, 0), Biome::Cave);
    let mut w = SimWorld::with_map(exact(), 1, map, p(14, 5));
    zombie(&mut w, 11, 5);
    w.step_world(60);
    assert_eq!(w.mobs[0].pos, p(11, 5), "sunlight keeps it in the cave");
    w.tick = 1200;
    w.step_world(13);
    assert_eq!(w.mobs[0].pos, p(13, 5), "at night it follows");
    let mut w = flat();
    w.tick = 1200;
    w.map.set(p(26, 24), Tile::Shelter);
    zombie(&mut w, 27, 24);
    w.step_world(1);
    assert_eq!(
        w.mobs[0].pos,
        p(27, 24),
        "blocked by the shelter, no side step"
    );
}

#[test]
fn pigs_wander_every_24_ticks() {
    let mut w = flat();
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(5, 5), t));
    w.step_world(1);
    let first = w.mobs[0].pos;
    assert_eq!(sim::geom::cheb(first, p(5, 5)), 1);
    w.step_world(23);
    assert_eq!(w.mobs[0].pos, first);
    w.step_world(1);
    assert_ne!(w.mobs[0].pos, first);
}

// ---------- paths ----------

#[test]
fn paths_go_around_obstacles() {
    let mut w = flat();
    assert_eq!(w.path_to(p(24, 24), false), Some(vec![]));
    let path = w.path_to(p(27, 24), false).unwrap();
    assert_eq!(path, vec![p(25, 24), p(26, 24), p(27, 24)]);
    assert_eq!(w.path_to(p(27, 24), true).unwrap().len(), 2);
    for z in 22..=26 {
        w.map.set(p(25, z), Tile::Water);
    }
    assert_eq!(w.path_to(p(26, 24), false).unwrap().len(), 8);
    let r = w.reach();
    assert_eq!(r.steps(p(26, 24)), Some(8));
    assert_eq!(r.steps(p(24, 24)), Some(0));
    assert_eq!(r.steps(p(25, 24)), None, "water");
    assert_eq!(r.steps_adjacent(p(25, 24)), Some(0), "standing next to it");
    assert_eq!(
        r.steps_adjacent(p(26, 24)),
        Some(7),
        "diagonal counts as next to"
    );
    assert_eq!(r.steps(p(24 + 21, 24)), None, "outside the search window");
    assert_eq!(r.steps(p(24 - 21, 24)), None);
    assert_eq!(r.steps(p(24, 24 + 21)), None);
    assert_eq!(r.steps(p(24, 24 - 21)), None);
    assert_eq!(r.steps(p(24, 24 + 20)), Some(20));
}

#[test]
fn unreachable_and_far_goals_have_no_path() {
    let mut w = flat();
    for (x, z) in [(30, 29), (29, 30), (31, 30), (30, 31)] {
        w.map.set(p(x, z), Tile::Water);
    }
    assert_eq!(w.path_to(p(30, 30), false), None);
    assert!(w.path_to(p(30, 30), true).is_some());
    assert_eq!(w.path_to(p(46, 24), false), None, "beyond PATH_RADIUS");
    assert_eq!(w.reach().steps(p(30, 30)), None);
    let w = flat();
    assert_eq!(w.path_to(p(-1, 24), false), None, "off the map");
}

// ---------- perception and novelty ----------

#[test]
fn perceive_lists_blocks_and_entities_nearest_first() {
    let mut w = flat();
    w.tick = 1199;
    w.map.set(p(26, 24), Tile::Wheat);
    w.map.set(p(24, 25), Tile::Stone);
    zombie(&mut w, 27, 27);
    w.step_world(1);
    w.mobs[0].pos = p(27, 27);
    let o = w.perceive();
    let names: Vec<_> = o.visible.iter().map(|s| s.thing.0.as_str()).collect();
    assert_eq!(names, ["block:stone", "block:wheat", "entity:zombie"]);
    assert_eq!(o.visible[0].distance, 1.0);
    assert_eq!(o.visible[2].kind, SeenKind::Entity { hostile: true });
    assert_eq!(o.agent, AgentId(0));
    assert_eq!((o.tick, o.position, o.day_phase), (1200, p(24, 24), 0.5));
    assert!(!o.sheltered && !o.resting);
    zombie(&mut w, 24, 33);
    assert_eq!(w.perceive().visible.len(), 3, "9 tiles is out of view");
}

#[test]
fn perceive_caps_blocks_but_keeps_entities() {
    let mut w = SimWorld::with_map(
        exact(),
        1,
        Map::filled(48, Tile::Stone, Biome::Hills),
        p(24, 24),
    );
    w.map.set(p(24, 24), Tile::Grass);
    zombie(&mut w, 32, 32);
    let o = w.perceive();
    assert_eq!(o.visible.len(), 48 + 1);
    assert!(o.visible.iter().any(|s| s.thing.0 == "entity:zombie"));
}

#[test]
fn perceive_skips_tiles_off_the_map() {
    let mut w = SimWorld::with_map(
        exact(),
        1,
        Map::filled(48, Tile::Grass, Biome::Plains),
        p(0, 0),
    );
    assert!(w.perceive().visible.is_empty());
}

#[test]
fn damage_is_reported_once() {
    let mut w = flat();
    w.tick = 1200;
    zombie(&mut w, 25, 24);
    w.step_world(1);
    w.mobs.clear();
    assert!(close(w.perceive().damage_taken, 0.1));
    assert_eq!(w.perceive().damage_taken, 0.0);
}

#[test]
fn new_regions_and_sights_are_new_once() {
    let mut w = flat();
    w.map.set(p(40, 24), Tile::IronOre);
    assert_eq!(w.region_visits(p(24, 24)), 1);
    assert_eq!(w.region_visits(p(-1, 0)), u32::MAX);
    assert_eq!(w.region_visits(p(0, 48)), u32::MAX);
    let o = w.execute(&at(Skill::MoveTo, 32, 24), FULL);
    assert_eq!(
        o.new_things,
        vec![
            ThingId("region:4,3".into()),
            ThingId("block:iron_ore".into())
        ]
    );
    assert_eq!(w.region_visits(p(32, 24)), 1);
    assert_eq!(w.region_visits(p(24, 24)), 8, "7 steps plus the start");
    let o = w.execute(&at(Skill::MoveTo, 24, 24), FULL);
    assert!(o.new_things.is_empty());
    assert_eq!(w.region_visits(p(32, 24)), 1);
}

#[test]
fn entities_in_view_are_new_once() {
    let mut w = flat();
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(26, 26), t));
    w.mobs[0].next_move = u64::MAX;
    let o = w.execute(&at(Skill::MoveTo, 25, 24), FULL);
    assert_eq!(o.new_things, vec![ThingId("entity:pig".into())]);
}

// ---------- world API ----------

#[test]
fn reset_and_host_follow_the_world_api() {
    let mut w = SimWorld::new(Rules::default(), 9);
    let first = w.perceive();
    w.execute(&act(Skill::Shelter, None), FULL);
    w.reset(
        9,
        &host_api::Scenario {
            name: "first_night".into(),
        },
    );
    assert_eq!(w.tick, 0);
    assert_eq!(w.host(AgentId(0)).perceive(), first);
    assert!(w.mobs.iter().all(|m| m.kind == MobKind::Pig));
    assert!(!w.mobs.is_empty(), "plains have pigs");
}

#[test]
#[should_panic(expected = "one agent")]
fn only_agent_zero_exists() {
    let mut w = flat();
    let _ = w.host(AgentId(1));
}

#[test]
fn food_values() {
    let r = Rules::default();
    assert_eq!(sim::world::food_value(&r, "bread"), Some(0.5));
    assert_eq!(sim::world::food_value(&r, "porkchop"), Some(0.3));
    assert_eq!(sim::world::food_value(&r, "stick"), None);
}

// ---------- edges found by mutation testing ----------

#[test]
fn path_search_reaches_exactly_twenty_tiles() {
    let w = flat();
    assert_eq!(w.path_to(p(24, 44), false).map(|v| v.len()), Some(20));
    assert_eq!(w.path_to(p(4, 24), false).map(|v| v.len()), Some(20));
    let r = w.reach();
    assert_eq!(r.steps(p(4, 24)), Some(20), "west edge of the window");
    assert_eq!(r.steps(p(24, 4)), Some(20), "north edge of the window");
    assert_eq!(r.steps(p(44, 24)), Some(20), "east edge of the window");
}

#[test]
fn zombies_spawn_all_around_between_24_and_40_tiles() {
    // Mobs are frozen after their first step, which comes in the tick they
    // spawn: a zombie spawned 24–40 tiles out is found 23–40 tiles out (a
    // diagonal step-in on one axis can leave it at 40).
    let mut fresh = SimWorld::with_map(
        Rules {
            night_spawn: 1.0,
            max_hostiles: 1000,
            mob_move_ticks: 1_000_000,
            ..exact()
        },
        1,
        Map::filled(96, Tile::Grass, Biome::Plains),
        p(48, 48),
    );
    fresh.tick = 1200;
    fresh.step_world(400);
    let rel: Vec<(i32, i32)> = fresh
        .mobs
        .iter()
        .map(|m| (m.pos.x - 48, m.pos.z - 48))
        .collect();
    let ring: Vec<i32> = rel.iter().map(|(x, z)| x.abs().max(z.abs())).collect();
    assert!(
        ring.contains(&23) && ring.contains(&39),
        "both ends of the ring: {ring:?}"
    );
    assert!(ring.iter().all(|r| (23..=40).contains(r)), "{ring:?}");
    for (sx, sz) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
        assert!(
            rel.iter()
                .any(|(x, z)| x.signum() == sx && z.signum() == sz),
            "quadrant {sx},{sz}"
        );
    }
}

#[test]
fn pigs_wander_every_way_and_never_chase() {
    let mut w = flat();
    w.body.pos = p(2, 2);
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(24, 24), t));
    let mut seen = std::collections::HashSet::new();
    let mut last = w.mobs[0].pos;
    for _ in 0..80 {
        w.step_world(24);
        let now = w.mobs[0].pos;
        seen.insert((now.x - last.x, now.z - last.z));
        last = now;
    }
    for d in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        assert!(seen.contains(&d), "never moved {d:?}");
    }
    let mut chased = 0;
    for seed in 0..8 {
        let mut w = SimWorld::with_map(
            exact(),
            seed,
            Map::filled(48, Tile::Grass, Biome::Plains),
            p(24, 24),
        );
        let t = w.tick;
        w.mobs.push(Mob::new(MobKind::Pig, p(28, 24), t));
        w.step_world(1);
        chased += usize::from(w.mobs[0].pos == p(27, 24));
    }
    assert!(chased < 8, "pigs walk at random, not at the agent");
}

#[test]
fn zombies_lose_interest_beyond_40_tiles() {
    let mut caught = 0;
    for seed in 0..8 {
        let mut w = SimWorld::with_map(
            exact(),
            seed,
            Map::filled(96, Tile::Grass, Biome::Plains),
            p(4, 48),
        );
        w.tick = 1200;
        zombie(&mut w, 45, 48);
        w.step_world(1);
        caught += usize::from(w.mobs[0].pos == p(44, 48));
    }
    assert!(caught < 8, "41 tiles away it wanders");
    let mut w = SimWorld::with_map(
        exact(),
        1,
        Map::filled(96, Tile::Grass, Biome::Plains),
        p(4, 48),
    );
    w.tick = 1200;
    zombie(&mut w, 44, 48);
    w.step_world(1);
    assert_eq!(w.mobs[0].pos, p(43, 48), "40 tiles away it chases");
}

#[test]
fn zombies_at_a_diagonal_stay_put() {
    let mut w = flat();
    w.tick = 1200;
    zombie(&mut w, 25, 25);
    w.step_world(30);
    assert_eq!(w.mobs[0].pos, p(25, 25));
}

#[test]
fn nothing_off_the_map_is_noticed() {
    let mut w = SimWorld::with_map(
        exact(),
        1,
        Map::filled(48, Tile::Grass, Biome::Plains),
        p(0, 0),
    );
    let o = w.execute(&at(Skill::MoveTo, 0, 3), FULL);
    assert!(o.new_things.is_empty(), "{:?}", o.new_things);
}
