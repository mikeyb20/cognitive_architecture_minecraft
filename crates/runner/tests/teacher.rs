//! Build step 13 and 14 gates (spec §27; 01 §4.11, #58, #59): the sim
//! teacher shows the shelter recipe while ρ is low and stops as ρ rises;
//! the shelter test stays available, and a passing test completes the
//! goal however the shelter came about.

use cognitive_core::agent::{Brain, BrainConfig, Goal, ThinkTools, TickLog};
use host_api::{Host, Inventory, Skill};
use runner::brain::{SHELTER, SimTools, run_with, setup, shelter_recipe};

fn inv(items: &[(&str, u32)]) -> Inventory {
    Inventory {
        items: items.iter().map(|(i, n)| (i.to_string(), *n)).collect(),
    }
}

fn skills(inv: &Inventory) -> Vec<(Skill, String)> {
    shelter_recipe(inv)
        .steps
        .into_iter()
        .map(|s| (s.skill, s.thing))
        .collect()
}

#[test]
fn the_shelter_recipe_starts_from_what_is_held() {
    let build = (Skill::Shelter, String::new());
    let craft = (Skill::Craft, "item:oak_planks".to_string());
    let chop = (Skill::Chop, "block:oak_log".to_string());
    assert_eq!(skills(&inv(&[])), [chop, craft.clone(), build.clone()]);
    assert_eq!(
        skills(&inv(&[("oak_log", 1)])),
        [craft.clone(), build.clone()]
    );
    assert_eq!(
        skills(&inv(&[("oak_planks", 3)])).len(),
        3,
        "3 planks is short"
    );
    assert_eq!(
        skills(&inv(&[("oak_planks", 4)])),
        skills(&inv(&[("cobblestone", 4)]))
    );
    assert_eq!(skills(&inv(&[("cobblestone", 4)])), [build]);
    assert_eq!(shelter_recipe(&inv(&[])).goal, SHELTER);
}

#[test]
fn the_sim_teacher_explains_only_the_shelter() {
    let mut t = SimTools {
        shelter: Some(true),
        ..SimTools::default()
    };
    let obs = setup("first_night", 0).world.perceive();
    assert_eq!(t.explain(SHELTER, &obs), Some(true));
    assert_eq!(t.explain("stick", &obs), None);
    assert!(t.show("stick", &obs).is_none());
    assert!(t.show(SHELTER, &obs).is_some());
    assert_eq!(
        SimTools::default().explain(SHELTER, &obs),
        None,
        "not yet seen"
    );
}

fn run(seed: u64, rho_pre: f32) -> Vec<TickLog> {
    let mut b = Brain::new(
        BrainConfig {
            rho_pre,
            ..BrainConfig::default()
        },
        seed,
    );
    b.set_goal(Some(Goal {
        item: SHELTER.into(),
        commanded: false,
    }));
    let mut rows = vec![];
    run_with(setup("first_night", seed), &mut b, seed, &mut |r| {
        rows.push(r.clone())
    });
    rows
}

#[test]
fn a_young_agent_builds_a_hut_that_passes_and_a_grown_one_is_not_shown() {
    for seed in 0..3 {
        // ρ_eff = 0: chop, craft planks, build, each with the full bonus,
        // and the hut passes the test within 80 ticks.
        let rows = run(seed, 0.0);
        let shown: Vec<&str> = rows
            .iter()
            .filter(|r| r.shown > 0.0)
            .map(|r| r.chosen.split(':').next().unwrap())
            .collect();
        assert_eq!(shown, ["Chop", "Craft", "Shelter"], "seed {seed}");
        // The hut passes, which completes the goal (#58), within 80 ticks.
        // (`imagine` may set a new goal at once, so look for any other goal.)
        let done = rows
            .iter()
            .find(|r| r.goal.as_deref() != Some(SHELTER))
            .unwrap();
        assert!(done.tick < 80, "seed {seed}: done at {}", done.tick);
        // ρ_eff = 1: no bonus at all, but the test is still asked every tick.
        let rows = run(seed, 1.0);
        assert!(rows.iter().all(|r| r.shown == 0.0));
        // Until the goal is met: seed 0 builds a passing hut unshown at
        // tick 1,815 since dig-in needs soil (14 Nov sim audit).
        assert!(
            rows.iter()
                .take_while(|r| r.goal.as_deref() == Some(SHELTER))
                .all(|r| r.goal_test.is_some()),
            "seed {seed}"
        );
    }
}

