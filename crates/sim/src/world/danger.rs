//! Danger cuts an action short (sim audit 14 Nov 2026, fix 3): an action
//! begun with no hostile near stops the tick one comes within
//! `Rules::threat_range` of an unsheltered agent, so the agent can choose again
//! (flee, dig in) instead of taking free hits mid-chop. The Minecraft
//! adapter can abort a skill at any time (spec §22, survival preemption).
//! An action begun in danger runs on: the agent chose it with the hostile
//! in view.

use super::SimWorld;

impl SimWorld {
    /// Whether a hostile threatens the unsheltered agent: within
    /// `Rules::threat_range`, or a skeleton that can shoot it (`ranged`).
    pub fn in_danger(&self) -> bool {
        !self.sheltered() && self.mobs.iter().any(|m| self.threatens(m))
    }

    /// After each tick: interrupt an action begun out of danger once danger
    /// arrives.
    pub(super) fn check_danger(&mut self) {
        if self.calm_start && self.in_danger() {
            self.interrupted = true;
        }
    }
}
