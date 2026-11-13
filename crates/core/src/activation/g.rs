//! Activation function g(C) (01 §1.4; spec §16; OQ8) and external drive (#41).

use serde::{Deserialize, Serialize};

/// g maps C to how much movement happens. 01 §1.4 fixes only g(0) = 0 and
/// "increasing"; the form is open (OQ8), so it is a trait.
pub trait Activation {
    fn g(&self, c: f32) -> f32;
}

/// `g(C) = C^α` with α > 1 (spec §16 default α = 2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerActivation {
    pub alpha: f32,
}

impl Default for PowerActivation {
    fn default() -> Self {
        Self { alpha: 2.0 }
    }
}

impl Activation for PowerActivation {
    fn g(&self, c: f32) -> f32 {
        c.clamp(0.0, 1.0).powf(self.alpha)
    }
}

/// `g(C) = log(1 + kC) / log(1 + k)` (spec §16 sweep alternative).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogActivation {
    pub k: f32,
}

impl Activation for LogActivation {
    fn g(&self, c: f32) -> f32 {
        (1.0 + self.k * c.clamp(0.0, 1.0)).ln() / (1.0 + self.k).ln()
    }
}

/// Which g(C) form a brain uses (OQ8 sweep handle, spec §16).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GShape {
    /// `C^α` (spec §16 default).
    #[default]
    Power,
    /// `log(1 + kC) / log(1 + k)`.
    Log,
}

/// Build the g(C) a config asks for (spec §16; OQ8).
pub fn build_activation(shape: GShape, alpha: f32, k: f32) -> Box<dyn Activation> {
    match shape {
        GShape::Power => Box::new(PowerActivation { alpha }),
        GShape::Log => Box::new(LogActivation { k }),
    }
}

/// Effort for a commanded step: `max(g(C), ε_ext·(1 − D))` (spec §16, #41).
/// Self-generated actions use plain g(C), so g(0) = 0 still means no
/// self-generated movement (01 §1.4, §1.5 drift).
pub fn external_drive_effort(g_c: f32, eps_ext: f32, d: f32) -> f32 {
    g_c.max(eps_ext * (1.0 - d.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn forms() -> Vec<Box<dyn Activation>> {
        vec![
            Box::new(PowerActivation { alpha: 1.5 }),
            Box::new(PowerActivation { alpha: 2.0 }),
            Box::new(PowerActivation { alpha: 3.0 }),
            Box::new(LogActivation { k: 10.0 }),
        ]
    }

    #[test]
    fn build_picks_the_form() {
        let p = build_activation(GShape::Power, 3.0, 10.0);
        assert!((p.g(0.5) - 0.125).abs() < 1e-6);
        let l = build_activation(GShape::Log, 3.0, 10.0);
        assert!((l.g(0.5) - 6f32.ln() / 11f32.ln()).abs() < 1e-6);
        assert_eq!(GShape::default(), GShape::Power);
    }

    #[test]
    fn g_of_zero_is_zero() {
        for f in forms() {
            assert_eq!(f.g(0.0), 0.0);
        }
    }

    proptest! {
        /// 01 §1.4: g increases with C.
        #[test]
        fn g_is_monotonic(a in 0.0f32..=1.0, b in 0.0f32..=1.0) {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            for f in forms() {
                prop_assert!(f.g(lo) <= f.g(hi));
            }
        }

        /// #41: a commanded step never runs below ε_ext·(1−D), and a shut-down
        /// agent (D → 1) barely responds.
        #[test]
        fn external_drive_floor(c in 0.0f32..=1.0, d in 0.0f32..=1.0) {
            let g = PowerActivation::default().g(c);
            let e = external_drive_effort(g, 0.2, d);
            prop_assert!(e >= g);
            prop_assert!(e >= 0.2 * (1.0 - d) - 1e-7);
        }
    }

    #[test]
    fn shutdown_agent_barely_responds_to_commands() {
        assert!(external_drive_effort(0.0, 0.2, 0.95) < 0.011);
    }
}
