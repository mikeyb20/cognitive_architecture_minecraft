//! Valence V (01 §1.3) and suppression D (01 §1.2) updates (spec §12; OQ17).
//! All coefficients are proposals that OQ17 sweeps.

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
    /// DEF CHANGE F prototype: rate of the relief contingency estimates
    /// (0 = off: κ stays 0 and D is as in 01 §1.2).
    #[serde(default)]
    pub control_rate: f32,
}

impl Default for AffectParams {
    /// Spec §12 (proposed): D rises about 20× faster than it recovers.
    fn default() -> Self {
        Self {
            v_rate: 0.2,
            d_threshold: -0.3,
            d_rise: 0.01,
            d_recover: 0.0005,
            control_rate: F_RATE,
        }
    }
}

pub const F_RATE: f32 = 0.2;
pub const F_STRESS: bool = true;
pub const F_CTRL_RECOVER: f32 = 0.005;
pub const F_PAIN: f32 = 20.0;
pub const F2: bool = true;

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
    relief_act: f32,
    #[serde(default)]
    relief_other: f32,
}

impl Affect {
    pub fn new(params: AffectParams) -> Self {
        Self {
            params,
            v: 0.0,
            d: 0.0,
            relief_act: 0.0,
            relief_other: 0.0,
        }
    }

    pub fn v(&self) -> f32 {
        self.v
    }

    pub fn d(&self) -> f32 {
        self.d
    }

    /// DEF CHANGE F prototype: perceived control κ = ΔP, the relief rate
    /// after own escape attempts minus after anything else, floored at 0.
    pub fn kappa(&self) -> f32 {
        (self.relief_act - self.relief_other).clamp(0.0, 1.0)
    }

    /// Record one action taken in pain: was it an escape attempt, and did
    /// the pain ease by the next observation.
    pub fn ests(&self) -> (f32, f32) {
        (self.relief_act, self.relief_other)
    }

    pub fn observe_control(&mut self, escaped: bool, relief: bool) {
        if std::env::var("FDBG").is_ok() { eprintln!("obs esc={escaped} relief={relief}"); }
        let a = self.params.control_rate;
        let r = f32::from(u8::from(relief));
        let e = if escaped { &mut self.relief_act } else { &mut self.relief_other };
        *e += a * (r - *e);
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
    /// negative V and otherwise recovers very slowly (01 §1.2).
    pub fn step(&mut self, x: ValenceInputs) {
        let p = &self.params;
        self.v = (self.v + p.v_rate * (Self::v_target(x) - self.v)).clamp(-1.0, 1.0);
        let k = 1.0 - self.kappa();
        let stress = if F_STRESS && p.control_rate > 0.0 { (x.damage * F_PAIN).min(1.0) } else { 0.0 };
        if self.v < p.d_threshold || stress > 0.0 {
            let drive = if self.v < p.d_threshold { self.v.abs() } else { 0.0 }.max(stress);
            self.d += p.d_rise * drive * (1.0 - self.d) * k;
        } else {
            self.d -= (p.d_recover + F_CTRL_RECOVER * self.kappa()) * self.d;
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
