//! Sim world rules, one test per rule (spec §21). Most tests use a flat
//! hand-built map and `base_success = 1` so every number is exact.

use host_api::{
    AgentId, Candidate, Host, Intensity, Inventory, Pos, SeenKind, Skill, Target, TaskType,
    ThingId, World,
};
use sim::map::{Biome, Map, Tile};
use sim::mob::{Mob, MobKind};
use sim::{Death, Rules, SimWorld};

const FULL: Intensity = Intensity {
    effort: 1.0,
    focus: 1.0,
    external_drive: false,
};

fn p(x: i32, z: i32) -> Pos {
    Pos { x, y: 0, z }
}

/// Exact rules: no random failure, no night spawns unless a test turns them on.
fn exact() -> Rules {
    Rules {
        base_success: 1.0,
        night_spawn: 0.0,
        cave_spawn: 0.0,
        regen: 0.0,
        threat_penalty: 1.0,
        ..Rules::default()
    }
}

/// A 48×48 grass plain with the agent at (24, 24).
fn flat_with(rules: Rules) -> SimWorld {
    SimWorld::with_map(
        rules,
        1,
        Map::filled(48, Tile::Grass, Biome::Plains),
        p(24, 24),
    )
}

fn flat() -> SimWorld {
    flat_with(exact())
}

fn act(skill: Skill, target: Option<Target>) -> Candidate {
    Candidate {
        id: 0,
        skill,
        target,
        task_type: TaskType::Travel,
        est_cost: 0.0,
        est_risk: 0.0,
        commanded: false,
    }
}

fn at(skill: Skill, x: i32, z: i32) -> Candidate {
    act(skill, Some(Target::Pos(p(x, z))))
}

fn item(skill: Skill, name: &str) -> Candidate {
    act(skill, Some(Target::Item(name.into())))
}

fn give(w: &mut SimWorld, name: &str, n: u32) {
    sim::craft::give(&mut w.body.inv, name, n);
}

