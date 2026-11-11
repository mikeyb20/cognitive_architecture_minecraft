//! Arm A's behaviour tree, one test per branch (spec §24).

use baselines::Policy;
use baselines::arm_a::{ArmA, ArmAConfig};
use host_api::{
    AgentId, Candidate, Inventory, Observation, Pos, Seen, SeenKind, Skill, Target, TaskType,
    ThingId,
};

fn p(x: i32, z: i32) -> Pos {
    Pos { x, y: 0, z }
}

/// Daytime, healthy, fed, with a tree visible at (1, 1).
fn obs() -> Observation {
    let mut o = bare();
    block(&mut o, "oak_log", 1, 1);
    o
}

fn bare() -> Observation {
    Observation {
        agent: AgentId(0),
        tick: 0,
        day_phase: 0.1,
        health: 1.0,
        food: 1.0,
        position: p(0, 0),
        inventory: Inventory::default(),
        visible: vec![],
        chat: vec![],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

fn hostile(o: &mut Observation, d: f32) {
    o.visible.push(Seen {
        thing: ThingId("entity:zombie".into()),
        kind: SeenKind::Entity { hostile: true },
        pos: p(d as i32, 0),
        distance: d,
    });
}

fn block(o: &mut Observation, name: &str, x: i32, z: i32) {
    o.visible.push(Seen {
        thing: ThingId(format!("block:{name}")),
        kind: SeenKind::Block,
        pos: p(x, z),
        distance: 1.0,
    });
}

fn c(skill: Skill, task: TaskType, target: Option<Target>, cost: f32) -> Candidate {
    Candidate {
        id: 0,
        skill,
        target,
        task_type: task,
        est_cost: cost,
        est_risk: 0.0,
        commanded: false,
    }
}

/// One of every kind of candidate the sim offers.
fn all() -> Vec<Candidate> {
    let pos = |x, z| Some(Target::Pos(p(x, z)));
    let it = |s: &str| Some(Target::Item(s.into()));
    vec![
        c(Skill::Explore, TaskType::Travel, pos(8, 0), 0.25), // 0
        c(Skill::Flee, TaskType::Travel, pos(-8, 0), 0.3),    // 1
        c(Skill::Attack, TaskType::Combat, pos(2, 0), 0.3),   // 2
        c(Skill::Eat, TaskType::Farming, it("bread"), 0.05),  // 3
        c(Skill::Shelter, TaskType::Building, None, 0.25),    // 4
        c(Skill::Rest, TaskType::Building, None, 0.1),        // 5
        c(Skill::Craft, TaskType::Crafting, it("bread"), 0.05), // 6
        c(Skill::Attack, TaskType::Farming, pos(5, 5), 0.2),  // 7
        c(Skill::Farm, TaskType::Farming, pos(3, 3), 0.2),    // 8
        c(Skill::Chop, TaskType::Mining, pos(1, 1), 0.15),    // 9
        c(Skill::Mine, TaskType::Mining, pos(2, 2), 0.12),    // 10
        c(Skill::Mine, TaskType::Mining, pos(4, 4), 0.11),    // 11
        c(Skill::Craft, TaskType::Crafting, it("oak_planks"), 0.05), // 12
    ]
}

fn pick(o: &Observation, cands: &[Candidate]) -> Option<usize> {
    ArmA::default().choose(o, cands)
}

fn only(cands: &[Candidate], keep: &[usize]) -> Vec<Candidate> {
    keep.iter().map(|&i| cands[i].clone()).collect()
}

#[test]
fn defaults_are_pinned() {
    let d = ArmAConfig::default();
    assert_eq!(d.goal, "iron_pickaxe");
    assert_eq!((d.dusk, d.threat_range, d.fight_health), (0.42, 2.5, 0.5));
    assert_eq!((d.crowd_range, d.eat_below, d.hunt_below), (4.0, 0.6, 0.7));
    assert_eq!(ArmA::new(d.clone()).config, d);
}

#[test]
fn fights_one_close_hostile_when_healthy() {
    let mut o = obs();
    hostile(&mut o, 2.5);
    assert_eq!(pick(&o, &all()), Some(2));
    o.health = 0.51;
    assert_eq!(pick(&o, &all()), Some(2));
    let a = all();
    assert_eq!(
        pick(&o, &only(&a, &[7, 2])),
        Some(1),
        "the zombie, not the pig"
    );
}

#[test]
fn flees_when_hurt_or_crowded() {
    let mut o = obs();
    hostile(&mut o, 2.0);
    o.health = 0.5;
    assert_eq!(pick(&o, &all()), Some(1));
    let mut o = obs();
    hostile(&mut o, 2.0);
    hostile(&mut o, 4.0);
    assert_eq!(pick(&o, &all()), Some(1), "two within 4 tiles");
    let mut o = obs();
    hostile(&mut o, 2.0);
    hostile(&mut o, 4.1);
    assert_eq!(pick(&o, &all()), Some(2), "the second is too far to count");
}

#[test]
fn ignores_far_hostiles_and_hostiles_outside_the_shelter() {
    let mut o = obs();
    hostile(&mut o, 2.6);
    assert_eq!(pick(&o, &all()), Some(9), "carries on with the plan");
    let mut o = obs();
    hostile(&mut o, 1.0);
    o.sheltered = true;
    assert_eq!(pick(&o, &all()), Some(9));
}

#[test]
fn falls_through_when_the_threat_option_is_missing() {
    let mut o = obs();
    hostile(&mut o, 1.0);
    o.health = 0.3;
    let cands = only(&all(), &[0, 2, 9]);
    assert_eq!(
        pick(&o, &cands),
        Some(2),
        "no flee offered: plan step (chop)"
    );
}

#[test]
fn eats_below_threshold_when_food_is_offered() {
    let mut o = obs();
    o.food = 0.59;
    assert_eq!(pick(&o, &all()), Some(3));
    o.food = 0.6;
    o.inventory.items.push(("bread".into(), 1));
    assert_eq!(pick(&o, &all()), Some(9));
    o.food = 0.3;
    assert_eq!(pick(&o, &only(&all(), &[9])), Some(0), "no eat offered");
}

#[test]
fn shelters_at_dusk_and_rests_inside() {
    let mut o = obs();
    o.day_phase = 0.42;
    assert_eq!(pick(&o, &all()), Some(4));
    o.sheltered = true;
    assert_eq!(pick(&o, &all()), Some(5));
    o.day_phase = 0.41;
    assert_eq!(pick(&o, &all()), Some(9), "not dusk yet");
    o.day_phase = 0.9;
    assert_eq!(
        pick(&o, &only(&all(), &[0, 9])),
        Some(1),
        "no rest offered: plan step"
    );
}

#[test]
fn hunts_when_hungry_bread_then_pig_then_wheat_then_explore() {
    let mut o = obs();
    o.food = 0.69;
    let a = all();
    assert_eq!(pick(&o, &a), Some(6), "craft bread");
    assert_eq!(pick(&o, &only(&a, &[0, 7, 8, 9])), Some(1), "pig");
    assert_eq!(
        pick(&o, &only(&a, &[2, 7])),
        Some(1),
        "the pig, not a zombie"
    );
    assert_eq!(pick(&o, &only(&a, &[0, 8, 9])), Some(1), "wheat");
    assert_eq!(pick(&o, &only(&a, &[9, 0])), Some(1), "explore for food");
    o.food = 0.7;
    assert_eq!(pick(&o, &a), Some(9), "not hungry: plan");
    o.food = 0.65;
    o.inventory.items.push(("porkchop".into(), 1));
    assert_eq!(pick(&o, &a), Some(9), "food in hand: no hunt");
}

#[test]
fn hunting_without_options_falls_back_to_the_plan() {
    let mut o = obs();
    o.food = 0.65;
    assert_eq!(pick(&o, &only(&all(), &[9])), Some(0));
}

#[test]
fn plan_step_matches_the_planner() {
    let mut o = obs();
    let a = all();
    assert_eq!(pick(&o, &a), Some(9), "empty inventory: chop");
    assert_eq!(
        pick(&bare(), &a),
        Some(11),
        "no tree in view: cheapest resource"
    );
    o.inventory.items.push(("oak_log".into(), 1));
    assert_eq!(pick(&o, &a), Some(12), "then planks");
    let mut o = obs();
    for (i, n) in [("crafting_table", 1), ("wooden_pickaxe", 1), ("stick", 2)] {
        o.inventory.items.push((i.into(), n));
    }
    block(&mut o, "iron_ore", 4, 4);
    block(&mut o, "stone", 2, 2);
    assert_eq!(pick(&o, &a), Some(10), "mine the stone, not the iron");
    let mut o = obs();
    for (i, n) in [
        ("stone_pickaxe", 1),
        ("furnace", 1),
        ("crafting_table", 1),
        ("stick", 2),
    ] {
        o.inventory.items.push((i.into(), n));
    }
    block(&mut o, "stone", 2, 2);
    block(&mut o, "iron_ore", 4, 4);
    assert_eq!(pick(&o, &a), Some(11), "mine the iron");
}

#[test]
fn wheat_goal_farms_the_visible_wheat() {
    let mut arm = ArmA::new(ArmAConfig {
        goal: "bread".into(),
        ..ArmAConfig::default()
    });
    let mut o = obs();
    block(&mut o, "wheat", 3, 3);
    assert_eq!(arm.choose(&o, &all()), Some(8));
}

#[test]
fn without_a_plan_match_takes_the_cheapest_resource_then_explores() {
    let mut o = obs();
    for (i, n) in [("crafting_table", 1), ("wooden_pickaxe", 1)] {
        o.inventory.items.push((i.into(), n));
    }
    let a = all();
    assert_eq!(
        pick(&o, &a),
        Some(11),
        "stone not visible: cheapest resource"
    );
    assert_eq!(pick(&o, &only(&a, &[4, 0])), Some(1), "explore");
    assert_eq!(pick(&o, &only(&a, &[4, 5])), Some(0), "anything");
    assert_eq!(pick(&o, &[]), None);
    let mut done = obs();
    done.inventory.items.push(("iron_pickaxe".into(), 1));
    assert_eq!(
        pick(&done, &only(&a, &[8, 9])),
        Some(1),
        "goal held: cheapest resource"
    );
}

#[test]
fn follows_the_cheapest_commanded_step_after_danger_and_before_dusk() {
    let mut a = all();
    for i in [10, 11] {
        a[i].commanded = true;
    }
    let mut o = obs();
    o.day_phase = 0.42;
    assert_eq!(pick(&o, &a), Some(11), "the cheaper of two, before dusk");
    a[11].est_cost = 0.13;
    assert_eq!(pick(&o, &a), Some(10));
    o.food = 0.59;
    assert_eq!(pick(&o, &a), Some(3), "eating first");
    o.food = 1.0;
    hostile(&mut o, 2.0);
    assert_eq!(pick(&o, &a), Some(2), "a close hostile first");
}
