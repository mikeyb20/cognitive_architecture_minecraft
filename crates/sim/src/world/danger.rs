//! Danger cuts an action short (sim audit 14 Nov 2026, fix 3): an action
//! begun with no hostile near stops the tick one comes within
//! `Rules::threat_range` of an unsheltered agent, so the agent can choose again
//! (flee, dig in) instead of taking free hits mid-chop. The Minecraft
//! adapter can abort a skill at any time (spec §22, survival preemption).
//! An action begun in danger runs on: the agent chose it with the hostile
//! in view.

use super::SimWorld;
use crate::geom::cheb;
use crate::mob::MobKind;

/// The mob lessons' tests (01 §4.11) where the agent stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MobTest {
    /// A zombie is within `Rules::threat_range` of the unsheltered agent.
    pub zombie_near: bool,
    /// A skeleton can shoot the unsheltered agent.
    pub shot_at: bool,
    /// The unsheltered agent's health is below `Rules::fight_health`.
    pub weak: bool,
}

impl SimWorld {
    /// The mob lessons' tests where the agent stands (01 §4.11).
    pub fn mob_test(&self) -> MobTest {
        if self.sheltered() {
            return MobTest::default();
        }
        let me = self.body.pos;
        MobTest {
            zombie_near: self
                .mobs
                .iter()
                .any(|m| m.kind == MobKind::Zombie && cheb(m.pos, me) <= self.rules.threat_range),
            shot_at: self.exposed_at(me),
            weak: self.body.health < self.rules.fight_health,
        }
    }

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
