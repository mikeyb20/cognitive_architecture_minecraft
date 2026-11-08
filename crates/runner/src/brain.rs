//! Arm C: the full brain in the sim (build step 4). Wires the brain to the
//! sim's code tools, the scripted player and the shared emergency reflexes.

use cognitive_core::agent::{Brain, BrainConfig, Decision, Horizon, ThinkTools, TickLog};
use cognitive_core::direction::PlanStep;
use host_api::reflex::{ReflexThresholds, emergency_reflex};
use host_api::{Host, Observation, Skill, TaskType, World};
use sim::planner;
use sim::scenario::{self, Setup};
use sim::{Death, Rules};

use crate::FULL;

/// The sim's code tools (spec §21): the scripted planner and the
/// tech-tree recombiner stand in for the LLM.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimTools;

/// Dusk, from which the planner puts shelter first (same as arm A's tree).
pub const DUSK: f32 = 0.42;
/// Food level below which the planner puts food first.
pub const HUNGRY: f32 = 0.5;
const FOODS: [&str; 2] = ["porkchop", "bread"];

impl ThinkTools for SimTools {
    /// The scripted planner, survival first like an LLM planner would be:
    /// shelter and rest at dusk, food when hungry, then the goal.
    fn plan(&mut self, goal: &str, obs: &Observation) -> Option<PlanStep> {
        let step = |skill, thing: &str, task| PlanStep {
            skill,
            thing: thing.to_string(),
            task,
        };
        if obs.day_phase >= DUSK {
            return Some(if obs.sheltered {
                step(Skill::Rest, "", TaskType::Building)
            } else {
                step(Skill::Shelter, "", TaskType::Building)
            });
        }
        if obs.food < HUNGRY {
            if let Some(f) = FOODS.iter().find(|f| obs.inventory.count(f) > 0) {
                return Some(step(Skill::Eat, &format!("item:{f}"), TaskType::Farming));
            }
            if obs.visible.iter().any(|v| v.thing.0 == "entity:pig") {
                return Some(step(Skill::Attack, "entity:pig", TaskType::Farming));
            }
            if let Some(s) = planner::next_step(&obs.inventory, "bread") {
                let (skill, thing, task) = planner::in_ontology(s);
                return Some(PlanStep { skill, thing, task });
            }
        }
        planner::next_step(&obs.inventory, goal).map(|s| {
            let (skill, thing, task) = planner::in_ontology(s);
            PlanStep { skill, thing, task }
        })
    }

    fn progress(&mut self, goal: &str, obs: &Observation) -> f32 {
        planner::progress(&obs.inventory, goal)
    }

    fn propose_goal(&mut self, obs: &Observation) -> Option<String> {
        planner::next_goal(&obs.inventory).map(str::to_string)
    }
}

/// What one brain run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct BrainResult {
    pub scenario: &'static str,
    pub seed: u64,
    pub survived: bool,
    pub ticks: u64,
    pub death: Option<Death>,
    pub decisions: u32,
    pub idle: u32,
    pub thoughts: u32,
    pub reflexes: u32,
    pub new_things: usize,
    pub inventory: Vec<(String, u32)>,
}

/// Run one scenario with the brain. `on_row` sees every log row.
pub fn run_brain(
    setup: Setup,
    mut config: BrainConfig,
    seed: u64,
    mut on_row: impl FnMut(&TickLog),
) -> BrainResult {
    if let Some(h) = setup.horizon {
        config.horizon = Some(Horizon { ticks: h });
    }
    let mut brain = Brain::new(config, seed);
    run_with(setup, &mut brain, seed, &mut on_row)
}

/// Run with a brain the caller built (tests force states first).
pub fn run_with(
    setup: Setup,
    brain: &mut Brain,
    seed: u64,
    on_row: &mut dyn FnMut(&TickLog),
) -> BrainResult {
    let Setup {
        name,
        mut world,
        mut player,
        max_ticks,
        harass_until,
        ..
    } = setup;
    let end = brain.lifespan().map_or(max_ticks, |l| l.min(max_ticks));
    let mut tools = SimTools;
    let mut harsh_open = false;
    let mut r = BrainResult {
        scenario: name,
        seed,
        survived: false,
        ticks: 0,
        death: None,
        decisions: 0,
        idle: 0,
        thoughts: 0,
        reflexes: 0,
        new_things: 0,
        inventory: Vec::new(),
    };
    while world.tick < end && world.alive() {
        if world.tick < harass_until {
            scenario::harass(&mut world);
            harsh_open = true;
        } else if harsh_open {
            scenario::end_harsh(&mut world);
            harsh_open = false;
        }
        let mut obs = world.perceive();
        let mut cands = world.candidate_actions(&obs);
        player.apply(&mut obs, &mut cands);
        if let Some((_, c)) = emergency_reflex(&obs, &cands, ReflexThresholds::default()) {
            let c = c.clone();
            r.reflexes += 1;
            brain.reflex_fired(obs.tick);
            r.new_things += world.execute(&c, FULL).new_things.len();
            continue;
        }
        r.decisions += 1;
        match brain.step(&obs, &cands, &mut tools) {
            Decision::Act(i, it) => {
                let out = world.execute(&cands[i], it);
                r.new_things += out.new_things.len();
                brain.outcome(out);
            }
            Decision::Thought(_) => {
                r.thoughts += 1;
                world.step_world(1);
            }
            Decision::Idle => {
                r.idle += 1;
                world.step_world(1);
            }
        }
        on_row(brain.last_log());
    }
    r.survived = world.alive();
    r.ticks = world.tick;
    r.death = world.body.death;
    r.inventory = world.body.inv.items.clone();
    r
}

/// Build a scenario or panic with the known names (CLI and tests).
pub fn setup(name: &str, seed: u64) -> Setup {
    scenario::build(name, Rules::default(), seed)
        .unwrap_or_else(|| panic!("unknown scenario {name}; known: {:?}", scenario::NAMES))
}

/// Run header (spec §25): versions, scenario, seed, arm. The git commit and
/// config hash are added by the caller when it knows them.
pub fn header(scenario: &str, seed: u64) -> String {
    serde_json::json!({
        "header": true,
        "scenario": scenario,
        "seed": seed,
        "arm": "C",
        "llm_tier": null,
        "defs_version": cognitive_core::DEFS_VERSION,
        "paper_version": cognitive_core::PAPER_VERSION,
        "spec_version": cognitive_core::SPEC_VERSION,
    })
    .to_string()
}