fn zombie(w: &mut SimWorld, x: i32, z: i32) {
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Zombie, p(x, z), t));
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

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
    let mut w = flat_with(Rules {
        night_spawn: 1.0,
        ..exact()
    });
    w.tick = 1200;
    w.step_world(1);
    assert_eq!(w.mobs.len(), 1);
    let d = sim::mob::cheb(w.mobs[0].pos, w.body.pos);
    assert!((6..=12).contains(&d), "spawned {d} tiles away");
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
    let mut map = Map::filled(48, Tile::CaveFloor, Biome::Cave);
    map.set_biome(p(0, 0), Biome::Plains);
    let mut w = SimWorld::with_map(
        Rules {
            cave_spawn: 1.0,
            ..exact()
        },
        1,
        map,
        p(24, 24),
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
    assert_eq!(sim::mob::cheb(first, p(5, 5)), 1);
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

// ---------- execute: movement ----------

#[test]
fn moving_costs_walk_or_sprint_ticks_per_tile() {
    let mut w = flat();
    let o = w.execute(&at(Skill::MoveTo, 27, 24), FULL);
    assert!(o.success && !o.blocked);
    assert_eq!(
        (o.duration_ticks, o.progress),
        (6, 1.0),
        "sprint: 2 ticks/tile"
    );
    assert_eq!(w.body.pos, p(27, 24));
    let walk = Intensity {
        effort: 0.59,
        ..FULL
    };
    let o = w.execute(&at(Skill::Explore, 24, 24), walk);
    assert_eq!(o.duration_ticks, 12, "walk: 4 ticks/tile");
    let edge = Intensity {
        effort: 0.6,
        ..FULL
    };
    assert_eq!(
        w.execute(&at(Skill::MoveTo, 25, 24), edge).duration_ticks,
        2
    );
    let slow = Intensity {
        effort: 0.05,
        ..FULL
    };
    assert_eq!(
        w.execute(&at(Skill::Flee, 27, 24), slow).duration_ticks,
        4,
        "flee always sprints"
    );
}

#[test]
fn every_action_takes_at_least_a_tick() {
    let mut w = flat();
    let o = w.execute(&at(Skill::MoveTo, 24, 24), FULL);
    assert!(o.success && !o.blocked);
    assert_eq!(o.duration_ticks, 1, "already there");
}

#[test]
fn below_min_effort_nothing_moves() {
    let mut w = flat();
    let o = w.execute(
        &at(Skill::MoveTo, 27, 24),
        Intensity {
            effort: 0.049,
            ..FULL
        },
    );
    assert!(o.blocked && !o.success);
    assert_eq!(o.duration_ticks, 1);
    assert_eq!(w.body.pos, p(24, 24));
}

#[test]
fn blocked_actions_cost_a_few_ticks() {
    let mut w = flat();
    for c in [
        act(Skill::MoveTo, None),
        at(Skill::MoveTo, 60, 60),
        act(Skill::Place, None),
        act(Skill::Chat, None),
        act(Skill::FollowPlayer, None),
        act(Skill::Chop, None),
        at(Skill::Chop, 25, 24),
        act(Skill::Attack, None),
        at(Skill::Attack, 30, 30),
        act(Skill::Eat, None),
        item(Skill::Eat, "stick"),
        item(Skill::Eat, "bread"),
        act(Skill::Craft, None),
        item(Skill::Craft, "diamond"),
        item(Skill::Craft, "stick"),
        act(Skill::Rest, None),
    ] {
        let t = w.tick;
        let o = w.execute(&c, FULL);
        assert!(o.blocked && !o.success, "{:?}", c.skill);
        assert_eq!(o.duration_ticks, 5, "{:?}", c.skill);
        assert_eq!(w.tick, t + 5);
    }
}

#[test]
fn the_dead_do_nothing() {
    let mut w = flat();
    w.body.death = Some(Death::Mob);
    let o = w.execute(&at(Skill::MoveTo, 25, 24), FULL);
    assert!(o.blocked);
    assert_eq!(o.duration_ticks, 0);
}

// ---------- execute: gathering ----------

#[test]
fn chopping_takes_three_logs_from_a_tree() {
    let mut w = flat();
    w.map.set(p(26, 24), Tile::Tree { chops_left: 3 });
    let o = w.execute(&at(Skill::Chop, 26, 24), FULL);
    assert!(o.success);
    assert_eq!(o.duration_ticks, 2 + 30);
    assert_eq!(o.items_gained.count("oak_log"), 1);
    assert_eq!(w.map.get(p(26, 24)), Tile::Tree { chops_left: 2 });
    assert!(o.new_things.contains(&ThingId("item:oak_log".into())));
    let o = w.execute(&at(Skill::Chop, 26, 24), FULL);
    assert_eq!(o.duration_ticks, 30, "already adjacent");
    assert!(o.new_things.is_empty());
    w.execute(&at(Skill::Chop, 26, 24), FULL);
    assert_eq!(w.map.get(p(26, 24)), Tile::Grass);
    assert_eq!(w.body.inv.count("oak_log"), 3);
}

#[test]
fn mining_needs_the_right_pickaxe_and_is_faster_with_better_ones() {
    let mut w = flat();
    w.map.set(p(25, 24), Tile::Stone);
    w.map.set(p(23, 24), Tile::IronOre);
    assert!(w.execute(&at(Skill::Mine, 25, 24), FULL).blocked);
    give(&mut w, "wooden_pickaxe", 1);
    assert!(
        w.execute(&at(Skill::Mine, 23, 24), FULL).blocked,
        "iron needs stone"
    );
    let o = w.execute(&at(Skill::Mine, 25, 24), FULL);
    assert_eq!((o.success, o.duration_ticks), (true, 40));
    assert_eq!(o.items_gained.count("cobblestone"), 1);
    assert_eq!(w.map.get(p(25, 24)), Tile::Grass);
    give(&mut w, "stone_pickaxe", 1);
    let o = w.execute(&at(Skill::Mine, 23, 24), FULL);
    assert_eq!((o.success, o.duration_ticks), (true, 30));
    assert_eq!(o.items_gained.count("iron_ore"), 1);
    give(&mut w, "iron_pickaxe", 1);
    w.map.set(p(25, 24), Tile::Stone);
    assert_eq!(w.execute(&at(Skill::Mine, 25, 24), FULL).duration_ticks, 20);
}

#[test]
fn mined_cave_tiles_become_cave_floor() {
    let mut w = SimWorld::with_map(
        exact(),
        1,
        Map::filled(48, Tile::CaveFloor, Biome::Cave),
        p(24, 24),
    );
    w.map.set(p(25, 24), Tile::Stone);
    give(&mut w, "wooden_pickaxe", 1);
    w.execute(&at(Skill::Mine, 25, 24), FULL);
    assert_eq!(w.map.get(p(25, 24)), Tile::CaveFloor);
    w.map.set(p(25, 24), Tile::Tree { chops_left: 1 });
    w.execute(&at(Skill::Chop, 25, 24), FULL);
    assert_eq!(w.map.get(p(25, 24)), Tile::CaveFloor);
}

#[test]
fn farming_harvests_wheat() {
    let mut w = flat();
    w.map.set(p(25, 25), Tile::Wheat);
    let o = w.execute(&at(Skill::Farm, 25, 25), FULL);
    assert_eq!((o.success, o.duration_ticks), (true, 20));
    assert_eq!(o.items_gained.count("wheat"), 1);
    assert_eq!(w.map.get(p(25, 25)), Tile::Grass);
    assert!(w.execute(&at(Skill::Farm, 25, 25), FULL).blocked);
}

#[test]
fn a_failed_roll_spends_the_time_but_gains_nothing() {
    let mut w = flat_with(Rules {
        base_success: 0.0,
        ..exact()
    });
    w.map.set(p(25, 24), Tile::Tree { chops_left: 3 });
    let o = w.execute(&at(Skill::Chop, 25, 24), FULL);
    assert!(!o.success && !o.blocked);
    assert_eq!((o.progress, o.duration_ticks), (0.5, 30));
    assert!(o.items_gained.items.is_empty());
    assert_eq!(w.map.get(p(25, 24)), Tile::Tree { chops_left: 3 });
}

#[test]
fn unreachable_resources_are_blocked() {
    let mut w = flat();
    w.map.set(p(40, 40), Tile::Wheat);
    for (x, z) in [
        (39, 39),
        (40, 39),
        (41, 39),
        (39, 40),
        (41, 40),
        (39, 41),
        (40, 41),
        (41, 41),
    ] {
        w.map.set(p(x, z), Tile::Water);
    }
    let o = w.execute(&at(Skill::Farm, 40, 40), FULL);
    assert!(o.blocked && !o.success);
}

// ---------- execute: combat, food, crafting, shelter, rest ----------

#[test]
fn pigs_take_two_swings_and_drop_porkchops() {
    let mut w = flat();
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(25, 24), t));
    w.mobs[0].next_move = u64::MAX;
    let o = w.execute(&at(Skill::Attack, 25, 24), FULL);
    assert!(o.success);
    assert_eq!(o.duration_ticks, 20);
    let chops = o.items_gained.count("porkchop");
    assert!((1..=2).contains(&chops), "{chops}");
    assert!(w.mobs.is_empty());
}

