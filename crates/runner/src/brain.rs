//! Arm C: the full brain in the sim (build step 4). Wires the brain to the
//! sim's code tools, the scripted player and the shared emergency reflexes.

use cognitive_core::agent::{Brain, BrainConfig, Decision, Horizon, Need, ThinkTools, TickLog};
use cognitive_core::control::teacher::Recipe;
use cognitive_core::direction::PlanStep;
use host_api::{Candidate, Outcome};
use host_api::{Observation, Skill, TaskType};
use sim::planner;
use sim::scenario::{self, Setup};
use sim::world::ShelterTest;
use sim::{Death, Rules};

use crate::drive::{Driver, run_scenario};

/// The sim's code tools (spec §21): the scripted planner and the
/// tech-tree recombiner stand in for the LLM. As the teacher (01 §4.11)
/// they also report the host's shelter test, set before each decision.
#[derive(Debug, Clone, Copy, Default)]
pub struct SimTools {
    pub shelter: Option<bool>,
}

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
            if let Some(s) = food_step(obs) {
                return Some(s);
            }
        }
        planned(&obs.inventory, goal)
    }

    fn progress(&mut self, goal: &str, obs: &Observation) -> f32 {
        planner::progress(&obs.inventory, goal)
    }

    fn propose_goal(&mut self, obs: &Observation) -> Option<String> {
        planner::next_goal(&obs.inventory).map(str::to_string)
    }

    /// The teacher for felt hunger (01 §1.6): the planner's food step.
    fn teach(&mut self, need: Need, obs: &Observation) -> Option<PlanStep> {
        match need {
            Need::Hunger => food_step(obs),
        }
    }

    /// The teacher shows how to build a shelter (01 §4.11, #59).
    fn show(&mut self, goal: &str, obs: &Observation) -> Option<Recipe> {
        (goal == SHELTER).then(|| shelter_recipe(&obs.inventory))
    }

    /// The shelter test (01 §4.11): no hostile has a path in, and a way out.
    fn explain(&mut self, goal: &str, _obs: &Observation) -> Option<bool> {
        if goal == SHELTER { self.shelter } else { None }
    }
}

/// The goal the sim teacher can show and explain (01 §4.11).
pub const SHELTER: &str = "shelter";

/// The shelter recipe from what the agent holds (01 §4.11; spec §27 step
/// 13): a hut needs 4 blocks, so chop a log and craft planks unless it
/// holds 4 cobblestone or planks, then build.
pub fn shelter_recipe(inv: &host_api::Inventory) -> Recipe {
    let step = |skill, thing: &str, task| PlanStep {
        skill,
        thing: thing.to_string(),
        task,
    };
    let mut steps = Vec::new();
    if inv.count("cobblestone") < 4 && inv.count("oak_planks") < 4 {
        if inv.count("oak_log") == 0 {
            steps.push(step(Skill::Chop, "block:oak_log", TaskType::Mining));
        }
        steps.push(step(Skill::Craft, "item:oak_planks", TaskType::Crafting));
    }
    steps.push(step(Skill::Shelter, "", TaskType::Building));
    Recipe {
        goal: SHELTER.to_string(),
        steps,
    }
}

/// The planner's next step toward `goal`, in ontology names.
fn planned(inv: &host_api::Inventory, goal: &str) -> Option<PlanStep> {
    planner::next_step(inv, goal).map(|s| {
        let (skill, thing, task) = planner::in_ontology(s);
        PlanStep { skill, thing, task }
    })
}

