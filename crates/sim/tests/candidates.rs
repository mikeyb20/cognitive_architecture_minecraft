//! What each skill handler offers (spec §6, §21).

mod common;

use common::*;
use host_api::{Candidate, Host, Skill, Target, TaskType};
use sim::SimWorld;
use sim::map::{Biome, Map, Tile};
use sim::mob::{Mob, MobKind};

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
        Some(Target::Pos(p(24, 16))),
        "on top: four tiles tie at 8; the first in scan order (north) wins"
    );
    w.mobs[0].pos = p(31, 24);
    assert_ne!(
        w.candidate_actions(&o)[0].skill,
        Skill::Flee,
        "7 tiles is not a threat"
    );
}

#[test]
fn flee_finds_a_way_out_when_the_hostile_is_diagonal_or_the_way_is_walled() {
    // Debt D5: a diagonal hostile and a wall behind used to leave no flee.
    let mut w = flat();
    for z in 0..48 {
        w.map.set(p(22, z), Tile::Stone);
    }
    zombie(&mut w, 26, 26);
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    let flee = find(&c, Skill::Flee);
    // The old rule aimed at (16, 16), behind the wall, and offered nothing.
    // Now: the furthest open tile within 8 steps, √104 ≈ 10.2 tiles away.
    assert_eq!(flee.target, Some(Target::Pos(p(24, 16))));
    assert!(close(flee.est_cost, 0.1 + 8.0 / 32.0));
}

#[test]
fn flee_weighs_every_hostile_in_chase_range() {
    let mut w = flat();
    zombie(&mut w, 20, 24);
    zombie(&mut w, 28, 24);
    zombie(&mut w, 24, 41);
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    assert_eq!(
        find(&c, Skill::Flee).target,
        Some(Target::Pos(p(24, 16))),
        "between two, run north; the one 17 tiles south is out of chase range"
    );
}

#[test]
fn flee_ignores_passive_mobs() {
    let mut w = flat();
    zombie(&mut w, 27, 24);
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(19, 24), t));
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    assert_eq!(
        find(&c, Skill::Flee).target,
        Some(Target::Pos(p(16, 24))),
        "a pig in the way is not a threat"
    );
}

#[test]
fn flee_takes_the_shorter_way_when_two_spots_are_as_safe() {
    // Walled in with the zombie on top: a corridor runs 5 tiles north to
    // (24, 19), and another east then north to (27, 20). Both are 5 tiles
    // from the zombie; the first is 5 steps away, the second 7.
    let mut w = flat();
    for q in sim::geom::square(p(24, 24), 9) {
        w.map.set(q, Tile::Stone);
    }
    let open = [(24, 24), (24, 23), (24, 22), (24, 21), (24, 20), (24, 19)];
    let bend = [(25, 24), (26, 24), (27, 24), (27, 23), (27, 22), (27, 21)];
    for (x, z) in open.into_iter().chain(bend).chain([(27, 20)]) {
        w.map.set(p(x, z), Tile::Grass);
    }
    zombie(&mut w, 24, 24);
    let o = w.perceive();
    let flee = find(&w.candidate_actions(&o), Skill::Flee).clone();
    assert_eq!(flee.target, Some(Target::Pos(p(24, 19))));
    assert!(close(flee.est_cost, 0.1 + 5.0 / 32.0));
}

#[test]
fn no_flee_when_nowhere_is_further_away() {
    let mut w = flat();
    for q in sim::geom::ring(p(24, 24), 1) {
        w.map.set(q, Tile::Stone);
    }
    zombie(&mut w, 25, 25);
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    assert!(!skills(&c).contains(&Skill::Flee), "boxed in with it");
    assert!(skills(&c).contains(&Skill::Attack));
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
        ("raw_iron", 1),
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

// ---------- edges found by mutation testing ----------

#[test]
fn nearest_resource_wins_even_when_scanned_later() {
    let mut w = flat();
    w.map.set(p(24, 18), Tile::Wheat);
    w.map.set(p(24, 27), Tile::Wheat);
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    assert_eq!(find(&c, Skill::Farm).target, Some(Target::Pos(p(24, 27))));
    w.map.set(p(24, 21), Tile::Wheat);
    let c = w.candidate_actions(&o);
    assert_eq!(
        find(&c, Skill::Farm).target,
        Some(Target::Pos(p(24, 21))),
        "a tie goes to the first in scan order (north)"
    );
}

#[test]
fn explore_clamps_at_the_far_edges_too() {
    let mut w = SimWorld::with_map(
        exact(),
        1,
        Map::filled(48, Tile::Grass, Biome::Plains),
        p(45, 45),
    );
    let o = w.perceive();
    let t: Vec<_> = w
        .candidate_actions(&o)
        .into_iter()
        .filter(|x| x.skill == Skill::Explore)
        .map(|x| x.target.unwrap())
        .collect();
    assert!(t.contains(&Target::Pos(p(47, 45))), "{t:?}");
    assert!(t.contains(&Target::Pos(p(45, 47))), "{t:?}");
}

#[test]
fn eat_offers_the_food_actually_held() {
    let mut w = flat();
    w.body.food = 0.5;
    give(&mut w, "porkchop", 1);
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    assert_eq!(
        find(&c, Skill::Eat).target,
        Some(Target::Item("porkchop".into()))
    );
}

#[test]
fn zombies_are_not_offered_as_food() {
    let mut w = flat();
    w.tick = 1200;
    zombie(&mut w, 31, 24);
    let t = w.tick;
    w.mobs.push(Mob::new(MobKind::Pig, p(24, 33), t));
    let o = w.perceive();
    let c = w.candidate_actions(&o);
    assert!(
        !c.iter()
            .any(|x| x.skill == Skill::Attack && x.task_type == TaskType::Farming),
        "the zombie is no food and the pig is out of view"
    );
}

#[test]
fn lava_on_any_side_adds_risk() {
    for (x, z) in [(23, 23), (25, 25), (23, 25), (25, 23)] {
        let mut w = flat();
        w.map.set(p(x, z), Tile::Lava);
        let o = w.perceive();
        let c = w.candidate_actions(&o);
        assert!(
            close(find(&c, Skill::Shelter).est_risk, 0.3),
            "lava at {x},{z}"
        );
    }
}