#[test]
fn zombies_take_three_swings() {
    let mut w = flat();
    w.tick = 1200;
    zombie(&mut w, 25, 24);
    let o = w.execute(&at(Skill::Attack, 25, 24), FULL);
    assert!(o.success);
    assert_eq!(o.duration_ticks, 30);
    assert!(w.mobs.is_empty());
    assert!(o.items_gained.items.is_empty());
    assert!(close(o.damage, 0.2), "hit at ticks 1 and 21: {}", o.damage);
}

#[test]
fn attacks_give_up_after_max_swings() {
    let mut w = flat_with(Rules {
        base_success: 0.0,
        ..exact()
    });
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(25, 24), t));
    w.mobs[0].next_move = u64::MAX;
    let o = w.execute(&at(Skill::Attack, 25, 24), FULL);
    assert!(!o.success && !o.blocked);
    assert_eq!((o.progress, o.duration_ticks), (0.5, 40));
    assert_eq!(w.mobs.len(), 1);
}

#[test]
fn attacks_follow_a_moving_target() {
    let mut w = flat();
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(27, 24), t));
    w.mobs[0].next_move = u64::MAX;
    let o = w.execute(&at(Skill::Attack, 27, 24), FULL);
    assert!(o.success);
    assert_eq!(o.duration_ticks, 2 * 2 + 20);
}

#[test]
fn attacks_end_if_the_target_vanishes() {
    let mut w = flat();
    w.tick = 2390;
    zombie(&mut w, 25, 24);
    w.mobs[0].next_hit = u64::MAX;
    let o = w.execute(&at(Skill::Attack, 25, 24), FULL);
    assert!(
        !o.success && !o.blocked,
        "the zombie burned at dawn mid-fight"
    );
    assert!(w.mobs.is_empty());
}

