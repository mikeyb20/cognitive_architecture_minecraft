//! Runs an arm in Minecraft (spec §22, §26): the runner decides, the bot
//! acts. Each decision: perceive → candidates (with player commands) →
//! emergency reflex (#42) or the arm's choice → execute on the bot. Arms A,
//! B and B+N choose through `Policy`; the brain (arm C) runs in `run_brain`.

mod brain;
mod host;
mod link;
mod setup;

pub use brain::{BrainCounts, BrainRun, run_brain};
pub use host::McHost;
pub use link::{Link, LinkError};
pub use setup::{McArm, arm, arm_with, provider, tier};

use baselines::Policy;
use host_api::reflex::{ReflexThresholds, emergency_reflex};
use host_api::{Candidate, Host, Observation, Outcome};
use llm::LlmStats;
use serde_json::{Value, json};

use crate::FULL;

/// One decision, for the log (spec §25).
#[derive(Debug, Clone, PartialEq)]
pub struct McStep {
    pub obs: Observation,
    pub chosen: Option<Candidate>,
    pub reflex: bool,
    pub outcome: Option<Outcome>,
    /// The arm's LLM use so far, if it calls one.
    pub llm: Option<LlmStats>,
    /// The brain's goal after the decision, and the teacher's shown
    /// recipe step (01 §4.11), for the brain arm; `None` otherwise.
    pub goal: Option<String>,
    pub recipe_step: Option<String>,
}

/// Up to `decisions` decisions, or until the link fails. Calls `log` after
/// each one.
pub fn run(
    host: &mut McHost,
    policy: &mut dyn Policy,
    decisions: u32,
    thresholds: ReflexThresholds,
    mut log: impl FnMut(&McStep),
) -> u32 {
    let mut made = 0;
    for _ in 0..decisions {
        if host.error.is_some() {
            break;
        }
        let obs = host.perceive();
        let cands = host.candidate_actions(&obs);
        let (chosen, reflex) = match emergency_reflex(&obs, &cands, thresholds) {
            Some((_, c)) => (Some(c.clone()), true),
            None => (policy.choose(&obs, &cands).map(|i| cands[i].clone()), false),
        };
        let outcome = chosen.as_ref().map(|c| host.execute(c, FULL));
        made += 1;
        log(&McStep {
            obs,
            chosen,
            reflex,
            outcome,
            llm: policy.llm_stats().cloned(),
            goal: None,
            recipe_step: None,
        });
    }
    made
}

/// One JSONL log row for a decision (spec §25: body, choice, outcome, LLM),
/// with the chat heard and, for the brain, its goal and the teacher's step.
pub fn row(s: &McStep) -> Value {
    let o = &s.obs;
    json!({
        "tick": o.tick,
        "health": o.health,
        "food": o.food,
        "day_phase": o.day_phase,
        "position": o.position,
        "sheltered": o.sheltered,
        "chosen": s.chosen,
        "reflex": s.reflex,
        "outcome": s.outcome,
        "chat": o.chat.iter().map(|c| format!("{}: {}", c.from, c.text)).collect::<Vec<_>>(),
        "goal": s.goal,
        "recipe_step": s.recipe_step,
        "llm": s.llm.as_ref().map(|l| json!({
            "calls": l.calls,
            "cached": l.cached,
            "fallbacks": l.fallbacks,
            "unparsed": l.unparsed,
            "input_tokens": l.input_tokens,
            "output_tokens": l.output_tokens,
            "cost_usd": l.cost_usd,
        })),
    })
}
