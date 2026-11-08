//! Episode loop shared by the CLI and the gate tests (spec §26).
//!
//! Each decision: perceive → candidates → emergency reflex (shared by every
//! arm, #42) or the arm's choice → execute.

pub mod archetype;
pub mod brain;
pub(crate) mod drive;
pub mod mc;
pub(crate) mod par;
pub mod sweep;
pub mod theory;
pub mod think;

use baselines::Policy;
use host_api::reflex::{ReflexThresholds, emergency_reflex};
use host_api::{Host, Intensity, World};
use sim::{Death, Rules, SimWorld};

/// Full effort and focus: the baselines have no g(C) or choking.
pub const FULL: Intensity = Intensity {
    effort: 1.0,
    focus: 1.0,
    external_drive: false,
};

/// End of the first night (spec §21: 1 day = 2,400 ticks).
pub const FIRST_NIGHT_END: u64 = 2400;

#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeResult {
    pub seed: u64,
    pub survived: bool,
    pub ticks: u64,
    pub death: Option<Death>,
    pub decisions: u32,
    pub reflexes: u32,
    pub health: f32,
    pub food: f32,
    pub new_things: usize,
}

/// Run one episode until `max_ticks` or death.
pub fn run_episode(
    rules: Rules,
    seed: u64,
    policy: &mut dyn Policy,
    max_ticks: u64,
) -> EpisodeResult {
    let mut w = SimWorld::new(rules, seed);
    let (mut decisions, mut reflexes, mut new_things) = (0, 0, 0);
    while w.tick < max_ticks && w.alive() {
        let obs = w.perceive();
        let cands = w.candidate_actions(&obs);
        let chosen = match emergency_reflex(&obs, &cands, ReflexThresholds::default()) {
            Some((_, c)) => {
                reflexes += 1;
                Some(c.clone())
            }
            None => policy.choose(&obs, &cands).map(|i| cands[i].clone()),
        };
        decisions += 1;
        match chosen {
            Some(c) => new_things += w.execute(&c, FULL).new_things.len(),
            None => w.step_world(1),
        }
    }
    EpisodeResult {
        seed,
        survived: w.alive(),
        ticks: w.tick,
        death: w.body.death,
        decisions,
        reflexes,
        health: w.body.health,
        food: w.body.food,
        new_things,
    }
}

/// First-night survival over seeds `0..n` (gate for build step 3).
pub fn first_night(n: u64, mut make: impl FnMut() -> Box<dyn Policy>) -> Vec<EpisodeResult> {
    (0..n)
        .map(|seed| run_episode(Rules::default(), seed, make().as_mut(), FIRST_NIGHT_END))
        .collect()
}
