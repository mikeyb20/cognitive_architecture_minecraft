//! Valence V (01 §1.3) and suppression D (01 §1.2) updates (spec §12; OQ17).
//! All coefficients are proposals that OQ17 sweeps.

use super::control::{Control, ControlParams};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AffectParams {
    /// Drift rate of V toward V_target.
    pub v_rate: f32,
    /// V below this raises D.
    pub d_threshold: f32,
    /// D rise rate: `D += d_rise·|V|·(1 − D)`.
    pub d_rise: f32,
    /// D recovery rate: `D −= d_recover·D`.
    pub d_recover: f32,
    /// Learned control κ (01 §1.2, #64; off by default).
    #[serde(default)]
    pub control: ControlParams,
}

impl Default for AffectParams {
    /// Spec §12 (proposed): D rises about 20× faster than it recovers.
    fn default() -> Self {
        Self {
            v_rate: 0.2,
            d_threshold: -0.3,
            d_rise: 0.01,
            d_recover: 0.0005,
            control: ControlParams::default(),
        }
    }
}

/// Inputs to V_target (spec §12).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ValenceInputs {
    /// Mean R over the last 20 ticks.
    pub r_mean_20: f32,
    pub threat: f32,
    pub damage: f32,
    /// Mean V_enc of the items in thought momentum.
    pub tm_valence: f32,
}

/// Global V ∈ [−1,1] and D ∈ [0,1].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Affect {
    pub params: AffectParams,
    v: f32,
    d: f32,
    #[serde(default)]
    control: Control,
}

impl Affect {
    pub fn new(params: AffectParams) -> Self {
        Self {
            params,
            v: 0.0,
            d: 0.0,
            control: Control::default(),
        }
    }

    pub fn v(&self) -> f32 {
        self.v
    }

    pub fn d(&self) -> f32 {
        self.d
    }

    /// Learned control κ, 0 while control is off (01 §1.2, #64).
    pub fn kappa(&self) -> f32 {
        if self.params.control.enabled {
            self.control.kappa()
        } else {
            0.0
        }
    }

    /// Record an action taken in pain and whether the pain eased by the
    /// next observation (01 §1.2, #64).
    pub fn observe_control(&mut self, escaped: bool, relief: bool) {
        self.control
            .observe(self.params.control.rate, escaped, relief);
    }

    /// The push behind a threat-forced Flee: 1, or 1 − D with control on
    /// (01 §1.2, #64; spec §16 threat drive).
    pub fn flee_push(&self) -> f32 {
        if self.params.control.enabled {
            1.0 - self.d
        } else {
            1.0
        }
    }

    /// `V_target = clamp(0.6·R̄_20 − 0.3·threat − 0.5·damage + 0.2·mean(V_enc over TM), −1, 1)`
    /// Set V and D directly (scenarios that reproduce 01 §1.5 states).
    pub fn force(&mut self, v: f32, d: f32) {
        self.v = v.clamp(-1.0, 1.0);
        self.d = d.clamp(0.0, 1.0);
    }

    pub fn v_target(x: ValenceInputs) -> f32 {
        (0.6 * x.r_mean_20 - 0.3 * x.threat - 0.5 * x.damage + 0.2 * x.tm_valence).clamp(-1.0, 1.0)
    }

