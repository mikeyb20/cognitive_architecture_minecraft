//! Danger cuts an action short (sim audit 14 Nov 2026, fix 3): an action
//! begun with no hostile near stops the tick one comes within
//! `THREAT_RANGE` of an unsheltered agent, so the agent can choose again
//! (flee, dig in) instead of taking free hits mid-chop. The Minecraft
//! adapter can abort a skill at any time (spec §22, survival preemption).
//! An action begun in danger runs on: the agent chose it with the hostile
//! in view.

use super::SimWorld;
use crate::geom::cheb;
use crate::skills::THREAT_RANGE;

impl SimWorld {
    /// Whether a hostile is within `THREAT_RANGE` of the unsheltered agent.
    pub fn in_danger(&self) -> bool {
        !self.sheltered()
            && self
                .mobs
                .iter()
                .any(|m| m.kind.hostile() && cheb(m.pos, self.body.pos) <= THREAT_RANGE)
    }

    /// After each tick: interrupt an action begun out of danger once danger
    /// arrives.
    pub(super) fn check_danger(&mut self) {
        if self.calm_start && self.in_danger() {
            self.interrupted = true;
        }
    }
}
