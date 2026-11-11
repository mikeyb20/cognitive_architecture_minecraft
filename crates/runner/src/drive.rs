//! One scenario run (spec §21, §26): the world, the scripted player, the
//! harsh episode and the shared emergency reflexes (#42), driven by any
//! arm that implements [`Driver`].

use cognitive_core::agent::{Decision, TickLog};
use host_api::reflex::{ReflexThresholds, emergency_reflex};
use host_api::{Candidate, Host, Observation, Outcome, World};
use sim::scenario::{self, Setup};
use sim::world::ShelterTest;

use crate::FULL;
use crate::brain::BrainResult;

/// What picks the agent's actions in a scenario run.
pub(crate) trait Driver {
    fn decide(&mut self, obs: &Observation, cands: &[Candidate]) -> Decision;
    /// The result of the last `Act`.
    fn outcome(&mut self, out: Outcome);
    /// An emergency reflex acted for the agent at `tick`.
    fn reflex_fired(&mut self, tick: u64);
    /// The log row of the last decision, if the driver keeps one.
    fn log(&self) -> Option<&TickLog>;
    /// The host's goal tests where the agent stands (01 §4.11), before each
    /// decision. Drivers without a teacher ignore them.
    fn see_tests(&mut self, _shelter: ShelterTest) {}
}

/// Run `setup` with `d` until its tick limit (or `lifespan`, if sooner) or
/// death.
pub(crate) fn run_scenario(
    setup: Setup,
    d: &mut dyn Driver,
    lifespan: Option<u64>,
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
    let end = lifespan.map_or(max_ticks, |l| l.min(max_ticks));
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
            d.reflex_fired(obs.tick);
            r.new_things += world.execute(&c, FULL).new_things.len();
            continue;
        }
        r.decisions += 1;
        d.see_tests(world.shelter_test());
        match d.decide(&obs, &cands) {
            Decision::Act(i, it) => {
                let out = world.execute(&cands[i], it);
                r.new_things += out.new_things.len();
                d.outcome(out);
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
        if let Some(row) = d.log() {
            on_row(row);
        }
    }
    r.survived = world.alive();
    r.ticks = world.tick;
    r.death = world.body.death;
    r.inventory = world.body.inv.items.clone();
    r
}
