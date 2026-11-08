//! Drawing an agent around its archetype's means (01 §4.10; spec §19).

use cognitive_core::agent::{BrainConfig, LensPrior, Spread, draw, jitter};
use proptest::prelude::*;

#[test]
fn jitter_by_hand() {
    // mean + spread·|mean|·z, clipped.
    assert!((jitter(0.4, 1.0, 0.1, 0.0, 1.0) - 0.44).abs() < 1e-6);
    assert!((jitter(-0.4, 1.0, 0.1, f32::NEG_INFINITY, 0.0) + 0.36).abs() < 1e-6);
    assert!((jitter(2.0, -0.5, 0.2, 0.0, 9.0) - 1.8).abs() < 1e-6);
    assert_eq!(jitter(0.95, 1.0, 0.1, 0.0, 1.0), 1.0, "clipped high");
    assert_eq!(jitter(0.1, -20.0, 0.1, 0.0, 1.0), 0.0, "clipped low");
    assert_eq!(jitter(0.0, 3.0, 0.1, -1.0, 1.0), 0.0, "a zero mean stays");
}

/// An archetype with every drawn field set away from zero.
fn busy() -> BrainConfig {
    let mut c = BrainConfig {
        oracle_bias: [0.5, -0.5, 0.3, 0.2],
        lens_priors: vec![LensPrior {
            mode: 1,
            feature: 7,
            weight: -1.0,
        }],
        ..BrainConfig::default()
    };
    c.tenacity_ab[2] = (3.0, 1.0);
    c
}

/// Every drawn parameter, flattened.
fn drawn(c: &BrainConfig) -> Vec<f32> {
    let mut v = vec![
        c.curiosity.base,
        c.curiosity.rate,
        c.k.novelty,
        c.k.learning_progress,
        c.k.energy,
        c.k.threat,
        c.k.habituation,
        c.k.social,
        c.permeability.base,
        c.tenacity_gamma,
        c.kick.p_base,
        c.kick.theta,
        c.kick.n as f32,
        c.kick.kappa,
        c.tm_k as f32,
        c.tm_tau,
        c.beta_u,
        c.beta_c,
        c.eps_ext,
        c.aging_rate,
    ];
    v.extend(c.tenacity_ab.iter().flat_map(|&(a, b)| [a, b]));
    v.extend(c.oracle_bias);
    v.extend(c.lens_priors.iter().map(|p| p.weight));
    v.extend(c.lens_lr);
    v
}

#[test]
fn every_parameter_is_drawn() {
    let mean = drawn(&busy());
    let draws: Vec<Vec<f32>> = (0..20)
        .map(|s| drawn(&draw(&busy(), Spread::default(), s)))
        .collect();
    for (i, m) in mean.iter().enumerate() {
        assert!(
            draws.iter().any(|d| d[i] != *m),
            "parameter {i} never moved"
        );
    }
}

#[test]
fn no_spread_no_change_and_seeds_repeat() {
    assert_eq!(draw(&busy(), Spread { sd_frac: 0.0 }, 7), busy());
    let a = draw(&busy(), Spread::default(), 7);
    assert_eq!(a, draw(&busy(), Spread::default(), 7));
    assert_ne!(a, draw(&busy(), Spread::default(), 8));
}

#[test]
fn draws_centre_on_the_mean_with_ten_percent_spread() {
    let n = 4000;
    let xs: Vec<f32> = (0..n)
        .map(|s| {
            draw(&BrainConfig::default(), Spread::default(), s)
                .curiosity
                .base
        })
        .collect();
    let mean = xs.iter().sum::<f32>() / n as f32;
    let sd = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n as f32).sqrt();
    assert!((mean - 0.40).abs() < 0.003, "mean {mean}");
    assert!((sd - 0.04).abs() < 0.003, "sd {sd}");
}

proptest! {
    #[test]
    fn draws_stay_in_range(seed in any::<u64>(), frac in 0.0f32..3.0) {
        let mean = busy();
        let c = draw(&mean, Spread { sd_frac: frac }, seed);
        for x in [c.curiosity.base, c.curiosity.rate, c.permeability.base, c.tenacity_gamma,
                  c.kick.p_base, c.kick.theta, c.eps_ext] {
            prop_assert!((0.0..=1.0).contains(&x));
        }
        for x in [c.kick.kappa, c.tm_tau, c.beta_u, c.beta_c, c.aging_rate] {
            prop_assert!(x >= 0.0);
        }
        prop_assert!(c.kick.n >= 1 && c.tm_k >= 1);
        prop_assert!(c.lens_lr.iter().all(|&x| x >= 0.0));
        prop_assert!(c.tenacity_ab.iter().all(|&(a, b)| a >= 0.0 && b >= 0.0));
        // Signed weights keep their sign.
        for (d, m) in drawn(&c).iter().zip(drawn(&mean)) {
            prop_assert!(d * m >= 0.0, "{d} vs {m}");
        }
    }
}
