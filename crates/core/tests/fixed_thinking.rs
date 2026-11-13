//! Fixed-trigger thinking, an ablation (plan §5.2; #40): plan, reflect and
//! imagine leave the choice and run on a new goal and on a timer.

use cognitive_core::agent::{Brain, BrainConfig, FixedThinking, ThinkTools};
use cognitive_core::direction::PlanStep;
use host_api::{AgentId, ChatMsg, Inventory, Observation, Pos, Skill, TaskType};

/// A planner that always says "chop a log", counting its calls; it
/// proposes "stick" as an own goal when `propose` is set.
#[derive(Default)]
struct Counting {
    propose: bool,
    plans: Vec<(u64, String)>,
    proposals: u32,
}

impl Counting {
    fn proposing() -> Self {
        Self {
            propose: true,
            ..Self::default()
        }
    }
}

impl ThinkTools for Counting {
    fn plan(&mut self, goal: &str, obs: &Observation) -> Option<PlanStep> {
        self.plans.push((obs.tick, goal.to_string()));
        Some(PlanStep {
            skill: Skill::Chop,
            thing: "block:oak_log".into(),
            task: TaskType::Mining,
        })
    }
    fn progress(&mut self, _: &str, _: &Observation) -> f32 {
        0.0
    }
    fn propose_goal(&mut self, _: &Observation) -> Option<String> {
        self.proposals += 1;
        self.propose.then(|| "stick".into())
    }
}

fn obs(tick: u64, said: Option<&str>) -> Observation {
    Observation {
        agent: AgentId(0),
        tick,
        day_phase: 0.1,
        health: 1.0,
        food: 1.0,
        position: Pos { x: 0, y: 0, z: 0 },
        inventory: Inventory::default(),
        visible: vec![],
        chat: said
            .map(|t| ChatMsg {
                from: "steve".into(),
                text: t.into(),
            })
            .into_iter()
            .collect(),
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

fn fixed(every: u64) -> BrainConfig {
    BrainConfig {
        fixed_thinking: FixedThinking {
            on: true,
            every,
            effort: 0.5,
        },
        ..BrainConfig::default()
    }
}

/// Step the brain once with no candidates; returns `triggered`.
fn step(b: &mut Brain, t: &mut Counting, tick: u64, said: Option<&str>) -> u32 {
    b.step(&obs(tick, said), &[], t);
    b.last_log().triggered
}

#[test]
fn the_default_chooses_its_thoughts() {
    let mut b = Brain::new(BrainConfig::default(), 1);
    let mut t = Counting::default();
    for tick in 0..50 {
        assert_eq!(step(&mut b, &mut t, tick, None), 0);
    }
    assert_eq!(b.last_log().n_choices, 5, "all five internal actions");
}

#[test]
fn thoughts_run_on_the_timer_and_on_new_goals() {
    let mut b = Brain::new(fixed(100), 1);
    let mut t = Counting::proposing();
    // Tick 0, the timer: reflect, imagine (proposes "stick", a new goal),
    // then plan for it.
    assert_eq!(step(&mut b, &mut t, 0, None), 3);
    assert_eq!(t.proposals, 1);
    assert_eq!(t.plans, vec![(0, "stick".to_string())]);
    assert_eq!(
        b.last_log().plan_step.as_deref(),
        Some("Chop:block:oak_log")
    );
    assert_eq!(
        b.last_log().n_choices,
        2,
        "only rehearse and consolidate are chosen"
    );
    assert!(
        !matches!(b.last_log().chosen.as_str(), "Plan" | "Reflect" | "Imagine"),
        "{}",
        b.last_log().chosen
    );
    // Nothing fires until the timer or a new goal.
    for tick in 1..40 {
        assert_eq!(step(&mut b, &mut t, tick, None), 0, "tick {tick}");
    }
    // A command is a new goal: plan at once, nothing else.
    assert_eq!(step(&mut b, &mut t, 40, Some("get iron")), 1);
    assert_eq!(t.plans.last(), Some(&(40, "iron".to_string())));
    // The same goal again later is still a new goal.
    assert_eq!(step(&mut b, &mut t, 41, Some("get iron")), 1);
    for tick in 42..100 {
        assert_eq!(step(&mut b, &mut t, tick, None), 0, "tick {tick}");
    }
    // Tick 100, the timer: reflect, imagine (a goal is held, so no
    // proposal) and re-plan.
    assert_eq!(step(&mut b, &mut t, 100, None), 3);
    assert_eq!((t.proposals, t.plans.len()), (1, 4));
    assert_eq!(step(&mut b, &mut t, 101, None), 0);
}

#[test]
fn without_a_goal_the_timer_does_not_plan() {
    let mut b = Brain::new(fixed(10), 2);
    let mut t = Counting::default();
    // No proposal, so no goal and no plan: reflect and imagine only.
    assert_eq!(step(&mut b, &mut t, 0, None), 2);
    assert!(b.goal().is_none() && t.plans.is_empty());
    assert_eq!(t.proposals, 1);
    // The timer is "at or after": a tick that skips past it still fires.
    assert_eq!(step(&mut b, &mut t, 25, None), 2);
    assert_eq!(step(&mut b, &mut t, 34, None), 0, "next timer at 35");
    t.propose = true;
    assert_eq!(step(&mut b, &mut t, 35, None), 3);
}
