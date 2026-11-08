//! The budget guard (spec §23, #40): at most N tool calls in any hour.
//! When it says no, the internal action runs its code tool instead and the
//! fallback is logged. Time is passed in, so the sim's clock and replays
//! give the same answers.

use std::collections::VecDeque;

const HOUR_S: f64 = 3600.0;

#[derive(Debug, Clone, Default)]
pub struct BudgetGuard {
    cap: Option<u32>,
    calls: VecDeque<f64>,
}

impl BudgetGuard {
    /// `cap` calls per sliding hour; `None` never refuses (T-local).
    pub fn new(cap: Option<u32>) -> Self {
        Self {
            cap,
            calls: VecDeque::new(),
        }
    }

    /// Whether a call may go out at `now_s` (seconds); if so, it is counted.
    pub fn try_spend(&mut self, now_s: f64) -> bool {
        while self.calls.front().is_some_and(|t| now_s - t >= HOUR_S) {
            self.calls.pop_front();
        }
        if let Some(cap) = self.cap
            && self.calls.len() >= cap as usize
        {
            return false;
        }
        self.calls.push_back(now_s);
        true
    }

    /// Calls counted in the hour before the last `try_spend`.
    pub fn used(&self) -> usize {
        self.calls.len()
    }
}
