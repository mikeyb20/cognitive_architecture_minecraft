//! The teacher's "show" and "explain" (01 §4.11, #59; spec §27 step 13)
//! through the public API: a shown recipe's next step gets a choice bonus
//! `w_R·(1 − ρ_eff)` that fades as ρ rises, the lesson moves on as steps
//! succeed, and the goal's test is stored and reported whatever ρ is.

use cognitive_core::agent::{Brain, BrainConfig, Decision, Goal, ThinkTools};
use cognitive_core::control::teacher::{Lesson, Recipe, TeacherParams, shown, step_matches};
use cognitive_core::direction::PlanStep;
use cognitive_core::memory::{Source, Tier};
use host_api::{
    AgentId, Candidate, Inventory, Observation, Outcome, Pos, Skill, Target, TaskType, ThingId,
};
use proptest::prelude::*;

fn step(skill: Skill, thing: &str) -> PlanStep {
    PlanStep {
        skill,
        thing: thing.into(),
        task: TaskType::Building,
    }
}

fn recipe() -> Recipe {
    Recipe {
        goal: "shelter".into(),
        steps: vec![
            step(Skill::Chop, "block:oak_log"),
            step(Skill::Craft, "item:oak_planks"),
            step(Skill::Shelter, ""),
        ],
    }
}

/// Shows the shelter recipe and reports a fixed test result.
struct Teacher {
    test: Option<bool>,
    shows: u32,
}

impl ThinkTools for Teacher {
    fn plan(&mut self, _: &str, _: &Observation) -> Option<PlanStep> {
        None
    }
    fn progress(&mut self, _: &str, _: &Observation) -> f32 {
        0.0
    }
    fn propose_goal(&mut self, _: &Observation) -> Option<String> {
        None
    }
    fn show(&mut self, goal: &str, _: &Observation) -> Option<Recipe> {
        self.shows += 1;
        (goal == "shelter").then(recipe)
    }
    fn explain(&mut self, goal: &str, _: &Observation) -> Option<bool> {
        if goal == "shelter" { self.test } else { None }
    }
}