/// The step that gets food: eat what is held, else hunt a visible pig,
/// else work toward bread.
pub fn food_step(obs: &Observation) -> Option<PlanStep> {
    let step = |skill, thing: String| PlanStep {
        skill,
        thing,
        task: TaskType::Farming,
    };
    if let Some(f) = FOODS.iter().find(|f| obs.inventory.count(f) > 0) {
        return Some(step(Skill::Eat, format!("item:{f}")));
    }
    if obs.visible.iter().any(|v| v.thing.0 == "entity:pig") {
        return Some(step(Skill::Attack, "entity:pig".into()));
    }
    planned(&obs.inventory, "bread")
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

/// The brain as a scenario driver, thinking with the sim's code tools.
struct BrainDriver<'a> {
    brain: &'a mut Brain,
    tools: SimTools,
}

impl Driver for BrainDriver<'_> {
    fn decide(&mut self, obs: &Observation, cands: &[Candidate]) -> Decision {
        self.brain.step(obs, cands, &mut self.tools)
    }
    fn outcome(&mut self, out: Outcome) {
        self.brain.outcome(out);
    }
    fn reflex_fired(&mut self, tick: u64) {
        self.brain.reflex_fired(tick);
    }
    fn log(&self) -> Option<&TickLog> {
        Some(self.brain.last_log())
    }
    fn see_tests(&mut self, shelter: ShelterTest) {
        self.tools.shelter = Some(shelter.passes());
    }
}

/// Run with a brain the caller built (tests force states first).
pub fn run_with(
    setup: Setup,
    brain: &mut Brain,
    seed: u64,
    on_row: &mut dyn FnMut(&TickLog),
) -> BrainResult {
    let end = brain.lifespan();
    let mut d = BrainDriver {
        brain,
        tools: SimTools::default(),
    };
    run_scenario(setup, &mut d, end, seed, on_row)
}

/// World ticks the default brain runs on `open_day` over seeds 0..n (the
/// spec §27 speed gate times this).
pub fn speed_ticks(n: u64) -> u64 {
    (0..n)
        .map(|seed| {
            run_brain(
                setup("open_day", seed),
                BrainConfig::default(),
                seed,
                |_| {},
            )
            .ticks
        })
        .sum()
}

/// Ticks per second (spec §27).
pub fn rate(ticks: u64, secs: f64) -> f64 {
    ticks as f64 / secs
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

#[cfg(test)]
mod tests {
    use super::*;

    // A unit test so it runs before the long integration tests (mutation
    // testing stops at the first failing test binary). A starving agent past
    // the tick limit must not act: run `while tick < end && alive`.
    #[test]
    fn nothing_runs_past_the_tick_limit() {
        let mut s = setup("first_night", 0);
        s.max_ticks = 0;
        (s.world.body.food, s.world.body.health) = (0.0, 0.05);
        let mut brain = Brain::new(BrainConfig::default(), 0);
        let r = run_with(s, &mut brain, 0, &mut |_| {});
        assert!(r.survived, "{r:?}");
        let r = (r.decisions, r.ticks);
        assert_eq!(r, (0, 0));
    }

    // The driver passes reflexes on, so O learns from them (spec §7.1).
    #[test]
    fn the_brain_driver_passes_reflexes_on() {
        use cognitive_core::modes::Mode;
        use host_api::{Host, World};
        let mut s = setup("first_night", 0);
        let mut brain = Brain::new(BrainConfig::default(), 0);
        let mut d = BrainDriver {
            brain: &mut brain,
            tools: SimTools::default(),
        };
        // Run until one action has been taken and learned from.
        let mut acted = false;
        let mut tick = 0;
        for _ in 0..500 {
            let obs = s.world.perceive();
            let cands = s.world.candidate_actions(&obs);
            tick = obs.tick;
            match d.decide(&obs, &cands) {
                Decision::Act(i, it) if !acted => {
                    let out = s.world.execute(&cands[i], it);
                    d.outcome(out);
                    acted = true;
                }
                _ if acted => break,
                _ => s.world.step_world(1),
            }
        }
        assert!(acted);
        let before = d.brain.lens(Mode::Optimizing).theta;
        d.reflex_fired(tick);
        assert_ne!(d.brain.lens(Mode::Optimizing).theta, before);
    }
}
