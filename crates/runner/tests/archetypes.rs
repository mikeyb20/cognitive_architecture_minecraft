//! The archetype files match spec §19's table (#44), so a typo in a TOML
//! breaks a test.

use cognitive_core::agent::{BrainConfig, LensPrior, Spread, draw};
use runner::archetype::{ARCHETYPES, DRAW_SALT, agent, means};

/// One column of spec §19.
struct Column {
    c_base: f32,
    k: [f32; 6],
    c_rate: f32,
    pi_base: f32,
    perf_ab: (f32, f32),
    gamma: f32,
    kick: (f32, f32, usize, f32),
    bias: [f32; 4],
    lens: Option<(usize, usize, f32)>,
    lr: [f32; 4],
    tau: f32,
    beta_u: f32,
    eps_ext: f32,
}

fn column(name: &str) -> Column {
    match name {
        "explorer" => Column {
            c_base: 0.55,
            k: [0.60, 0.35, -0.30, -0.25, -0.45, 0.10],
            c_rate: 0.15,
            pi_base: 0.45,
            perf_ab: (1.0, 1.0),
            gamma: 0.97,
            kick: (0.08, 0.9, 200, 0.20),
            bias: [0.0, 0.8, -0.3, 0.0],
            lens: Some((1, 7, -1.0)),
            lr: [1.0, 1.3, 1.0, 1.0],
            tau: 30.0,
            beta_u: 0.3,
            eps_ext: 0.20,
        },
        "builder" => Column {
            c_base: 0.35,
            k: [0.20, 0.40, -0.40, -0.40, -0.15, 0.15],
            c_rate: 0.06,
            pi_base: 0.25,
            perf_ab: (3.0, 1.0),
            gamma: 0.99,
            kick: (0.03, 0.95, 500, 0.10),
            bias: [0.0, -0.3, 0.8, 0.2],
            lens: Some((2, 4, 2.0)),
            lr: [1.0, 0.8, 1.2, 1.0],
            tau: 100.0,
            beta_u: 0.1,
            eps_ext: 0.20,
        },
        "guardian" => Column {
            c_base: 0.40,
            k: [0.30, 0.25, -0.40, -0.60, -0.30, 0.45],
            c_rate: 0.10,
            pi_base: 0.50,
            perf_ab: (2.0, 1.0),
            gamma: 0.98,
            kick: (0.05, 0.9, 300, 0.15),
            bias: [0.0, 0.0, 0.3, 0.5],
            lens: Some((3, 7, -2.5)),
            lr: [1.0, 1.0, 1.0, 1.2],
            tau: 50.0,
            beta_u: 0.2,
            eps_ext: 0.20,
        },
        "helper" => Column {
            c_base: 0.40,
            k: [0.30, 0.30, -0.40, -0.40, -0.20, 0.60],
            c_rate: 0.10,
            pi_base: 0.40,
            perf_ab: (2.0, 1.0),
            gamma: 0.98,
            kick: (0.05, 0.9, 300, 0.15),
            bias: [0.0, -0.2, 0.6, 0.0],
            lens: Some((2, 5, 1.5)),
            lr: [1.0, 1.0, 1.1, 1.0],
            tau: 50.0,
            beta_u: 0.2,
            eps_ext: 0.35,
        },
        _ => unreachable!(),
    }
}

/// Spec §19's column on top of the defaults; nothing else changes.
fn expected(x: &Column) -> BrainConfig {
    let mut c = BrainConfig::default();
    c.curiosity.base = x.c_base;
    c.curiosity.rate = x.c_rate;
    [
        c.k.novelty,
        c.k.learning_progress,
        c.k.energy,
        c.k.threat,
        c.k.habituation,
        c.k.social,
    ] = x.k;
    c.permeability.base = x.pi_base;
    c.tenacity_ab[2] = x.perf_ab;
    c.tenacity_gamma = x.gamma;
    (c.kick.p_base, c.kick.theta, c.kick.n, c.kick.kappa) = x.kick;
    c.oracle_bias = x.bias;
    c.lens_priors = x
        .lens
        .map(|(mode, feature, weight)| LensPrior {
            mode,
            feature,
            weight,
        })
        .into_iter()
        .collect();
    c.lens_lr = x.lr;
    c.tm_tau = x.tau;
    c.beta_u = x.beta_u;
    c.eps_ext = x.eps_ext;
    c
}

#[test]
fn archetypes_match_spec_19() {
    for name in ARCHETYPES {
        assert_eq!(means(name).unwrap(), expected(&column(name)), "{name}");
    }
    // The defaults the files leave alone are the spec's default column.
    let d = BrainConfig::default();
    assert_eq!((d.tm_k, d.beta_c, d.aging_rate), (7, 0.1, 1e-6));
}

#[test]
fn the_default_archetype_is_the_default_config() {
    assert_eq!(means("default").unwrap(), BrainConfig::default());
    assert_eq!(
        means("trickster").err().unwrap(),
        "no archetype called \"trickster\""
    );
}

#[test]
fn agents_are_drawn_per_seed() {
    let a = agent("explorer", 3).unwrap();
    assert_eq!(a, agent("explorer", 3).unwrap());
    assert_ne!(a, agent("explorer", 4).unwrap());
    assert_ne!(a, means("explorer").unwrap());
    let salted = draw(
        &means("explorer").unwrap(),
        Spread::default(),
        3 ^ DRAW_SALT,
    );
    assert_eq!(a, salted, "the draw does not reuse the brain's seed");
    assert!(agent("nobody", 0).is_err());
}
