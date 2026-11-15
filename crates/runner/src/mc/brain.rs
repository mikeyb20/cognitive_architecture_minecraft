//! Arm C in Minecraft (spec §22, §26): the full brain drives the bot. Same
//! order as the sim driver: perceive → candidates → teacher's dusk call →
//! emergency reflex (#42) or the brain's decision → execute on the bot,
//! outcome back to the brain.

use cognitive_core::agent::{Brain, Decision};
use host_api::Host;
use host_api::reflex::{ReflexThresholds, emergency_reflex};
use sim::player::ScriptedPlayer;

use super::{McHost, McStep};
use crate::FULL;
use crate::bed_lesson::BedCall;
use crate::brain::SimTools;
use crate::teachability::teach_until;

/// What one Minecraft run of the brain did, for the CLI summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrainCounts {
    pub acts: u32,
    pub thoughts: u32,
    pub idle: u32,
    pub reflexes: u32,
    /// Dusk calls the teacher made ("get shelter", 01 §4.11).
    pub dusk_calls: u32,
    /// Afternoon calls for a bed ("get bed", `bed_lesson`).
    pub bed_calls: u32,
}

impl BrainCounts {
    pub fn decisions(&self) -> u32 {
        self.acts + self.thoughts + self.idle + self.reflexes
    }
}

/// How a brain run goes (spec §22).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrainRun {
    /// Stop after this many decisions, or when the link fails.
    pub decisions: u32,
    pub thresholds: ReflexThresholds,
    /// The shelter teacher (01 §4.11, #59): once a day from dusk, unless
    /// the bot is sheltered, the player says "get shelter", and the sim's
    /// code tools show the shelter recipe and explain the test with the
    /// bot's own sheltered flag. As in the sim's dusk-call scenarios.
    /// Also the afternoon bed call and its lesson (`bed_lesson`).
    pub teacher: bool,
    /// Days the teacher shows its recipes (shelter, and the bed: `bed_lesson`),
    /// before they fade to the test alone (plan §5.0 teachability); `None`
    /// shows them for the whole run.
    pub teach_days: Option<u64>,
}

/// Run `brain` on the bot (spec §22). The sim's code tools stand in for
/// the LLM (spec §21). `wait` runs after a thought or idle tick, so the
/// game moves on; the CLI passes a one-tick sleep. Calls `log` after each
/// decision.
pub fn run_brain(
    host: &mut McHost,
    brain: &mut Brain,
    run: BrainRun,
    mut wait: impl FnMut(),
    mut log: impl FnMut(&McStep),
) -> BrainCounts {
    let day_ticks = host.params().day_ticks;
    let mut tools = SimTools {
        bed: run.teacher,
        teach_until: teach_until(run.teach_days, day_ticks),
        ..SimTools::default()
    };
    let mut player = ScriptedPlayer::idle();
    player.dusk_call = run.teacher;
    let mut bed_call = BedCall::default();
    let mut n = BrainCounts::default();
    for _ in 0..run.decisions {
        if host.error.is_some() {
            break;
        }
        let mut obs = host.perceive();
        let cands = host.candidate_actions(&obs);
        let sheltered = obs.sheltered;
        tools.shelter = Some(sheltered);
        n.dusk_calls += u32::from(player.call_at_dusk(&mut obs, day_ticks, sheltered));
        if run.teacher {
            n.bed_calls += u32::from(bed_call.call(&mut obs, day_ticks));
        }
        let (chosen, it, reflex) = match emergency_reflex(&obs, &cands, run.thresholds) {
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
            goal: brain.goal().map(|g| g.item.clone()),
            recipe_step: brain.last_log().recipe_step.clone(),
        });
    }
    n
}