#[test]
fn a_shelter_found_ready_made_completes_the_goal() {
    // #58: the test, not the recipe, decides. A stone ring with a door is
    // built around the agent before the run, so the test passes before any
    // recipe step is taken and the goal is done on the first learning step.
    for seed in 0..3 {
        let s = ready_made(seed);
        let mut b = Brain::new(BrainConfig::default(), seed);
        b.set_goal(Some(Goal {
            item: SHELTER.into(),
            commanded: false,
        }));
        let mut rows = vec![];
        let r = run_with(s, &mut b, seed, &mut |r| rows.push(r.clone()));
        assert_eq!(r.shelter_at, Some(0), "seed {seed}: passes from the start");
        let done = rows
            .iter()
            .position(|r| r.goal.as_deref() != Some(SHELTER))
            .unwrap();
        assert!(done <= 5, "seed {seed}: done at row {done}");
        assert!(
            rows[..done].iter().all(|r| r.chosen != "Shelter"),
            "seed {seed}: done without building"
        );
    }
}

#[test]
fn a_run_records_when_the_agent_first_stood_in_a_passing_shelter() {
    // Plan §5.0 transfer reads this. With no time to act, the end of the
    // run still counts: in a ready-made hut, tick 0; on open ground, never.
    let mut s = ready_made(0);
    s.max_ticks = 0;
    let mut b = Brain::new(BrainConfig::default(), 0);
    assert_eq!(run_with(s, &mut b, 0, &mut |_| {}).shelter_at, Some(0));
    let mut s = setup("first_night", 0);
    s.max_ticks = 0;
    let r = run_with(s, &mut b, 0, &mut |_| {});
    assert!(r.survived && r.shelter_at.is_none(), "{r:?}");
}

/// First night with a stone ring and a door built around the agent.
fn ready_made(seed: u64) -> sim::scenario::Setup {
    let mut s = setup("first_night", seed);
    let me = s.world.body.pos;
    for dx in -2i32..=2 {
        for dz in -2i32..=2 {
            if dx.abs().max(dz.abs()) == 2 {
                let p = host_api::Pos {
                    x: me.x + dx,
                    y: 0,
                    z: me.z + dz,
                };
                s.world.map.set(p, sim::map::Tile::Stone);
            }
        }
    }
    let door = host_api::Pos {
        x: me.x + 2,
        y: 0,
        z: me.z,
    };
    s.world.map.set(door, sim::map::Tile::Door);
    // Clear the floor, so nothing inside blocks the 4-way check.
    for dx in -1i32..=1 {
        for dz in -1i32..=1 {
            let p = host_api::Pos {
                x: me.x + dx,
                y: 0,
                z: me.z + dz,
            };
            s.world.map.set(p, sim::map::Tile::Grass);
        }
    }
    assert!(s.world.shelter_test().passes(), "seed {seed}");
    s
}

#[test]
fn a_run_records_the_first_tick_each_item_was_held() {
    // Plan §5.0 pace reads these, for every arm. Held at the start: the
    // start tick, once; a zero count is not held.
    let mut s = setup("held_out", 0);
    s.world.body.inv.items.push(("stick".into(), 0));
    s.max_ticks = 3;
    let mut b = Brain::new(BrainConfig::default(), 0);
    let r = run_with(s, &mut b, 0, &mut |_| {});
    assert!(r.decisions > 0);
    assert_eq!(r.start_tick, 0);
    assert_eq!(r.firsts[0], ("wooden_pickaxe".to_string(), 0));
    assert!(r.firsts.iter().all(|(i, _)| i != "stick"));
    let picks = r.firsts.iter().filter(|(i, _)| i == "wooden_pickaxe");
    assert_eq!(picks.count(), 1, "{:?}", r.firsts);
    // A scenario that starts mid-day keeps its start tick.
    let mut s = setup("harsh", 0);
    s.max_ticks = 0;
    let r = run_with(s, &mut b, 0, &mut |_| {});
    assert_eq!(r.start_tick, sim::scenario::DAY / 2);
}
