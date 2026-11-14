//! Arm C in Minecraft (spec §22, §26): the full brain drives the bot. Same
//! order as the sim driver: perceive → candidates → emergency reflex (#42)
//! or the brain's decision → execute on the bot, outcome back to the brain.

use cognitive_core::agent::{Brain, Decision};
use host_api::Host;
use host_api::reflex::{ReflexThresholds, emergency_reflex};

use super::{McHost, McStep};
use crate::FULL;
use crate::brain::SimTools;

/// What one Minecraft run of the brain did, for the CLI summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrainCounts {
    pub acts: u32,
    pub thoughts: u32,
    pub idle: u32,
    pub reflexes: u32,
}

impl BrainCounts {
    pub fn decisions(&self) -> u32 {
        self.acts + self.thoughts + self.idle + self.reflexes
    }
}

/// Up to `decisions` decisions by `brain` on the bot, or until the link
/// fails (spec §22). The sim's code tools stand in for the LLM (spec §21).
/// `wait` runs after a thought or idle tick, so the game moves on; the CLI
/// passes a one-tick sleep. Calls `log` after each decision.
pub fn run_brain(
    host: &mut McHost,
    brain: &mut Brain,
    decisions: u32,
    thresholds: ReflexThresholds,
    mut wait: impl FnMut(),
    mut log: impl FnMut(&McStep),
) -> BrainCounts {
    let mut tools = SimTools::default();
    let mut n = BrainCounts::default();
    for _ in 0..decisions {
        if host.error.is_some() {
            break;
        }
        let obs = host.perceive();
        let cands = host.candidate_actions(&obs);
        let (chosen, it, reflex) = match emergency_reflex(&obs, &cands, thresholds) {
            Some((_, c)) => {
                n.reflexes += 1;
                brain.reflex_fired(obs.tick);
                (Some(c.clone()), FULL, true)
            }
            None => match brain.step(&obs, &cands, &mut tools) {
                Decision::Act(i, it) => {
                    n.acts += 1;
                    (Some(cands[i].clone()), it, false)
                }
                Decision::Thought(_) => {
                    n.thoughts += 1;
                    (None, FULL, false)
                }
                Decision::Idle => {
                    n.idle += 1;
                    (None, FULL, false)
                }
            },
        };
        let outcome = chosen.as_ref().map(|c| host.execute(c, it));
        match (&outcome, reflex) {
            (Some(out), false) => brain.outcome(out.clone()),
            (None, _) => wait(),
            _ => {}
        }
        log(&McStep {
            obs,
            chosen,
            reflex,
            outcome,
            llm: None,
        });
    }
    n
}
