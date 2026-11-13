//! Drawing one agent's personality from an archetype (01 §4.10; spec §19):
//! every personality parameter is drawn from Normal(mean, sd) with
//! sd = `spread`·|mean| (spec §19: 10% unless stated), then clipped to its
//! range. Zero means stay zero, so an archetype's unset Oracle biases do
//! not wander. Signed weights keep their sign.

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use super::{BrainConfig, normal};

/// Spread of a drawn parameter as a fraction of its mean (spec §19).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spread {
    pub sd_frac: f32,
}

impl Default for Spread {
    fn default() -> Self {
        Self { sd_frac: 0.10 }
    }
}

/// One draw: `mean + spread·|mean|·z`, clipped to `[lo, hi]` (spec §19).
pub fn jitter(mean: f32, z: f32, spread: f32, lo: f32, hi: f32) -> f32 {
    (mean + spread * mean.abs() * z).clamp(lo, hi)
}

/// Bounds that keep a signed weight's sign (a negative k stays negative).
fn signed(x: f32) -> (f32, f32) {
    if x.is_sign_negative() {
        (f32::NEG_INFINITY, 0.0)
    } else {
        (0.0, f32::INFINITY)
    }
}

struct Draw {
    rng: ChaCha8Rng,
    spread: f32,
}

impl Draw {
    fn one(&mut self, x: &mut f32, lo: f32, hi: f32) {
        *x = jitter(*x, normal(&mut self.rng), self.spread, lo, hi);
    }

    fn sign(&mut self, x: &mut f32) {
        let (lo, hi) = signed(*x);
        self.one(x, lo, hi);
    }

    fn unit(&mut self, x: &mut f32) {
        self.one(x, 0.0, 1.0);
    }

    fn positive(&mut self, x: &mut f32) {
        self.one(x, 0.0, f32::INFINITY);
    }

    /// A count, drawn as a real and rounded, at least 1.
    fn count(&mut self, n: &mut usize) {
        let mut x = *n as f32;
        self.one(&mut x, 1.0, f32::INFINITY);
        *n = x.round() as usize;
    }
}

/// One agent drawn from an archetype's means (01 §4.10; spec §19). The
/// same `seed` gives the same agent.
pub fn draw(mean: &BrainConfig, spread: Spread, seed: u64) -> BrainConfig {
    let mut c = mean.clone();
    let mut d = Draw {
        rng: ChaCha8Rng::seed_from_u64(seed),
        spread: spread.sd_frac,
    };
    d.unit(&mut c.curiosity.base);
    d.unit(&mut c.curiosity.rate);
    let k = &mut c.k;
    for w in [
        &mut k.novelty,
        &mut k.learning_progress,
        &mut k.energy,
        &mut k.threat,
        &mut k.habituation,
        &mut k.social,
    ] {
        d.sign(w);
    }
    d.unit(&mut c.permeability.base);
    for (a, b) in &mut c.tenacity_ab {
        d.positive(a);
        d.positive(b);
    }
    d.unit(&mut c.tenacity_gamma);
    d.unit(&mut c.kick.p_base);
    d.unit(&mut c.kick.theta);
    d.count(&mut c.kick.n);
    d.positive(&mut c.kick.kappa);
    for b in &mut c.oracle_bias {
        d.sign(b);
    }
    for p in &mut c.lens_priors {
        d.sign(&mut p.weight);
    }
    for lr in &mut c.lens_lr {
        d.positive(lr);
    }
    d.count(&mut c.tm_k);
    d.positive(&mut c.tm_tau);
    d.positive(&mut c.beta_u);
    d.positive(&mut c.beta_c);
    d.unit(&mut c.eps_ext);
    d.positive(&mut c.aging_rate);
    c
}
