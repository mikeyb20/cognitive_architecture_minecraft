//! Tenacity T[mode, task type], 01 §4.4 and spec §10.

use serde::{Deserialize, Serialize};

/// `T = (succ + a)/(att + a + b)` with fading counts (01 §4.4).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tenacity {
    pub a: f32,
    pub b: f32,
    /// Fade factor γ ∈ (0, 1].
    pub gamma: f32,
    succ: f32,
    att: f32,
}

impl Tenacity {
    pub fn new(a: f32, b: f32, gamma: f32) -> Self {
        Self {
            a,
            b,
            gamma,
            succ: 0.0,
            att: 0.0,
        }
    }

    /// T ∈ [0, 1].
    pub fn value(&self) -> f32 {
        (self.succ + self.a) / (self.att + self.a + self.b)
    }

    /// Record one episode in which the agent persisted through a setback
    /// (spec §10). `success` is 1 if persisting paid off, else 0.
    pub fn record(&mut self, success: f32) {
        self.succ = self.succ * self.gamma + success.clamp(0.0, 1.0);
        self.att = self.att * self.gamma + 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Spec §27 gate: T starts at a/(a+b).
    #[test]
    fn starts_at_prior() {
        assert_eq!(Tenacity::new(1.0, 1.0, 0.98).value(), 0.5);
        assert_eq!(Tenacity::new(3.0, 1.0, 0.99).value(), 0.75);
    }

    #[test]
    fn can_be_rebuilt_after_fading() {
        // 00 §2.7: because old results fade, tenacity can be rebuilt.
        let mut t = Tenacity::new(1.0, 1.0, 0.9);
        for _ in 0..30 {
            t.record(0.0);
        }
        let low = t.value();
        for _ in 0..30 {
            t.record(1.0);
        }
        assert!(low < 0.2 && t.value() > 0.8, "{low} -> {}", t.value());
    }

    proptest! {
        #[test]
        fn stays_in_unit_interval(
            outcomes in proptest::collection::vec(0.0f32..=1.0, 0..200),
            a in 0.1f32..5.0, b in 0.1f32..5.0, g in 0.5f32..=1.0,
        ) {
            let mut t = Tenacity::new(a, b, g);
            for o in outcomes { t.record(o); }
            prop_assert!((0.0..=1.0).contains(&t.value()));
        }
    }
}
