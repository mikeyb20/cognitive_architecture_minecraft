//! Boundary permeability π, choking and focus (01 §4.5; spec §11).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermeabilityParams {
    pub base: f32,
    pub rate: f32,
    pub h_threat: f32,
    pub h_stakes: f32,
    pub h_fatigue: f32,
    pub h_tenacity: f32,
    pub h_routine: f32,
    /// Choking threshold π_choke.
    pub choke: f32,
    /// Share δ of the excess moved from P to O.
    pub delta: f32,
    /// Execution loss coefficient c_leak (sim only, #43).
    pub c_leak: f32,
}

impl Default for PermeabilityParams {
    /// Spec §11 and §19 defaults (proposed).
    fn default() -> Self {
        Self {
            base: 0.40,
            rate: 0.1,
            h_threat: 0.3,
            h_stakes: 0.4,
            h_fatigue: 0.2,
            h_tenacity: 0.4,
            h_routine: 0.3,
            choke: 0.6,
            delta: 0.5,
            c_leak: 0.8,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PermeabilityInputs {
    pub threat: f32,
    pub stakes: f32,
    pub fatigue: f32,
    /// T[Performing, current task].
    pub tenacity_p: f32,
    pub routine: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Permeability {
    pub params: PermeabilityParams,
    pi: f32,
}

impl Permeability {
    pub fn new(params: PermeabilityParams) -> Self {
        let pi = params.base;
        Self { params, pi }
    }

    pub fn value(&self) -> f32 {
        self.pi
    }

    /// `π_target = clamp(π_base + h_th·threat + h_st·stakes + h_fa·fatigue
    ///            + h_T·(0.5 − T[P,t]) − h_ro·routine, 0, 1)`
    pub fn target(&self, x: PermeabilityInputs) -> f32 {
        let p = &self.params;
        (p.base
            + p.h_threat * x.threat
            + p.h_stakes * x.stakes
            + p.h_fatigue * x.fatigue
            + p.h_tenacity * (0.5 - x.tenacity_p)
            - p.h_routine * x.routine)
            .clamp(0.0, 1.0)
    }

    /// `π ← π + rate_π·(π_target − π)` (01 §4.5).
    pub fn step(&mut self, x: PermeabilityInputs) {
        let t = self.target(x);
        self.pi = (self.pi + self.params.rate * (t - self.pi)).clamp(0.0, 1.0);
    }

    /// Weight moved from P to O when P leads: `δ·max(0, π − π_choke)`.
    pub fn choke_shift(&self) -> f32 {
        self.params.delta * (self.pi - self.params.choke).max(0.0)
    }

    /// `focus = 1 − c_leak·max(0, π − π_choke)·[w_P largest]` (sim only, #43).
    pub fn focus(&self, p_leads: bool) -> f32 {
        if !p_leads {
            return 1.0;
        }
        (1.0 - self.params.c_leak * (self.pi - self.params.choke).max(0.0)).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_raises_pi_routine_lowers_it() {
        let mut lava = Permeability::new(PermeabilityParams::default());
        let mut flow = lava.clone();
        for _ in 0..200 {
            lava.step(PermeabilityInputs {
                threat: 1.0,
                stakes: 1.0,
                fatigue: 0.5,
                tenacity_p: 0.2,
                routine: 0.0,
            });
            flow.step(PermeabilityInputs {
                tenacity_p: 0.9,
                routine: 1.0,
                ..Default::default()
            });
        }
        // 01 §4.5 example: bridging over lava with low T → π high → choking.
        assert!(lava.value() > 0.9 && lava.choke_shift() > 0.1);
        assert!(lava.focus(true) < 0.7);
        // Routine + high tenacity → low π → flow, no choking.
        assert!(flow.value() < 0.2 && flow.choke_shift() == 0.0);
        assert_eq!(flow.focus(true), 1.0);
    }
}
