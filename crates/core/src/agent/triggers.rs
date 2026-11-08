//! Fixed-trigger thinking, an ablation (plan §5.2; spec §24 C−X; #40).
//! By default thinking is chosen like any other action (spec §6). With this
//! switched on, plan, reflect and imagine leave the choice and run on set
//! events instead, as in v0.1: a new goal, and every `every` ticks.
//! v0.1's third event, a death, is not here: neither world reports a death
//! to the brain (a sim run ends at death).

use super::Brain;
use super::think::ThinkTools;
use crate::memory::InternalAction;
use host_api::Observation;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FixedThinking {
    /// Off (the default): thinking is chosen (#40).
    pub on: bool,
    /// Reflect, imagine and re-plan every this many ticks (proposed: one
    /// Minecraft minute, 1200 game ticks, is 120 sim ticks).
    pub every: u64,
    /// Effort the triggered thoughts run at, which sets their depth
    /// (spec §16; proposed).
    pub effort: f32,
}

impl Default for FixedThinking {
    fn default() -> Self {
        Self {
            on: false,
            every: 120,
            effort: 0.5,
        }
    }
}

impl FixedThinking {
    /// Whether `a` runs on events rather than by choice.
    pub fn triggered(&self, a: InternalAction) -> bool {
        self.on
            && matches!(
                a,
                InternalAction::Plan | InternalAction::Reflect | InternalAction::Imagine
            )
    }
}

/// When the triggered thoughts last ran.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct TriggerState {
    /// The timer fires at or after this tick.
    next: u64,
    /// Goals set so far, and the count when the last plan ran.
    goals: u64,
    planned: u64,
}

impl TriggerState {
    /// A goal was set (a command or the agent's own).
    pub(crate) fn goal_set(&mut self) {
        self.goals += 1;
    }
}

impl Brain {
    /// Run the thoughts whose events fired this tick: on the timer reflect
    /// and imagine, then plan if a goal is new or the timer fired. Returns
    /// how many ran.
    pub(crate) fn run_triggers(&mut self, obs: &Observation, tools: &mut dyn ThinkTools) -> u32 {
        let f = self.config.fixed_thinking;
        if !f.on {
            return 0;
        }
        let mut ran = 0;
        let timer = obs.tick >= self.triggers.next;
        if timer {
            self.triggers.next = obs.tick + f.every;
            for a in [InternalAction::Reflect, InternalAction::Imagine] {
                self.think(a, f.effort, obs, tools);
                ran += 1;
            }
        }
        let new_goal = self.triggers.goals != self.triggers.planned;
        if self.goal.is_some() && (timer || new_goal) {
            self.triggers.planned = self.triggers.goals;
            self.think(InternalAction::Plan, f.effort, obs, tools);
            ran += 1;
        }
        ran
    }
}