#[test]
fn dying_mid_action_ends_it() {
    for c in [
        at(Skill::Attack, 25, 24),
        at(Skill::Chop, 23, 24),
        at(Skill::MoveTo, 20, 24),
        act(Skill::Shelter, None),
    ] {
        let mut w = flat();
        w.tick = 1200;
        w.body.health = 0.05;
        w.map.set(p(23, 24), Tile::Tree { chops_left: 3 });
        zombie(&mut w, 25, 24);
        let o = w.execute(&c, FULL);
        assert!(!o.success, "{:?}", c.skill);
        assert!(!w.alive());
        assert!(close(o.damage, 0.05));
        assert_eq!(w.map.get(p(24, 24)), Tile::Grass, "no shelter for the dead");
    }
}

#[test]
fn eating_restores_food_and_caps_at_one() {
    let mut w = flat();
    give(&mut w, "porkchop", 1);
    give(&mut w, "bread", 2);
    w.body.food = 0.4;
    let o = w.execute(&item(Skill::Eat, "porkchop"), FULL);
    assert_eq!((o.success, o.duration_ticks), (true, 30));
    assert!(close(w.body.food, 0.4 - 30.0 / 2400.0 + 0.3));
    assert_eq!(w.body.inv.count("porkchop"), 0);
    w.execute(&item(Skill::Eat, "bread"), FULL);
    w.execute(&item(Skill::Eat, "bread"), FULL);
    assert_eq!(w.body.food, 1.0);
}

#[test]
fn crafting_takes_ten_ticks_and_counts_recipes_as_new() {
    let mut w = flat();
    give(&mut w, "oak_log", 2);
    let o = w.execute(&item(Skill::Craft, "oak_planks"), FULL);
    assert_eq!((o.success, o.duration_ticks), (true, 10));
    assert_eq!(o.items_gained.count("oak_planks"), 4);
    assert_eq!(
        o.new_things,
        vec![
            ThingId("recipe:oak_planks".into()),
            ThingId("item:oak_planks".into())
        ]
    );
    let o = w.execute(&item(Skill::Craft, "oak_planks"), FULL);
    assert!(o.new_things.is_empty());
    assert_eq!(o.items_gained.count("oak_planks"), 4);
}

#[test]
fn shelters_take_60_ticks_or_20_with_four_blocks() {
    let mut w = flat();
    let o = w.execute(&act(Skill::Shelter, None), FULL);
    assert_eq!((o.success, o.duration_ticks), (true, 60));
    assert!(w.sheltered());
    assert_eq!(w.map.get(p(24, 24)), Tile::Shelter);
    assert_eq!(o.new_things, vec![ThingId("block:shelter".into())]);
    let mut w = flat();
    give(&mut w, "oak_planks", 5);
    give(&mut w, "cobblestone", 4);
    let o = w.execute(&act(Skill::Shelter, None), FULL);
    assert_eq!(o.duration_ticks, 20);
    assert_eq!(w.body.inv.count("cobblestone"), 0, "cobblestone first");
    assert_eq!(w.body.inv.count("oak_planks"), 5);
    let mut w = flat();
    give(&mut w, "oak_planks", 4);
    give(&mut w, "cobblestone", 3);
    w.execute(&act(Skill::Shelter, None), FULL);
    assert_eq!(w.body.inv.count("oak_planks"), 0);
    assert_eq!(w.body.inv.count("cobblestone"), 3);
}

#[test]
fn rest_needs_shelter_and_ends_at_dawn() {
    let mut w = flat();
    assert!(w.execute(&act(Skill::Rest, None), FULL).blocked);
    w.map.set(p(24, 24), Tile::Shelter);
    let o = w.execute(&act(Skill::Rest, None), FULL);
    assert_eq!((o.success, o.duration_ticks), (true, 300));
    assert!(w.perceive().resting);
    w.tick = 1300;
    assert_eq!(w.execute(&act(Skill::Rest, None), FULL).duration_ticks, 300);
    w.tick = 2300;
    assert_eq!(w.execute(&act(Skill::Rest, None), FULL).duration_ticks, 100);
    w.execute(&at(Skill::MoveTo, 25, 24), FULL);
    assert!(!w.perceive().resting, "any action ends rest");
}

