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
use crate::recovery::Setbacks;

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
        start_tick: world.tick,
        ..BrainResult::default()
    };
    let mut setbacks = Setbacks::default();
    while world.tick < end && world.alive() {
        if world.tick < harass_until {
            scenario::harass(&mut world);
            harsh_open = true;
        } else if harsh_open {
            scenario::end_harsh(&mut world);
            harsh_open = false;
        }
        let test = milestones(&mut r, &world);
        setbacks.health(world.body.health, world.tick);
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
        d.see_tests(test);
        match d.decide(&obs, &cands) {
            Decision::Act(i, it) => {
                setbacks.acted(cands[i].skill, obs.tick);
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
    milestones(&mut r, &world);
    setbacks.health(world.body.health, world.tick);
    r.unrecovered = setbacks.unrecovered();
    r.recoveries = setbacks.recoveries;
    r.ticks = world.tick;
    r.death = world.body.death;
    r.inventory = world.body.inv.items.clone();
    r
}

/// Note the first tick of each milestone the plan §5.0 scores read: a
/// passing shelter test (01 §4.11) while alive, and each item first held.
/// Returns the shelter test for the driver.
fn milestones(r: &mut BrainResult, world: &sim::SimWorld) -> ShelterTest {
    let test = world.shelter_test();
    if world.alive() && test.passes() {
        r.shelter_at.get_or_insert(world.tick);
    }
    for (item, n) in &world.body.inv.items {
        if *n > 0 && !r.firsts.iter().any(|(i, _)| i == item) {
            r.firsts.push((item.clone(), world.tick));
        }
    }
    test
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_api::Pos;
    use sim::Death;
    use sim::map::Tile;

    // Unit tests, so a mutant here fails before the slow CLI tests run.
    #[test]
    fn milestones_note_a_live_pass_and_each_held_item_once() {
        let mut w = crate::brain::setup("first_night", 0).world;
        let mut r = BrainResult::default();
        w.body.inv.items = vec![("oak_log".into(), 2), ("stick".into(), 0)];
        assert!(!milestones(&mut r, &w).passes(), "open ground");
        assert_eq!(r.shelter_at, None);
        w.tick = 7;
        milestones(&mut r, &w);
        assert_eq!(
            r.firsts,
            [("oak_log".to_string(), 0)],
            "once; a zero is not held"
        );
        // A stone ring with a door around the agent passes.
        let me = w.body.pos;
        for dx in -2i32..=2 {
            for dz in -2i32..=2 {
                let p = Pos {
                    x: me.x + dx,
                    y: 0,
                    z: me.z + dz,
                };
                let ring = dx.abs().max(dz.abs()) == 2;
                w.map.set(p, if ring { Tile::Stone } else { Tile::Grass });
            }
        }
        w.map.set(Pos { x: me.x + 2, ..me }, Tile::Door);
        w.mobs.clear();
        let mut dead = w.clone();
        dead.body.death = Some(Death::Mob);
        let mut d = BrainResult::default();
        assert!(milestones(&mut d, &dead).passes());
        assert_eq!(d.shelter_at, None, "only while alive");
        milestones(&mut r, &w);
        assert_eq!(r.shelter_at, Some(7));
    }

    #[test]
    fn a_run_names_its_scenario_seed_and_start() {
        let mut s = crate::brain::setup("harsh", 3);
        s.max_ticks = 0;
        let mut b = cognitive_core::agent::Brain::new(Default::default(), 3);
        let r = crate::brain::run_with(s, &mut b, 3, &mut |_| {});
        assert_eq!((r.scenario, r.seed), ("harsh", 3));
        assert_eq!(r.start_tick, sim::scenario::DAY / 2);
    }
}
