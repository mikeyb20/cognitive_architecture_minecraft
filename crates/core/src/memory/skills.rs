//! Procedural competence: success estimate p̂ and attempt count n per
//! (action, task type), and recent failures (spec §3 f3, f4, f11).

use host_api::{Skill, TaskType};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// Internal actions (spec §6; 00 §2.2: each mode can act inward).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InternalAction {
    /// Learning inward: turn recent episodes into semantic items.
    Reflect,
    /// Exploring inward: recombine two memories into an idea or a goal.
    Imagine,
    /// Performing inward: mental practice, lowers π for the task.
    Rehearse,
    /// Optimizing inward: foreground prune and reinforce.
    Consolidate,
    /// Composite O + E: make steps for the active goal.
    Plan,
}

impl InternalAction {
    pub const ALL: [InternalAction; 5] = [
        InternalAction::Reflect,
        InternalAction::Imagine,
        InternalAction::Rehearse,
        InternalAction::Consolidate,
        InternalAction::Plan,
    ];
}

/// Any action the brain can take: an external skill or an internal action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKey {
    External(SkillKey),
    Internal(InternalAction),
}

/// `Skill` with an order, so it can key a `BTreeMap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SkillKey(pub u8);

impl From<Skill> for ActionKey {
    fn from(s: Skill) -> Self {
        ActionKey::External(SkillKey(s as u8))
    }
}

impl From<InternalAction> for ActionKey {
    fn from(a: InternalAction) -> Self {
        ActionKey::Internal(a)
    }
}

/// Window for recent failures, in ticks, and the count that saturates f11
/// (spec §3: failures in the last 50 ticks ÷ 5).
pub const FAIL_WINDOW: u64 = 50;
pub const FAIL_SATURATE: f32 = 5.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
struct Stat {
    succ: f32,
    n: u32,
}

impl Stat {
    /// Beta(1, 1) posterior mean.
    fn p_hat(self) -> f32 {
        (self.succ + 1.0) / (self.n as f32 + 2.0)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillTable {
    stats: BTreeMap<(ActionKey, TaskType), Stat>,
    per_task: BTreeMap<TaskType, Stat>,
    fails: VecDeque<(u64, ActionKey, TaskType)>,
}

impl SkillTable {
    /// p̂(s, t) with a Beta prior a = b = 1 (spec §3 f4).
    pub fn p_hat(&self, a: ActionKey, t: TaskType) -> f32 {
        self.stats.get(&(a, t)).copied().unwrap_or_default().p_hat()
    }

    /// n(s, t).
    pub fn attempts(&self, a: ActionKey, t: TaskType) -> u32 {
        self.stats.get(&(a, t)).map_or(0, |s| s.n)
    }

    /// Task-level competence (Oracle context, spec §8).
    pub fn task_p_hat(&self, t: TaskType) -> f32 {
        self.per_task.get(&t).copied().unwrap_or_default().p_hat()
    }

    /// f3 uncertainty `1/√(n + 1)`.
    pub fn uncertainty(&self, a: ActionKey, t: TaskType) -> f32 {
        1.0 / (self.attempts(a, t) as f32 + 1.0).sqrt()
    }

    /// f4 competence `2·p̂ − 1`.
    pub fn competence(&self, a: ActionKey, t: TaskType) -> f32 {
        2.0 * self.p_hat(a, t) - 1.0
    }

    /// f11 failures of (s, t) in the last 50 ticks ÷ 5, clamped to 1.
    pub fn recent_fail(&self, a: ActionKey, t: TaskType, now: u64) -> f32 {
        let n = self
            .fails
            .iter()
            .filter(|(tick, k, tt)| *k == a && *tt == t && now.saturating_sub(*tick) < FAIL_WINDOW)
            .count();
        (n as f32 / FAIL_SATURATE).min(1.0)
    }

    /// Record one attempt. Returns the prediction error `|success − p̂|`
    /// measured before the update (input to learning progress, spec §5).
    pub fn record(&mut self, a: ActionKey, t: TaskType, success: bool, now: u64) -> f32 {
        let y = if success { 1.0 } else { 0.0 };
        let err = (y - self.p_hat(a, t)).abs();
        for s in [
            self.stats.entry((a, t)).or_default(),
            self.per_task.entry(t).or_default(),
        ] {
            s.succ += y;
            s.n += 1;
        }
        if !success {
            self.fails.push_back((now, a, t));
        }
        while self
            .fails
            .front()
            .is_some_and(|(tick, _, _)| now.saturating_sub(*tick) >= FAIL_WINDOW)
        {
            self.fails.pop_front();
        }
        err
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHOP: TaskType = TaskType::Mining;

    #[test]
    fn prior_is_beta_one_one() {
        let t = SkillTable::default();
        let a = ActionKey::from(Skill::Chop);
        assert_eq!(t.p_hat(a, CHOP), 0.5);
        assert_eq!(t.competence(a, CHOP), 0.0);
        assert_eq!(t.uncertainty(a, CHOP), 1.0);
        assert_eq!(t.task_p_hat(CHOP), 0.5);
    }

    #[test]
    fn record_updates_estimates_and_returns_the_error() {
        let mut t = SkillTable::default();
        let a = ActionKey::from(Skill::Chop);
        assert_eq!(t.record(a, CHOP, true, 0), 0.5);
        assert!((t.p_hat(a, CHOP) - 2.0 / 3.0).abs() < 1e-6);
        assert!((t.record(a, CHOP, false, 1) - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(t.attempts(a, CHOP), 2);
        assert_eq!(t.p_hat(a, CHOP), 0.5);
        assert!((t.uncertainty(a, CHOP) - 1.0 / 3f32.sqrt()).abs() < 1e-6);
        assert_eq!(t.task_p_hat(CHOP), 0.5);
        let other = ActionKey::from(InternalAction::Plan);
        assert_eq!(t.attempts(other, CHOP), 0, "keys are separate");
        assert_eq!(t.attempts(a, TaskType::Travel), 0, "tasks are separate");
    }

    #[test]
    fn recent_fail_counts_the_last_50_ticks_and_saturates_at_5() {
        let mut t = SkillTable::default();
        let a = ActionKey::from(Skill::Mine);
        for tick in 0..7 {
            t.record(a, CHOP, false, tick);
        }
        t.record(a, CHOP, true, 7);
        assert_eq!(t.recent_fail(a, CHOP, 7), 1.0);
        assert_eq!(t.recent_fail(a, TaskType::Travel, 7), 0.0);
        assert!(
            (t.recent_fail(a, CHOP, 53) - 0.6).abs() < 1e-6,
            "ticks 4–6 left"
        );
        assert_eq!(t.recent_fail(a, CHOP, 56), 0.0);
        t.record(a, CHOP, false, 100);
        assert_eq!(t.recent_fail(a, CHOP, 100), 0.2, "old failures dropped");
        assert_eq!(t.fails.len(), 1);
    }

    #[test]
    fn keys_order_and_convert() {
        assert_eq!(
            ActionKey::from(Skill::Mine),
            ActionKey::External(SkillKey(Skill::Mine as u8))
        );
        assert!(ActionKey::from(Skill::Chat) < ActionKey::from(InternalAction::Reflect));
        assert_eq!(InternalAction::ALL.len(), 5);
    }
}