#[test]
fn success_chance_is_base_times_focus_times_threat() {
    let mut w = flat_with(Rules::default());
    assert!(close(w.success_chance(1.0), 0.95));
    assert!(close(w.success_chance(0.5), 0.475));
    assert!(close(w.success_chance(2.0), 0.95), "focus clamps to 1");
    assert_eq!(w.success_chance(-1.0), 0.0);
    zombie(&mut w, 26, 26);
    assert!(close(w.success_chance(1.0), 0.95 * 0.75));
    w.mobs[0].pos = p(27, 24);
    assert!(
        close(w.success_chance(1.0), 0.95),
        "3 tiles is not a threat"
    );
    w.mobs[0].pos = p(25, 24);
    w.map.set(p(24, 24), Tile::Shelter);
    assert!(close(w.success_chance(1.0), 0.95), "no threat in a shelter");
    w.map.set(p(24, 24), Tile::Grass);
    w.mobs[0].kind = MobKind::Pig;
    assert!(close(w.success_chance(1.0), 0.95), "pigs are no threat");
}

#[test]
fn outcomes_report_damage_and_only_gains() {
    let mut w = flat();
    give(&mut w, "oak_log", 1);
    w.tick = 1200;
    zombie(&mut w, 25, 24);
    let o = w.execute(&item(Skill::Craft, "oak_planks"), FULL);
    assert!(close(o.damage, 0.1));
    assert_eq!(
        o.items_gained,
        Inventory {
            items: vec![("oak_planks".into(), 4)]
        },
        "the log spent is not a gain"
    );
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

// ---------- candidates ----------

fn skills(c: &[Candidate]) -> Vec<Skill> {
    c.iter().map(|c| c.skill).collect()
}

fn find(c: &[Candidate], s: Skill) -> &Candidate {
    c.iter().find(|c| c.skill == s).expect("candidate offered")
}

#[test]
fn open_plain_offers_shelter_and_four_ways_to_explore() {
    let mut w = flat();
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    assert_eq!(
        skills(&c),
        [
            Skill::Shelter,
            Skill::Explore,
            Skill::Explore,
            Skill::Explore,
            Skill::Explore
        ]
    );
    for (i, x) in c.iter().enumerate() {
        assert_eq!(x.id as usize, i);
        assert!(!x.commanded);
    }
    let s = find(&c, Skill::Shelter);
    assert_eq!(
        (s.est_cost, s.est_risk, s.task_type),
        (0.25, 0.0, TaskType::Building)
    );
    let e = &c[1];
    assert_eq!(
        e.target,
        Some(Target::Pos(p(24, 16))),
        "north first on a tie"
    );
    assert_eq!((e.est_cost, e.task_type), (0.25, TaskType::Travel));
    let targets: Vec<_> = c[1..].iter().map(|x| x.target.clone().unwrap()).collect();
    assert_eq!(
        targets,
        [p(24, 16), p(32, 24), p(24, 32), p(16, 24)].map(Target::Pos)
    );
}

#[test]
fn explore_prefers_least_visited_regions() {
    let mut w = flat();
    w.execute(&at(Skill::MoveTo, 24, 17), FULL);
    w.execute(&at(Skill::MoveTo, 24, 24), FULL);
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    let last = c.last().unwrap();
    assert_eq!(last.target, Some(Target::Pos(p(24, 16))));
}

#[test]
fn explore_targets_snap_to_reachable_ground_and_clamp_to_the_map() {
    let mut w = SimWorld::with_map(
        exact(),
        1,
        Map::filled(48, Tile::Grass, Biome::Plains),
        p(2, 2),
    );
    w.map.set(p(10, 2), Tile::Water);
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    let t: Vec<_> = c
        .iter()
        .filter(|x| x.skill == Skill::Explore)
        .map(|x| x.target.clone().unwrap())
        .collect();
    assert!(t.contains(&Target::Pos(p(2, 0))), "clamped north");
    assert!(t.contains(&Target::Pos(p(0, 2))), "clamped west");
    assert!(t.contains(&Target::Pos(p(9, 1))), "snapped off the water");
}

#[test]
fn food_rest_and_shelter_candidates() {
    let mut w = flat();
    give(&mut w, "porkchop", 1);
    let o = w.perceive();
    assert!(
        !skills(&w.candidate_actions(&o)).contains(&Skill::Eat),
        "full"
    );
    w.body.food = 0.9;
    give(&mut w, "bread", 1);
    let c = w.candidate_actions(&o);
    let e = find(&c, Skill::Eat);
    assert_eq!(e.target, Some(Target::Item("bread".into())), "bread first");
    assert_eq!((e.est_cost, e.task_type), (0.05, TaskType::Farming));
    give(&mut w, "oak_planks", 4);
    assert_eq!(find(&w.candidate_actions(&o), Skill::Shelter).est_cost, 0.1);
    w.map.set(p(27, 24), Tile::Shelter);
    let m = w.candidate_actions(&o);
    let m = find(&m, Skill::MoveTo);
    assert_eq!(m.target, Some(Target::Pos(p(27, 24))));
    assert!(close(m.est_cost, 3.0 / 32.0));
    w.execute(&at(Skill::MoveTo, 27, 24), FULL);
    let c = w.candidate_actions(&o);
    assert!(!skills(&c).contains(&Skill::Shelter) && !skills(&c).contains(&Skill::MoveTo));
    assert_eq!(find(&c, Skill::Rest).est_cost, 0.1);
}

#[test]
fn threats_offer_flee_away_and_attack() {
    let mut w = flat();
    zombie(&mut w, 27, 24);
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    assert_eq!(&skills(&c)[..2], [Skill::Flee, Skill::Attack]);
    assert_eq!(
        c[0].target,
        Some(Target::Pos(p(16, 24))),
        "away, to the west"
    );
    assert_eq!((c[0].est_risk, c[0].task_type), (0.2, TaskType::Travel));
    assert!(close(c[0].est_cost, 0.1 + 8.0 / 32.0));
    assert_eq!(c[1].task_type, TaskType::Combat);
    assert!(close(c[1].est_cost, 0.2 + 2.0 / 32.0));
    w.mobs[0].pos = p(24, 24);
    let c = w.candidate_actions(&o);
    assert_eq!(
        c[0].target,
        Some(Target::Pos(p(32, 24))),
        "on top: run east"
    );
    w.mobs[0].pos = p(31, 24);
    assert_ne!(
        w.candidate_actions(&o)[0].skill,
        Skill::Flee,
        "7 tiles is not a threat"
    );
}

#[test]
fn resources_need_the_tool_to_be_offered() {
    let mut w = flat();
    w.map.set(p(26, 24), Tile::Tree { chops_left: 1 });
    w.map.set(p(22, 24), Tile::Stone);
    w.map.set(p(24, 28), Tile::IronOre);
    w.map.set(p(24, 20), Tile::Wheat);
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(20, 20), t));
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    let s = skills(&c);
    assert!(s.contains(&Skill::Chop) && s.contains(&Skill::Farm));
    assert!(!s.contains(&Skill::Mine));
    let pig = c
        .iter()
        .find(|x| x.skill == Skill::Attack)
        .expect("pig offered");
    assert_eq!(pig.task_type, TaskType::Farming);
    assert_eq!(pig.target, Some(Target::Pos(p(20, 20))));
    assert!(close(pig.est_cost, 0.15 + 6.0 / 32.0));
    let chop = find(&c, Skill::Chop);
    assert_eq!(
        (chop.task_type, chop.est_cost),
        (TaskType::Mining, 0.1 + 1.0 / 32.0)
    );
    assert_eq!(find(&c, Skill::Farm).task_type, TaskType::Farming);
    give(&mut w, "wooden_pickaxe", 1);
    let c = w.candidate_actions(&o);
    let mines: Vec<_> = c.iter().filter(|x| x.skill == Skill::Mine).collect();
    assert_eq!(mines.len(), 1);
    assert_eq!(mines[0].target, Some(Target::Pos(p(22, 24))));
    give(&mut w, "stone_pickaxe", 1);
    let c = w.candidate_actions(&o);
    assert_eq!(c.iter().filter(|x| x.skill == Skill::Mine).count(), 2);
}