    /// One tick: V drifts toward its target, then D rises under strongly
    /// negative V (or, with control on, pain), scaled by 1 − κ, and
    /// otherwise recovers very slowly, faster with κ (01 §1.2, #64).
    pub fn step(&mut self, x: ValenceInputs) {
        let p = &self.params;
        self.v = (self.v + p.v_rate * (Self::v_target(x) - self.v)).clamp(-1.0, 1.0);
        let low_v = if self.v < p.d_threshold {
            self.v.abs()
        } else {
            0.0
        };
        let c = &p.control;
        let pain = if c.enabled {
            (c.pain_gain * x.damage).min(1.0)
        } else {
            0.0
        };
        let drive = low_v.max(pain);
        let kappa = self.kappa();
        if drive > 0.0 {
            self.d += p.d_rise * drive * (1.0 - self.d) * (1.0 - kappa);
        } else {
            self.d -= (p.d_recover + c.recover * kappa) * self.d;
        }
        self.d = self.d.clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn harsh_episode_raises_d_and_recovery_is_slower() {
        let mut a = Affect::new(AffectParams::default());
        let harsh = ValenceInputs {
            r_mean_20: -1.0,
            threat: 1.0,
            damage: 1.0,
            tm_valence: -1.0,
        };
        for _ in 0..200 {
            a.step(harsh);
        }
        let d_peak = a.d();
        assert!(d_peak > 0.5, "D after harsh episode: {d_peak}");
        for _ in 0..200 {
            a.step(ValenceInputs::default());
        }
        assert!(a.d() < d_peak);
        assert!(d_peak - a.d() < d_peak * 0.2, "recovery should be slow");
    }

    fn with_control() -> Affect {
        let mut p = AffectParams::default();
        p.control.enabled = true;
        Affect::new(p)
    }

    #[test]
    fn control_off_changes_nothing() {
        let mut a = Affect::default();
        a.observe_control(true, true);
        assert_eq!((a.kappa(), a.flee_push()), (0.0, 1.0));
        // Pain alone does not raise D without control (V stays above θ).
        a.step(ValenceInputs {
            damage: 0.05,
            ..ValenceInputs::default()
        });
        assert_eq!(a.d(), 0.0);
    }

    #[test]
    fn pain_raises_d_scaled_by_lack_of_control() {
        // V target −0.025 stays above θ; pain min(1, 20·0.05) = 1 drives D.
        let pain = ValenceInputs {
            damage: 0.05,
            ..ValenceInputs::default()
        };
        let mut a = with_control();
        a.step(pain);
        assert!((a.d() - 0.01).abs() < 1e-7, "d_rise·1·(1 − 0)·(1 − 0)");
        let mut b = with_control();
        b.observe_control(true, true); // κ = 0.2
        b.step(pain);
        assert!((b.d() - 0.008).abs() < 1e-7, "× (1 − 0.2)");
        // Half the pain, half the drive: 20·0.025 = 0.5.
        let mut h = with_control();
        h.step(ValenceInputs {
            damage: 0.025,
            ..ValenceInputs::default()
        });
        assert!((h.d() - 0.005).abs() < 1e-7);
        // Low V drives D when it beats pain: |V| after one step from −1.
        let mut v = with_control();
        v.step(ValenceInputs {
            threat: 1.0,
            damage: 1.0,
            ..ValenceInputs::default()
        });
        // V = 0.2·(−0.8) = −0.16 > θ, so pain (1) drives: 0.01.
        assert!((v.d() - 0.01).abs() < 1e-7);
    }

    #[test]
    fn control_speeds_recovery_and_damps_flee() {
        let mut a = with_control();
        a.force(0.0, 0.5);
        assert!((a.flee_push() - 0.5).abs() < 1e-7);
        a.observe_control(true, true); // κ = 0.2
        a.step(ValenceInputs::default());
        // D −= (0.0005 + 0.005·0.2)·0.5 = 0.00075.
        assert!((a.d() - 0.49925).abs() < 1e-7, "{}", a.d());
        let mut b = with_control();
        b.force(0.0, 0.5);
        b.step(ValenceInputs::default());
        assert!((b.d() - 0.49975).abs() < 1e-7, "κ = 0: base recovery");
    }

    #[test]
    fn force_clamps() {
        let mut a = Affect::default();
        a.force(-2.0, 1.5);
        assert_eq!((a.v(), a.d()), (-1.0, 1.0));
        a.force(0.5, -1.0);
        assert_eq!((a.v(), a.d()), (0.5, 0.0));
    }

    proptest! {
        #[test]
        fn ranges_hold(r in -1.0f32..=1.0, th in 0.0f32..=1.0, dm in 0.0f32..=1.0, tm in -1.0f32..=1.0) {
            let mut a = Affect::new(AffectParams::default());
            for _ in 0..50 {
                a.step(ValenceInputs { r_mean_20: r, threat: th, damage: dm, tm_valence: tm });
            }
            prop_assert!((-1.0..=1.0).contains(&a.v()));
            prop_assert!((0.0..=1.0).contains(&a.d()));
        }
    }
}