fn obs(tick: u64) -> Observation {
    Observation {
        agent: AgentId(0),
        tick,
        day_phase: 0.1,
        health: 1.0,
        food: 1.0,
        position: Pos { x: 0, y: 0, z: 0 },
        inventory: Inventory::default(),
        visible: vec![],
        chat: vec![],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

fn cand(id: u32, skill: Skill, target: Option<Target>) -> Candidate {
    Candidate {
        id,
        skill,
        target,
        task_type: TaskType::Building,
        est_cost: 0.1,
        est_risk: 0.0,
        commanded: false,
    }
}

/// Chop a log, craft planks, build a shelter, explore.
fn cands() -> Vec<Candidate> {
    vec![
        cand(
            0,
            Skill::Chop,
            Some(Target::Thing(ThingId("block:oak_log".into()))),
        ),
        cand(1, Skill::Craft, Some(Target::Item("oak_planks".into()))),
        cand(2, Skill::Shelter, None),
        cand(
            3,
            Skill::Explore,
            Some(Target::Pos(Pos { x: 9, y: 0, z: 9 })),
        ),
    ]
}

fn brain(rho_pre: f32, teacher: TeacherParams) -> Brain {
    let mut b = Brain::new(
        BrainConfig {
            rho_pre,
            teacher,
            ..BrainConfig::default()
        },
        5,
    );
    b.set_goal(Some(Goal {
        item: "shelter".into(),
        commanded: false,
    }));
    b
}

/// Run `n` ticks; every action succeeds. Returns (chosen, shown bonus,
/// recipe step) per tick that acted.
fn run(b: &mut Brain, t: &mut Teacher, n: u64) -> Vec<(usize, f32, Option<String>)> {
    let mut out = vec![];
    for tick in 0..n {
        if let Decision::Act(i, _) = b.step(&obs(tick), &cands(), t) {
            let l = b.last_log();
            out.push((i, l.shown, l.recipe_step.clone()));
            b.outcome(Outcome {
                success: true,
                ..Outcome::default()
            });
        }
    }
    out
}

proptest! {
    #[test]
    fn the_bonus_stays_in_range_and_fades_with_rho(w in 0.0f32..=2.0, rho in 0.0f32..=1.0) {
        let b = shown(w, rho);
        prop_assert!((0.0..=w).contains(&b));
        prop_assert!(shown(w, (rho + 0.1).min(1.0)) <= b);
    }
}

#[test]
fn the_bonus_and_matching_by_hand() {
    assert_eq!(shown(1.0, 0.0), 1.0);
    assert!((shown(1.0, 0.25) - 0.75).abs() < 1e-6);
    assert!((shown(2.0, 0.6) - 0.8).abs() < 1e-6);
    assert_eq!(shown(1.0, 1.0), 0.0);
    assert_eq!(shown(1.0, 1.5), 0.0, "ρ is clamped");
    let s = step(Skill::Chop, "block:oak_log");
    assert!(step_matches(&s, Skill::Chop, Some("block:oak_log")));
    assert!(!step_matches(&s, Skill::Chop, Some("block:stone")));
    assert!(!step_matches(&s, Skill::Mine, Some("block:oak_log")));
    assert!(!step_matches(&s, Skill::Chop, None));
    assert!(step_matches(
        &step(Skill::Shelter, ""),
        Skill::Shelter,
        None
    ));
    let mut l = Lesson::new(recipe());
    l.record(Skill::Craft, Some("item:oak_planks"), true);
    assert_eq!(l.next, 0, "out of order does not count");
    l.record(Skill::Chop, Some("block:oak_log"), false);
    assert_eq!(l.next, 0, "a failure does not count");
    l.record(Skill::Chop, Some("block:oak_log"), true);
    l.record(Skill::Craft, Some("item:oak_planks"), true);
    assert_eq!(l.step(), Some(&step(Skill::Shelter, "")));
    l.record(Skill::Shelter, None, true);
    assert_eq!((l.next, l.step()), (3, None), "done");
}

#[test]
fn a_young_agent_follows_the_shown_recipe_step_by_step() {
    let mut b = brain(0.0, TeacherParams::default());
    let mut t = Teacher {
        test: Some(false),
        shows: 0,
    };
    let acts = run(&mut b, &mut t, 400);
    assert_eq!(t.shows, 1, "shown once");
    // The first action on each step is that step, with the full bonus.
    let firsts: Vec<usize> = acts
        .iter()
        .filter(|(_, bonus, _)| *bonus > 0.0)
        .map(|(i, _, _)| *i)
        .collect();
    assert_eq!(&firsts[..3], &[0, 1, 2], "{acts:?}");
    assert!(
        acts.iter()
            .all(|(_, bonus, _)| *bonus == 0.0 || (*bonus - 1.0).abs() < 1e-6)
    );
    assert_eq!(b.last_log().recipe_step, None, "the recipe is done");
    // #58: the recipe ran to its end, but the test fails, so the goal stays.
    assert_eq!(b.goal().map(|g| g.item.as_str()), Some("shelter"));
    let m = b.memory();
    let r = m.get("recipe:shelter").expect("recipe stored");
    assert_eq!(m.entry(r).item.tier, Tier::Procedural);
    assert_eq!(m.entry(r).source, Source::Teacher);
    assert!((m.entry(r).confidence - 0.6).abs() < 1e-6);
}

#[test]
fn a_developed_agent_gets_no_bonus_but_keeps_the_test() {
    let mut b = brain(1.0, TeacherParams::default());
    let mut t = Teacher {
        test: Some(false),
        shows: 0,
    };
    let acts = run(&mut b, &mut t, 200);
    assert!(acts.iter().all(|(_, bonus, _)| *bonus == 0.0));
    let l = b.last_log();
    assert_eq!(l.goal_test, Some(false), "the test does not fade");
    let m = b.memory();
    let test = m.get("test:shelter").expect("test stored");
    assert_eq!(m.entry(test).item.tier, Tier::Semantic);
    assert_eq!(m.entry(test).source, Source::Teacher);
}

#[test]
fn with_the_teacher_off_nothing_is_shown_or_explained() {
    let off = TeacherParams {
        show: false,
        ..TeacherParams::default()
    };
    let mut b = brain(0.0, off);
    let mut t = Teacher {
        test: Some(true),
        shows: 0,
    };
    let acts = run(&mut b, &mut t, 100);
    assert_eq!(t.shows, 0);
    assert!(
        acts.iter()
            .all(|(_, bonus, step)| *bonus == 0.0 && step.is_none())
    );
    assert_eq!(b.last_log().goal_test, None);
    assert!(b.memory().get("recipe:shelter").is_none());
}

#[test]
fn a_new_goal_drops_the_old_lesson() {
    let mut b = brain(0.0, TeacherParams::default());
    let mut t = Teacher {
        test: None,
        shows: 0,
    };
    let _ = b.step(&obs(0), &cands(), &mut t);
    assert!(b.last_log().recipe_step.is_some());
    // Setting the same goal again keeps it; another goal drops it.
    b.set_goal(Some(Goal {
        item: "shelter".into(),
        commanded: true,
    }));
    let _ = b.step(&obs(1), &cands(), &mut t);
    assert_eq!(t.shows, 1);
    b.set_goal(Some(Goal {
        item: "stick".into(),
        commanded: false,
    }));
    let _ = b.step(&obs(2), &cands(), &mut t);
    let _ = b.step(&obs(3), &cands(), &mut t);
    assert_eq!(b.last_log().recipe_step, None);
    assert_eq!(
        t.shows, 2,
        "asked once about the new goal, which has no recipe"
    );
}

/// Step until the goal is gone or `n` ticks pass; returns the tick it went.
fn until_done(b: &mut Brain, t: &mut Teacher, from: u64, n: u64) -> Option<u64> {
    for tick in from..from + n {
        let _ = b.step(&obs(tick), &cands(), t);
        b.outcome(Outcome::default());
        if b.goal().is_none() {
            return Some(tick);
        }
    }
    None
}

#[test]
fn a_goal_is_done_when_its_known_test_passes() {
    // #58: however the agent got there, a passing test completes the goal.
    let mut b = brain(1.0, TeacherParams::default());
    let mut t = Teacher {
        test: Some(false),
        shows: 0,
    };
    assert_eq!(until_done(&mut b, &mut t, 0, 50), None, "fails: not done");
    t.test = Some(true);
    let done = until_done(&mut b, &mut t, 50, 5).expect("done once it passes");
    assert_eq!(done, 50, "on the next learning step");
    assert!(b.last_log().d_goal > 0.0, "and it counts as goal progress");
}

#[test]
fn a_test_the_agent_was_never_taught_does_not_count() {
    let off = TeacherParams {
        show: false,
        ..TeacherParams::default()
    };
    let mut b = brain(0.0, off);
    let mut t = Teacher {
        test: Some(true),
        shows: 0,
    };
    assert_eq!(until_done(&mut b, &mut t, 0, 50), None);
}
