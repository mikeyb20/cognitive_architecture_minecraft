//! Plan §5.0 recovery, for every arm: after a setback (the agent took
//! damage), the ticks until it starts normal work again. Read from the
//! host's health and the actions the arm chose, so arms without D or V are
//! measured the same way (my reading of "time back to normal activity";
//! the brain's own D curve is in its log rows).

use host_api::Skill;

/// Skills that cope with a setback rather than resume work: running,
/// hiding, resting and eating. Any other chosen action counts as normal
/// work (plan §5.0 "activity returns").
pub const COPING: [Skill; 4] = [Skill::Flee, Skill::Shelter, Skill::Rest, Skill::Eat];

/// Setbacks seen in one run and how long each took to recover from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Setbacks {
    /// Ticks from the last damage of each recovered setback to the next
    /// work action.
    pub recoveries: Vec<u64>,
    /// The tick of the latest damage not yet recovered from.
    pub open: Option<u64>,
    last_health: Option<f32>,
}

impl Setbacks {
    /// The agent's health at `tick`; any drop opens (or extends) a setback.
    pub fn health(&mut self, health: f32, tick: u64) {
        if self.last_health.is_some_and(|h| health < h) {
            self.open = Some(tick);
        }
        self.last_health = Some(health);
    }

    /// The arm chose `skill` at `tick`; normal work closes an open setback.
    pub fn acted(&mut self, skill: Skill, tick: u64) {
        if COPING.contains(&skill) {
            return;
        }
        if let Some(hurt) = self.open.take() {
            self.recoveries.push(tick.saturating_sub(hurt));
        }
    }

    /// Setbacks the run ended inside (0 or 1).
    pub fn unrecovered(&self) -> u32 {
        u32::from(self.open.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::BrainResult;
    use crate::metrics::{RunMetrics, recovery, summarise};

    // Unit tests, so a mutant here fails before the slow CLI tests run
    // (tests/recovery.rs has the fuller cases).
    #[test]
    fn a_drop_opens_a_setback_and_work_closes_it() {
        let mut s = Setbacks::default();
        s.health(1.0, 0);
        s.health(0.5, 10);
        s.acted(Skill::Flee, 11);
        assert_eq!((s.open, s.unrecovered()), (Some(10), 1));
        s.acted(Skill::Chop, 25);
        assert_eq!((s.recoveries.clone(), s.unrecovered()), (vec![15], 0));
        s.health(0.5, 30);
        assert_eq!(s.open, None);
    }

    #[test]
    fn recovery_metrics_and_summary() {
        let r = BrainResult {
            recoveries: vec![30, 20],
            unrecovered: 1,
            ..BrainResult::default()
        };
        let m = recovery(&r);
        assert_eq!((m.recovery, m.setbacks, m.recovered), (25.0, 3, 2));
        let s = summarise(&[
            m,
            RunMetrics {
                recovery: 10.0,
                setbacks: 2,
                recovered: 2,
                ..RunMetrics::default()
            },
        ]);
        assert_eq!((s.recovery, s.recovered), (17.5, 0.8));
    }
}
