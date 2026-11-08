//! Build step 3 gate (spec §27): arm A survives the first night in at least
//! 90 of 100 seeds. A wanderer that never shelters is the control: if it
//! survives as often, the night is not dangerous and the gate means nothing.

use baselines::{ArmA, Policy};
use host_api::{Candidate, Observation, Skill};
use runner::{FIRST_NIGHT_END, first_night, run_episode};
use sim::Rules;

/// Explores and never shelters.
struct Wanderer;

impl Policy for Wanderer {
    fn choose(&mut self, _: &Observation, c: &[Candidate]) -> Option<usize> {
        c.iter().position(|x| x.skill == Skill::Explore)
    }
}

/// Does nothing; only the shared emergency reflexes act.
struct Idle;

impl Policy for Idle {
    fn choose(&mut self, _: &Observation, _: &[Candidate]) -> Option<usize> {
        None
    }
}

#[test]
fn gate_arm_a_survives_the_first_night_in_90_of_100_seeds() {
    let results = first_night(100, || Box::new(ArmA::default()));
    let survived = results.iter().filter(|r| r.survived).count();
    let deaths: Vec<_> = results
        .iter()
        .filter(|r| !r.survived)
        .map(|r| (r.seed, r.ticks, r.death))
        .collect();
    assert!(survived >= 90, "{survived}/100; deaths: {deaths:?}");
    for r in results.iter().filter(|r| r.survived) {
        assert!(r.ticks >= FIRST_NIGHT_END);
        assert!(r.death.is_none() && r.health > 0.0);
    }
}

#[test]
fn control_the_night_kills_agents_that_do_not_shelter() {
    let wander = first_night(100, || Box::new(Wanderer));
    let survived = wander.iter().filter(|r| r.survived).count();
    assert!(survived <= 75, "wanderer survived {survived}/100");
    assert!(wander.iter().any(|r| r.death == Some(sim::Death::Mob)));
}

#[test]
fn idle_agents_only_get_reflexes() {
    let runs: Vec<_> = (0..20)
        .map(|s| run_episode(Rules::default(), s, &mut Idle, FIRST_NIGHT_END))
        .collect();
    for r in runs.iter().filter(|r| r.reflexes == 0) {
        assert_eq!(
            u64::from(r.decisions),
            r.ticks,
            "idle waits one tick per decision"
        );
    }
    assert!(
        runs.iter().any(|r| r.reflexes > 0),
        "low health near a zombie triggers the flee reflex"
    );
}

#[test]
fn episodes_replay_by_seed() {
    let a = run_episode(Rules::default(), 7, &mut ArmA::default(), 3000);
    let b = run_episode(Rules::default(), 7, &mut ArmA::default(), 3000);
    assert_eq!(a, b);
    assert_eq!(a.seed, 7);
    assert!(a.new_things > 10, "arm A finds things: {}", a.new_things);
    assert!(a.food > 0.0);
}

#[test]
fn starving_triggers_the_eat_reflex() {
    let rules = Rules {
        food_decay: 0.01,
        ..Rules::default()
    };
    let r = run_episode(rules, 2, &mut ArmA::default(), 1500);
    assert!(r.reflexes > 0, "{r:?}");
}
