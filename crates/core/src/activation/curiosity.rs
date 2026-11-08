//! Curiosity C (01 §1.1, stable) and how D dampens it (01 §1.2; decision #6).

use serde::{Deserialize, Serialize};

/// Personality parameters for C (01 §1.1, §4.10).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CuriosityParams {
    /// Resting curiosity C_base ∈ [0,1].
    pub base: f32,
    /// Drift rate toward C_target per tick, in (0,1].
    pub rate: f32,
}

impl Default for CuriosityParams {
    /// Spec §19 default archetype (proposed).
    fn default() -> Self {
        Self {
            base: 0.40,
            rate: 0.10,
        }
    }
}

/// The agent's single curiosity scalar (01 §1.1: one C per agent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curiosity {
    pub params: CuriosityParams,
    c: f32,
}

impl Curiosity {
    /// A new agent starts at its resting level.
    pub fn new(params: CuriosityParams) -> Self {
        let c = params.base.clamp(0.0, 1.0);
        Self { params, c }
    }

    /// Current C ∈ [0,1]. Never negative (01 §1.1; decision #5).
    pub fn value(&self) -> f32 {
        self.c
    }

    /// `C_target = clamp(C_base_eff + Σ_j k_j·signal_j, 0, 1)` (01 §1.1).
    /// `base_shift` carries urgency's `β_C·U` (01 §4.9; spec §15).
    pub fn target(&self, weighted_signals: f32, base_shift: f32) -> f32 {
        (self.params.base + base_shift + weighted_signals).clamp(0.0, 1.0)
    }

    /// One drift step toward `target` (01 §1.1):
    /// `ΔC = rate·(C_target − C)`; rises are scaled by `(1 − D)`; falls are not.
    /// Returns the applied ΔC.
    pub fn drift(&mut self, target: f32, d: f32) -> f32 {
        let delta = self.params.rate * (target.clamp(0.0, 1.0) - self.c);
        self.apply(delta, d)
    }

    /// A one-off upward impulse, such as a subconscious nudge (already scaled by π,
    /// 01 §4.5) or the stuck-kick boost κ (01 §4.3). Dampened by D like every rise
    /// (decision #6). Returns the applied ΔC.
    pub fn impulse(&mut self, amount: f32, d: f32) -> f32 {
        self.apply(amount, d)
    }

    fn apply(&mut self, delta: f32, d: f32) -> f32 {
        let delta = if delta > 0.0 {
            delta * (1.0 - d.clamp(0.0, 1.0))
        } else {
            delta
        };
        let before = self.c;
        self.c = (self.c + delta).clamp(0.0, 1.0);
        self.c - before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn agent(base: f32, rate: f32, c0: f32) -> Curiosity {
        let mut a = Curiosity::new(CuriosityParams { base, rate });
        a.c = c0;
        a
    }

    /// Spec §27 gate: with nothing happening, C settles back to C_base (01 §1.1).
    #[test]
    fn settles_to_base_with_no_signals() {
        for c0 in [0.0, 0.1, 0.5, 0.9, 1.0] {
            let mut a = agent(0.4, 0.1, c0);
            for _ in 0..500 {
                let t = a.target(0.0, 0.0);
                a.drift(t, 0.0);
            }
            assert!((a.value() - 0.4).abs() < 1e-4, "from {c0}: {}", a.value());
        }
    }

    #[test]
    fn suppressed_agent_still_settles_down_but_rises_slower() {
        // From above: falls are not dampened.
        let mut hi = agent(0.4, 0.1, 0.9);
        for _ in 0..500 {
            hi.drift(0.4, 0.9);
        }
        assert!((hi.value() - 0.4).abs() < 1e-4);
        // From below: D = 0.5 halves every rise.
        let mut free = agent(0.4, 0.1, 0.0);
        let mut damped = agent(0.4, 0.1, 0.0);
        free.drift(0.4, 0.0);
        damped.drift(0.4, 0.5);
        assert!((damped.value() - free.value() * 0.5).abs() < 1e-6);
    }

    proptest! {
        /// Spec §27 gate: every upward C impulse is × (1 − D) (decision #6).
        #[test]
        fn every_rise_is_scaled_by_one_minus_d(
            c0 in 0.0f32..0.5, amount in 0.0f32..0.5, d in 0.0f32..=1.0,
        ) {
            let mut a = agent(0.4, 0.1, c0);
            let applied = a.impulse(amount, d);
            prop_assert!((applied - amount * (1.0 - d)).abs() < 1e-6);
        }

        #[test]
        fn drift_rise_is_scaled_by_one_minus_d(
            c0 in 0.0f32..0.5, target in 0.5f32..=1.0, d in 0.0f32..=1.0,
        ) {
            let mut a = agent(0.4, 0.1, c0);
            let applied = a.drift(target, d);
            prop_assert!((applied - 0.1 * (target - c0) * (1.0 - d)).abs() < 1e-6);
        }

        /// 01 §1.1: C ∈ [0,1] always; never negative.
        #[test]
        fn c_stays_in_unit_interval(
            c0 in 0.0f32..=1.0, sig in -5.0f32..5.0, imp in -2.0f32..2.0, d in 0.0f32..=1.0,
        ) {
            let mut a = agent(0.4, 0.3, c0);
            let t = a.target(sig, 0.0);
            a.drift(t, d);
            a.impulse(imp, d);
            prop_assert!((0.0..=1.0).contains(&a.value()));
        }

        /// 01 §1.2: D dampens, it does not cap. Even at D = 0.9, C can still reach
        /// high values, just more slowly.
        #[test]
        fn suppression_does_not_cap(d in 0.0f32..0.95) {
            let mut a = agent(0.4, 0.2, 0.0);
            for _ in 0..20_000 {
                a.drift(1.0, d);
            }
            prop_assert!(a.value() > 0.99);
        }
    }

    #[test]
    fn defaults_and_target_clamp() {
        let a = Curiosity::new(CuriosityParams::default());
        assert_eq!(a.value(), 0.40);
        assert_eq!(a.target(5.0, 0.0), 1.0);
        assert_eq!(a.target(-5.0, 0.0), 0.0);
        assert!(
            (a.target(0.0, 0.1) - 0.5).abs() < 1e-6,
            "urgency raises C_base"
        );
    }
}
