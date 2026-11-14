//! Learned control κ (01 §1.2, #64): the ΔP contingency between the agent's
//! own escape attempts and relief from pain (Maier & Seligman 2016: control
//! is what is learned; the default assumes none).

use serde::{Deserialize, Serialize};

/// κ and how it acts on D (01 §1.2, #64). Off by default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ControlParams {
    /// κ on: pain raises D, κ buffers every D rise and speeds recovery, and
    /// D damps the threat-forced Flee push.
    pub enabled: bool,
    /// Rate of the two relief estimates (proposed).
    pub rate: f32,
    /// Pain as a D drive: `min(1, pain_gain · damage)` (proposed: 0.05
    /// damage over the V window reads as full pain).
    pub pain_gain: f32,
    /// Extra D recovery per unit κ (proposed: 10× the base recovery).
    pub recover: f32,
}

impl Default for ControlParams {
    /// #64: off by default until fleeing mobs works in the sim.
    fn default() -> Self {
        Self {
            enabled: false,
            rate: 0.2,
            pain_gain: 20.0,
            recover: 0.005,
        }
    }
}

/// The two relief rates behind κ, both starting at 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Control {
    relief_act: f32,
    relief_other: f32,
}

impl Control {
    /// κ = clamp(relief after escape attempts − relief after anything
    /// else, 0, 1) (01 §1.2, #64).
    pub fn kappa(&self) -> f32 {
        (self.relief_act - self.relief_other).clamp(0.0, 1.0)
    }

    /// One action taken in pain: was it an escape attempt, and did the pain
    /// ease by the next observation (01 §1.2, #64).
    pub fn observe(&mut self, rate: f32, escaped: bool, relief: bool) {
        let r = f32::from(u8::from(relief));
        let e = if escaped {
            &mut self.relief_act
        } else {
            &mut self.relief_other
        };
        *e += rate * (r - *e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn kappa_by_hand() {
        let mut c = Control::default();
        assert_eq!(c.kappa(), 0.0, "no control until learned");
        c.observe(0.2, true, true);
        assert!((c.kappa() - 0.2).abs() < 1e-6);
        c.observe(0.2, true, true);
        assert!((c.kappa() - 0.36).abs() < 1e-6);
        // Relief that comes anyway takes control away: 0.36 − 0.5 < 0.
        c.observe(0.5, false, true);
        assert_eq!(c.kappa(), 0.0);
        // Pain that stays without escaping restores it: 0.36 − 0.25.
        c.observe(0.5, false, false);
        assert!((c.kappa() - 0.11).abs() < 1e-6);
        // A failed escape lowers it: 0.18 − 0.25 < 0.
        c.observe(0.5, true, false);
        assert_eq!(c.kappa(), 0.0);
    }

    #[test]
    fn defaults_are_off() {
        let p = ControlParams::default();
        assert!(!p.enabled);
        assert_eq!((p.rate, p.pain_gain, p.recover), (0.2, 20.0, 0.005));
    }

    proptest! {
        #[test]
        fn kappa_stays_in_unit_interval(
            steps in proptest::collection::vec((any::<bool>(), any::<bool>()), 0..50),
            rate in 0.0f32..=1.0,
        ) {
            let mut c = Control::default();
            for (e, r) in steps {
                c.observe(rate, e, r);
                prop_assert!((0.0..=1.0).contains(&c.kappa()));
            }
        }
    }
}
