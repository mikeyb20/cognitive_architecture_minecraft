//! Unit tests for the brain loop's internals, split by responsibility:
//! learning writes (spec §14, §7.1) and the reflex lesson (spec §20) in
//! `learning`, the choice helpers and log tags (spec §8, §25) in `choice`,
//! the internal actions (spec §6) in `thinking`, and recipes kept in
//! memory (01 §4.11, §5) in `teaching`.

use super::choose::{HAZARD, Scored, dominant, hazard_vetoed, leads, preferences};
use super::learn::EPISODES_KEPT;
use super::log::{DAY_TICKS, action_tags, ticks_to_night};
use super::think::{NoTools, ThinkTools, ThoughtResult};
use super::*;
use crate::memory::InternalAction;
use crate::modes::feature as f;
use crate::modes::success::PARTIAL;
use crate::modes::{ModeNetwork, N_FEATURES};
use host_api::{Candidate, Skill, Target, TaskType, ThingId};

mod choice;
mod learning;
mod teaching;
mod thinking;

pub(super) fn pending(choice: Choice) -> Pending {
    Pending {
        tick: 0,
        choice,
        phi: [0.0; N_FEATURES],
        m: [0.0; 4],
        w: [0.25; 4],
        x: [0.0; crate::control::oracle::CONTEXT_DIM],
        task: TaskType::Mining,
        target: None,
        route: None,
        skill: None,
        skill_label: "test".into(),
        progress_before: 0.0,
        hazard_vetoed: false,
        tags: 0,
        internal: None,
        outcome: None,
        food_before: 0.0,
    }
}

pub(super) fn external(out: Outcome) -> Pending {
    Pending {
        outcome: Some(out),
        ..pending(Choice::External(0))
    }
}

pub(super) fn learn_at(b: &mut Brain, p: Pending, tick: u64) {
    b.pending = Some(p);
    let mut obs = test_obs();
    obs.tick = tick;
    b.learn(&obs, &mut NoTools);
}

pub(super) fn brain() -> Brain {
    Brain::new(BrainConfig::default(), 0)
}
