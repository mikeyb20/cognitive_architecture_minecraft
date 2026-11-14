//! Running each skill (spec §21; one handler per skill group under
//! `sim::skills`).

mod common;

use common::*;
use host_api::{Host, Intensity, Inventory, Skill, ThingId};
use sim::map::{Biome, Map, Tile};
use sim::mob::{Mob, MobKind};
use sim::{Death, Rules, SimWorld};

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
    // Exactly at the walking floor the agent walks.
    let o = w.execute(&at(Skill::MoveTo, 26, 24), slow);
    assert!(o.success && w.body.pos == p(26, 24));
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
    // Spec §16: below the walking floor the agent stays put.
    let low = Intensity {
        effort: 0.049,
        ..FULL
    };
    let mut w = flat();
    for skill in [Skill::MoveTo, Skill::Flee] {
        let o = w.execute(&at(skill, 27, 24), low);
        assert!(o.blocked && !o.success, "{skill:?}");
        assert_eq!(o.duration_ticks, 5, "a blocked action's ticks");
        assert_eq!(w.body.pos, p(24, 24));
    }
    // Where it stands it still acts: eating needs no walking.
    w.body.food = 0.5;
    give(&mut w, "bread", 1);
    let o = w.execute(&item(Skill::Eat, "bread"), low);
    assert!(o.success && w.body.food > 0.5);
    // Already there, a move succeeds without walking.
    assert!(w.execute(&at(Skill::MoveTo, 24, 24), low).success);
}

#[test]
fn zero_effort_does_nothing_at_all() {
    // g(0) = 0 (01 §1.4): not even an action in place.
    let mut w = flat();
    w.body.food = 0.5;
    give(&mut w, "bread", 1);
    let none = Intensity {
        effort: 0.0,
        ..FULL
    };
    let o = w.execute(&item(Skill::Eat, "bread"), none);
    assert!(o.blocked && !o.success);
    assert_eq!(
        (o.duration_ticks, w.body.food),
        (1, 0.5 - w.rules.food_decay)
    );
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
        // Skills the sim lacks stay blocked even with a place to go.
        at(Skill::Place, 26, 24),
        at(Skill::FollowPlayer, 26, 24),
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
    assert_eq!(o.items_gained.count("raw_iron"), 1);
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
fn shelters_take_6_ticks_to_dig_or_20_with_four_blocks() {
    let mut w = flat();
    let o = w.execute(&act(Skill::Shelter, None), FULL);
    assert_eq!(
        (o.success, o.duration_ticks),
        (true, 6),
        "#62: dig-in 60 → 6"
    );
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
fn digging_in_needs_soil_or_a_pickaxe_on_stone() {
    let on = |tile: Tile| {
        let mut w = flat();
        w.map.set(p(24, 24), tile);
        w
    };
    let offered = |w: &mut SimWorld| {
        let o = w.perceive();
        w.candidate_actions(&o)
            .iter()
            .any(|c| c.skill == Skill::Shelter)
    };
    // Farmland digs like grass.
    let mut w = on(Tile::Wheat);
    assert!(offered(&mut w));
    assert_eq!(
        w.execute(&act(Skill::Shelter, None), FULL).duration_ticks,
        6
    );
    // A cave's stone floor: not by hand, and as long as mining with a pick.
    let mut w = on(Tile::CaveFloor);
    assert!(!offered(&mut w));
    let o = w.execute(&act(Skill::Shelter, None), FULL);
    assert!(o.blocked && !w.sheltered());
    for (pick, ticks) in [
        ("wooden_pickaxe", 40),
        ("stone_pickaxe", 30),
        ("iron_pickaxe", 20),
    ] {
        let mut w = on(Tile::CaveFloor);
        give(&mut w, pick, 1);
        assert!(offered(&mut w), "{pick}");
        let o = w.execute(&act(Skill::Shelter, None), FULL);
        assert_eq!((o.success, o.duration_ticks), (true, ticks), "{pick}");
        assert!(w.sheltered());
    }
    // In a doorway nothing digs, but four blocks still build.
    let mut w = on(Tile::Door);
    assert!(!offered(&mut w));
    give(&mut w, "oak_planks", 4);
    assert!(offered(&mut w));
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

// ---------- edges found by mutation testing ----------

#[test]
fn attacks_hit_the_targeted_animal_only() {
    let mut w = flat();
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(40, 40), t));
    w.mobs.push(Mob::new(MobKind::Pig, p(25, 24), t));
    for m in &mut w.mobs {
        m.next_move = u64::MAX;
    }
    let o = w.execute(&at(Skill::Attack, 25, 24), FULL);
    assert!(o.success);
    assert_eq!(w.mobs.len(), 1);
    assert_eq!((w.mobs[0].pos, w.mobs[0].health), (p(40, 40), 0.5));
}

#[test]
fn unchanged_items_are_not_gains() {
    let mut w = flat();
    give(&mut w, "stick", 2);
    w.map.set(p(25, 24), Tile::Tree { chops_left: 3 });
    let o = w.execute(&at(Skill::Chop, 25, 24), FULL);
    assert_eq!(
        o.items_gained,
        Inventory {
            items: vec![("oak_log".into(), 1)]
        }
    );
}
