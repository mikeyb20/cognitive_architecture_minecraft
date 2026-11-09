//! Felt needs (01 §1.6, #47) through the public API: hunger from the bar
//! pushes the action a teacher points at, the push fades as ρ rises, and
//! what relieved hunger keeps its push once the teacher is gone.

use cognitive_core::activation::{CuriosityParams, SignalWeights};
use cognitive_core::agent::{Brain, BrainConfig, Decision, Need, NeedParams, ThinkTools};
use cognitive_core::direction::PlanStep;
use host_api::{
    AgentId, Candidate, Inventory, Observation, Outcome, Pos, Skill, Target, TaskType, ThingId,
};

/// Points at one action while hungry, or teaches nothing.
struct Teacher(Option<(Skill, &'static str)>);

/// The teacher that says "eat bread".
fn bread() -> Teacher {
    Teacher(Some((Skill::Eat, "item:bread")))
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
    fn teach(&mut self, need: Need, obs: &Observation) -> Option<PlanStep> {
        assert_eq!(need, Need::Hunger);
        assert!(obs.food < 0.75, "the teacher is asked only while hungry");
        self.0.map(|(skill, thing)| PlanStep {
            skill,
            thing: thing.into(),
            task: TaskType::Farming,
        })
    }
}

fn obs(tick: u64, food: f32) -> Observation {
    Observation {
        agent: AgentId(0),
        tick,
        day_phase: 0.1,
        health: 1.0,
        food,
        position: Pos { x: 0, y: 0, z: 0 },
        inventory: Inventory {
            items: vec![("bread".into(), 5)],
        },
        visible: vec![],
        chat: vec![],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

fn cand(id: u32, skill: Skill, target: Target) -> Candidate {
    Candidate {
        id,
        skill,
        target: Some(target),
        task_type: TaskType::Farming,
        est_cost: 0.1,
        est_risk: 0.0,
        commanded: false,
    }
}

/// Eat bread, hunt a pig, chop a log or eat porkchop.
fn cands() -> Vec<Candidate> {
    vec![
        cand(0, Skill::Eat, Target::Item("bread".into())),
        cand(
            1,
            Skill::Attack,
            Target::Thing(ThingId("entity:pig".into())),
        ),
        cand(
            2,
            Skill::Chop,
            Target::Thing(ThingId("block:oak_log".into())),
        ),
        cand(3, Skill::Eat, Target::Item("porkchop".into())),
    ]
}

/// Drift (01 §1.5): C stays 0, so only a push can start anything.
fn drifting(needs: NeedParams) -> BrainConfig {
    BrainConfig {
        curiosity: CuriosityParams {
            base: 0.0,
            rate: 0.1,
        },
        k: SignalWeights {
            novelty: 0.0,
            learning_progress: 0.0,
            energy: 0.0,
            threat: 0.0,
            habituation: 0.0,
            social: 0.0,
        },
        kick_enabled: false,
        needs,
        ..BrainConfig::default()
    }
}

/// Step `n` ticks at a fixed food level; returns the chosen candidates.
fn run(b: &mut Brain, t: &mut Teacher, from: u64, n: u64, food: f32) -> Vec<usize> {
    let mut picks = vec![];
    for tick in from..from + n {
        if let Decision::Act(i, it) = b.step(&obs(tick, food), &cands(), t) {
            assert!(it.effort > 0.0);
            picks.push(i);
            b.outcome(Outcome::default());
        }
    }
    picks
}

#[test]
fn a_sated_drifting_agent_does_nothing() {
    let mut b = Brain::new(drifting(NeedParams::default()), 1);
    assert!(run(&mut b, &mut bread(), 0, 100, 0.9).is_empty());
    assert_eq!(b.last_log().hunger, 0.0);
}

#[test]
fn hunger_pushes_the_taught_action() {
    let mut b = Brain::new(drifting(NeedParams::default()), 1);
    let picks = run(&mut b, &mut bread(), 0, 200, 0.3);
    // N = (0.75 − 0.3)/0.75 = 0.6; push = 0.6 at ρ = 0, so the floor is
    // 0.2·0.6 = 0.12 and about 36% of ticks start (u ≥ 0.64).
    assert!((40..=110).contains(&picks.len()), "{}", picks.len());
    assert!(picks.iter().all(|&i| i == 0), "only the taught action");
    let l = b.last_log();
    assert!((l.hunger - 0.6).abs() < 1e-6, "{}", l.hunger);
    let acted = run(&mut b, &mut bread(), 200, 50, 0.3);
    assert!(!acted.is_empty());
    let l = b.last_log();
    if l.chosen != "idle" {
        assert!((l.need_push - 0.6).abs() < 1e-6 && l.external_drive);
    }
}

#[test]
fn no_need_and_a_faded_teacher_push_nothing() {
    let off = NeedParams {
        on: false,
        ..NeedParams::default()
    };
    let mut b = Brain::new(drifting(off), 1);
    assert!(run(&mut b, &mut bread(), 0, 100, 0.0).is_empty());
    assert_eq!(b.last_log().hunger, 0.0);
    // A fully developed agent (ρ = 1) gets nothing from the teacher...
    let grown = BrainConfig {
        rho_pre: 1.0,
        ..drifting(NeedParams::default())
    };
    let mut b = Brain::new(grown.clone(), 1);
    assert!(run(&mut b, &mut bread(), 0, 100, 0.0).is_empty());
    assert_eq!(b.last_log().hunger, 1.0, "it still feels hunger");
    // ...unless the hints are set not to fade.
    let kept = BrainConfig {
        needs: NeedParams {
            fades: false,
            ..NeedParams::default()
        },
        ..grown
    };
    let mut b = Brain::new(kept, 1);
    assert!(!run(&mut b, &mut bread(), 0, 100, 0.0).is_empty());
}

/// Step until the brain starts candidate `want`, then report `out`.
fn act_once(b: &mut Brain, t: &mut Teacher, tick: &mut u64, want: usize, out: Outcome) {
    let start = *tick;
    while !matches!(b.step(&obs(*tick, 0.0), &cands(), t), Decision::Act(i, _) if i == want) {
        *tick += 1;
        assert!(*tick < start + 200, "never chose {want}");
    }
    b.outcome(out);
    *tick += 1;
}

#[test]
fn what_relieved_hunger_keeps_its_push_without_the_teacher() {
    let mut b = Brain::new(drifting(NeedParams::default()), 3);
    let mut tick = 0;
    act_once(&mut b, &mut bread(), &mut tick, 0, Outcome::default());
    // Learning runs on the next tick: the bar rose 0 → 0.25, so eating
    // bread relieved hunger (relief 0.5). No teacher from here on.
    let mut alone = Teacher(None);
    let _ = b.step(&obs(tick, 0.25), &cands(), &mut alone);
    let picks = run(&mut b, &mut alone, tick + 1, 300, 0.25);
    assert!(
        !picks.is_empty() && picks.iter().all(|&i| i == 0),
        "{picks:?}"
    );
    // N = 0.5/0.75 = 2/3; push = 2/3 × 0.5 = 1/3.
    let mut t = tick + 301;
    loop {
        let _ = b.step(&obs(t, 0.25), &cands(), &mut alone);
        if b.last_log().chosen != "idle" && b.last_log().need_push > 0.0 {
            break;
        }
        t += 1;
    }
    let l = b.last_log();
    assert!((l.need_push - 1.0 / 3.0).abs() < 1e-6, "{}", l.need_push);
    assert!((l.hunger - 2.0 / 3.0).abs() < 1e-6 && l.external_drive);
}

#[test]
fn a_hunt_learns_relief_only_through_a_food_known_to_relieve() {
    let pork_n = |n| Outcome {
        success: true,
        items_gained: Inventory {
            items: vec![("porkchop".into(), n)],
        },
        ..Outcome::default()
    };
    let pork = || pork_n(1);
    let hunt = || Teacher(Some((Skill::Attack, "entity:pig")));
    let mut b = Brain::new(drifting(NeedParams::default()), 5);
    let mut alone = Teacher(None);
    // Nothing taught and nothing learned: hunger alone starts nothing.
    assert!(run(&mut b, &mut alone, 0, 100, 0.0).is_empty());
    // A hunt that yields porkchop, before porkchop is known as food,
    // teaches nothing (the bar did not rise).
    let mut tick = 100;
    act_once(&mut b, &mut hunt(), &mut tick, 1, pork());
    assert!(run(&mut b, &mut alone, tick, 100, 0.0).is_empty());
    // Eating porkchop that fills the bar makes porkchop a known food...
    let mut c = Brain::new(drifting(NeedParams::default()), 5);
    let mut eat_pork = Teacher(Some((Skill::Eat, "item:porkchop")));
    let mut tick = 0;
    act_once(&mut c, &mut eat_pork, &mut tick, 3, Outcome::default());
    let _ = c.step(&obs(tick, 0.25), &cands(), &mut alone);
    // ...a hunt that yields none of it still teaches nothing...
    act_once(&mut c, &mut hunt(), &mut tick, 1, pork_n(0));
    let picks = run(&mut c, &mut alone, tick, 100, 0.0);
    assert!(!picks.is_empty() && !picks.contains(&1), "{picks:?}");
    tick += 100;
    // ...and then a hunt that yields it learns relief too.
    act_once(&mut c, &mut hunt(), &mut tick, 1, pork());
    let _ = c.step(&obs(tick, 0.0), &cands(), &mut alone);
    let picks = run(&mut c, &mut alone, tick + 1, 300, 0.0);
    assert!(picks.contains(&1), "the hunt is pushed: {picks:?}");
    assert!(!picks.contains(&2), "chopping never relieved");
}

/// An active agent: C held at 0.8, so g(C) starts most ticks by itself.
fn active(steer: f32) -> BrainConfig {
    BrainConfig {
        curiosity: CuriosityParams {
            base: 0.8,
            rate: 0.0,
        },
        needs: NeedParams {
            steer,
            ..NeedParams::default()
        },
        ..drifting(NeedParams::default())
    }
}

#[test]
fn hunger_steers_what_an_active_agent_picks() {
    // 01 §1.6 (#51): the effort floor adds nothing once g(C) starts the
    // tick anyway; the score bonus is what favours the taught food.
    let eats = |steer: f32| -> (usize, usize) {
        let mut picks = vec![];
        for seed in 0..5 {
            let mut b = Brain::new(active(steer), seed);
            picks.extend(run(&mut b, &mut bread(), 0, 200, 0.0));
        }
        (picks.iter().filter(|&&i| i == 0).count(), picks.len())
    };
    let (floor_only, n0) = eats(0.0);
    let (steered, n1) = eats(1.0);
    // Over 1,000 ticks: without the bonus the busy agent mostly thinks or
    // does other things; with it, eating wins most choices.
    assert!(
        steered > 3 * floor_only.max(1),
        "eats {floor_only}/{n0} → {steered}/{n1}"
    );
    assert!(
        steered * 2 > n1,
        "eating is most of what it does: {steered}/{n1}"
    );
    // A sated agent gets no bonus: steering changes nothing at food 0.9.
    let sated = |steer| {
        let mut b = Brain::new(active(steer), 7);
        run(&mut b, &mut bread(), 0, 200, 0.9)
    };
    assert_eq!(sated(0.0), sated(1.0));
}
