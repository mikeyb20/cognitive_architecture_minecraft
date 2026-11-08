//! Runs an arm in Minecraft (spec §22, §26): the runner decides, the bot
//! acts. Each decision: perceive → candidates (with player commands) →
//! emergency reflex (#42) or the arm's choice → execute on the bot.

mod host;
mod link;

pub use host::McHost;
pub use link::{Link, LinkError};

use baselines::Policy;
use host_api::reflex::{ReflexThresholds, emergency_reflex};
use host_api::{Candidate, Host, Observation, Outcome};

use crate::FULL;

/// One decision, for the log (spec §25).
#[derive(Debug, Clone, PartialEq)]
pub struct McStep {
    pub obs: Observation,
    pub chosen: Option<Candidate>,
    pub reflex: bool,
    pub outcome: Option<Outcome>,
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
        });
    }
    made
}