#[test]
fn nearest_resource_is_by_steps_not_straight_line() {
    let mut w = flat();
    w.map.set(p(26, 24), Tile::Wheat);
    for z in 20..=28 {
        w.map.set(p(25, z), Tile::Water);
    }
    w.map.set(p(20, 24), Tile::Wheat);
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    assert_eq!(find(&c, Skill::Farm).target, Some(Target::Pos(p(20, 24))));
}

#[test]
fn crafting_skips_owned_stations_and_worse_pickaxes() {
    let mut w = flat();
    give(&mut w, "oak_planks", 10);
    give(&mut w, "stick", 4);
    let o = w.perceive();
    let crafts = |w: &mut SimWorld| -> Vec<Target> {
        w.candidate_actions(&o)
            .into_iter()
            .filter(|x| x.skill == Skill::Craft)
            .map(|x| x.target.unwrap())
            .collect()
    };
    let names = |v: &[&str]| {
        v.iter()
            .map(|s| Target::Item(s.to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(crafts(&mut w), names(&["stick", "crafting_table"]));
    give(&mut w, "crafting_table", 1);
    assert_eq!(crafts(&mut w), names(&["stick", "wooden_pickaxe"]));
    give(&mut w, "wooden_pickaxe", 1);
    give(&mut w, "cobblestone", 8);
    give(&mut w, "furnace", 1);
    assert_eq!(crafts(&mut w), names(&["stick", "stone_pickaxe"]));
    give(&mut w, "stone_pickaxe", 1);
    give(&mut w, "iron_ingot", 3);
    assert_eq!(crafts(&mut w), names(&["stick", "iron_pickaxe"]));
    give(&mut w, "iron_pickaxe", 1);
    assert_eq!(crafts(&mut w), names(&["stick"]));
    let c = w.candidate_actions(&o);
    let s = c.iter().find(|x| x.skill == Skill::Craft).unwrap();
    assert_eq!((s.est_cost, s.task_type), (0.05, TaskType::Crafting));
}

/// The richest situation the sim can produce still fits in K = 20 (spec §6).
#[test]
fn the_fullest_candidate_list_fits_in_twenty() {
    let mut w = flat();
    for (x, z) in [(26, 24), (22, 24), (24, 26), (24, 22)] {
        w.map.set(p(x, z), Tile::Stone);
    }
    w.map.set(p(26, 26), Tile::IronOre);
    w.map.set(p(22, 22), Tile::Wheat);
    w.map.set(p(22, 26), Tile::Tree { chops_left: 1 });
    zombie(&mut w, 26, 22);
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(20, 20), t));
    w.body.food = 0.5;
    for (i, n) in [
        ("oak_log", 2),
        ("oak_planks", 8),
        ("stick", 4),
        ("cobblestone", 11),
        ("iron_ingot", 3),
        ("iron_ore", 1),
        ("wheat", 3),
        ("bread", 1),
        ("crafting_table", 1),
    ] {
        give(&mut w, i, n);
    }
    w.map.set(p(20, 24), Tile::Shelter);
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    assert_eq!(c.len(), 19);
    assert!(c.len() <= sim::world::MAX_CANDIDATES);
    assert_eq!(c.last().unwrap().id, 18);
}

#[test]
fn risk_counts_night_hostiles_and_lava() {
    let mut w = flat();
    let o = w.perceive();
    assert_eq!(find(&w.candidate_actions(&o), Skill::Shelter).est_risk, 0.0);
    w.tick = 1200;
    assert_eq!(find(&w.candidate_actions(&o), Skill::Shelter).est_risk, 0.3);
    zombie(&mut w, 32, 24);
    zombie(&mut w, 33, 24);
    let c = w.candidate_actions(&o);
    assert!(close(find(&c, Skill::Shelter).est_risk, 0.3 + 0.15));
    w.map.set(p(25, 25), Tile::Lava);
    let c = w.candidate_actions(&o);
    assert!(close(find(&c, Skill::Shelter).est_risk, 0.75));
    for x in [30, 31, 29] {
        zombie(&mut w, x, 30);
    }
    let c = w.candidate_actions(&o);
    assert_eq!(find(&c, Skill::Shelter).est_risk, 1.0, "capped");
    w.map.set(p(24, 24), Tile::Shelter);
    w.mobs.clear();
    w.map.set(p(25, 25), Tile::Grass);
    let c = w.candidate_actions(&o);
    let e = c.iter().find(|x| x.skill == Skill::Explore).unwrap();
    assert_eq!(e.est_risk, 0.0, "night risk only counts when exposed");
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
