//! Body: energy and fatigue (spec §5). The emergency reflex policy (#42)
//! lives with the hosts; the brain only sees reflex events in the log.

/// Agent-internal fatigue (spec §5), the same in both worlds:
/// `fatigue ← clamp(fatigue + e_t + e_x·effort·[external] − r_rest·[resting], 0, 1)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fatigue {
    pub e_t: f32,
    pub e_x: f32,
    pub r_rest: f32,
    value: f32,
}

impl Default for Fatigue {
    fn default() -> Self {
        Self {
            e_t: 1e-4,
            e_x: 2e-3,
            r_rest: 5e-3,
            value: 0.0,
        }
    }
}

impl Fatigue {
    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn step(&mut self, effort: f32, external: bool, resting: bool) {
        let mut f = self.value + self.e_t;
        if external {
            f += self.e_x * effort;
        }
        if resting {
            f -= self.r_rest;
        }
        self.value = f.clamp(0.0, 1.0);
    }
}

/// `energy = (health + food + (1 − fatigue)) / 3` (spec §5).
pub fn energy(health: f32, food: f32, fatigue: f32) -> f32 {
    ((health + food + (1.0 - fatigue)) / 3.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_tires_rest_restores() {
        let mut f = Fatigue::default();
        for _ in 0..500 {
            f.step(1.0, true, false);
        }
        let tired = f.value();
        assert!(tired > 0.9);
        for _ in 0..200 {
            f.step(0.0, false, true);
        }
        assert!(f.value() < tired - 0.5);
        assert!((energy(1.0, 1.0, 0.0) - 1.0).abs() < 1e-6);
    }
}
