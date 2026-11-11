//! Build step 13 gate (spec §27; 01 §4.11, #59): the sim teacher shows the
//! shelter recipe while ρ is low and stops as ρ rises; the shelter test
//! stays available.

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
        let pass = rows.iter().find(|r| r.goal_test == Some(true)).unwrap();
        assert!(pass.tick < 80, "seed {seed}: passed at {}", pass.tick);
        // ρ_eff = 1: no bonus at all, but the test is still asked every tick.
        let rows = run(seed, 1.0);
        assert!(rows.iter().all(|r| r.shown == 0.0));
        assert!(rows.iter().all(|r| r.goal_test.is_some()), "seed {seed}");
    }
}
