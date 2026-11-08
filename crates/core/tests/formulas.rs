//! Exact-value tests: every formula in 06-sim-spec.md Part B checked against
//! numbers worked out by hand (shown in the comments). Property tests check
//! ranges and directions; these check the arithmetic itself, so a wrong sign,
//! operator or coefficient fails a test. Found by mutation testing.

use cognitive_core::activation::{
    Activation, Affect, AffectParams, Curiosity, CuriosityParams, LogActivation, PowerActivation,
    ValenceInputs, external_drive_effort,
};
use cognitive_core::body::{Fatigue, energy};
use cognitive_core::control::development::{Development, Stage};
use cognitive_core::control::kick::{StuckDetector, kick_extremes};
use cognitive_core::control::oracle::{CONTEXT_DIM, Oracle, WeightAdjust, with_floor};
use cognitive_core::control::permeability::{Permeability, PermeabilityInputs, PermeabilityParams};
use cognitive_core::control::reward::meta_reward;
use cognitive_core::control::tenacity::Tenacity;
use cognitive_core::control::time::{age, plasticity, urgency};
use cognitive_core::derived::is_creative;
use cognitive_core::direction::ThoughtMomentum;
use cognitive_core::memory::{MemoryItem, MemoryParams, PriorityInputs, Tier, initial_priority};
use cognitive_core::modes::{Lens, Mode, ModeNetwork, N_FEATURES, feature, score};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-5, "{a} != {b}");
}

fn bias_ctx() -> [f32; CONTEXT_DIM] {
    let mut x = [0.0; CONTEXT_DIM];
    x[0] = 1.0;
    x
}

// ---------- 01 §1 Activation ----------

#[test]
fn curiosity_drift_exact() {
    let mut c = Curiosity::new(CuriosityParams::default()); // C = 0.4
    // rise: 0.1·(0.9 − 0.4) = 0.05, × (1 − 0.25) = 0.0375
    close(c.drift(0.9, 0.25), 0.0375);
    close(c.value(), 0.4375);
    // fall is not dampened: 0.1·(0 − 0.4375) = −0.04375
    close(c.drift(0.0, 0.9), -0.04375);
    close(c.value(), 0.39375);
}

#[test]
fn g_exact() {
    close(PowerActivation { alpha: 2.0 }.g(0.5), 0.25);
    close(PowerActivation { alpha: 3.0 }.g(0.5), 0.125);
    // ln(1 + 10·0.5) / ln(11) = ln 6 / ln 11
    close(LogActivation { k: 10.0 }.g(0.5), 0.747_221_7);
    close(LogActivation { k: 10.0 }.g(1.0), 1.0);
    // max(0.04, 0.2·(1 − 0.5)) = 0.1
    close(external_drive_effort(0.04, 0.2, 0.5), 0.1);
}

#[test]
fn valence_target_exact() {
    // 0.6·0.5 − 0.3·0.2 − 0.5·0.1 + 0.2·0.4 = 0.27
    let x = ValenceInputs {
        r_mean_20: 0.5,
        threat: 0.2,
        damage: 0.1,
        tm_valence: 0.4,
    };
    close(Affect::v_target(x), 0.27);
    let mut a = Affect::new(AffectParams::default());
    a.step(x);
    close(a.v(), 0.054); // 0 + 0.2·0.27
    close(a.d(), 0.0);
}

#[test]
fn valence_and_suppression_sequence_exact() {
    // Worked by hand in the PR: three harsh ticks, then three calm ones.
    let harsh = ValenceInputs {
        r_mean_20: -1.0,
        threat: 1.0,
        damage: 1.0,
        tm_valence: -1.0,
    };
    let mut a = Affect::new(AffectParams::default());
    for _ in 0..3 {
        a.step(harsh);
    }
    close(a.v(), -0.488);
    close(a.d(), 0.008_462_432);
    for _ in 0..3 {
        a.step(ValenceInputs::default());
    }
    close(a.v(), -0.249_856);
    close(a.d(), 0.015_410_366);
}

// ---------- 01 §2 Direction ----------

#[test]
fn thought_momentum_fade_exact() {
    let mut tm = ThoughtMomentum::new(3, 10.0);
    tm.tick(&[(1, 1.0)]);
    tm.tick(&[(1, 0.5)]);
    close(tm.activation(1), 0.904_837_4 + 0.5); // e^(−1/10) + 0.5
    assert_eq!(tm.activation(9), 0.0);
}

// ---------- 01 §3 Modes ----------

#[test]
fn lens_priors_exact() {
    use feature::*;
    let t = |m| Lens::with_priors(m).theta;
    let e = t(Mode::Exploring);
    assert_eq!(
        [e[BIAS], e[FRESH], e[UNCERTAINTY], e[RISK], e[VALENCE_TAG]],
        [-0.6, 2.0, 0.3, -1.5, 0.5]
    );
    let l = t(Mode::Learning);
    assert_eq!(
        [l[BIAS], l[UNKNOWN], l[UNCERTAINTY], l[COMPETENCE]],
        [-0.5, 1.5, 1.0, -0.5]
    );
    let p = t(Mode::Performing);
    assert_eq!(
        [p[COMPETENCE], p[RELEVANCE], p[RISK], p[VALENCE_TAG]],
        [1.5, 1.0, -0.5, 0.3]
    );
    let o = t(Mode::Optimizing);
    assert_eq!(
        [o[BIAS], o[SAVING], o[RECENT_FAIL], o[RISK], o[VALENCE_TAG]],
        [-0.3, 2.0, 0.8, -2.0, 0.8]
    );
    for (m, n) in [(e, 5), (l, 4), (p, 4), (o, 5)] {
        assert_eq!(
            m.iter().filter(|w| **w != 0.0).count(),
            n,
            "no extra weights"
        );
    }
}

#[test]
fn lens_learning_step_exact() {
    let mut lens = Lens::with_priors(Mode::Learning);
    lens.theta = [0.0; N_FEATURES];
    let mut phi = [0.0; N_FEATURES];
    phi[0] = 1.0;
    phi[1] = 0.5;
    // step = lr·(success − m)·(1 − m²) = 0.1·(1 − 0.5)·0.75 = 0.0375
    lens.learn(&phi, 0.5, 1.0, 0.1);
    close(lens.theta[0], 0.0375);
    close(lens.theta[1], 0.01875);
    // negative case: 0.1·(−1 − (−0.5))·0.75 = −0.0375
    lens.theta = [0.0; N_FEATURES];
    lens.learn(&phi, -0.5, -1.0, 0.1);
    close(lens.theta[0], -0.0375);
    // score = tanh(θ·φ)
    lens.theta = [0.0; N_FEATURES];
    lens.theta[0] = 0.5;
    lens.theta[1] = 1.0;
    close(lens.score(&phi), 1.0f32.tanh());
}

#[test]
fn aggregate_exact() {
    // 0.1·0.25 + 0.2·(−0.36) + 0.3·0.81 + 0.4·(−1) = −0.204
    close(
        score(&[0.1, 0.2, 0.3, 0.4], &[0.5, -0.6, 0.9, -1.0]),
        -0.204,
    );
    close(score(&[0.25; 4], &[0.5; 4]), 0.25);
}

// ---------- 01 §4 Control ----------

#[test]
fn oracle_weights_exact() {
    let mut o = Oracle::default();
    for w in o.weights(&bias_ctx(), WeightAdjust::default()) {
        close(w, 0.25);
    }
    // P preference ln 3 → softmax P = 0.5, others 1/6
    o.theta[Mode::Performing.index()][0] = 3.0f32.ln();
    let w = o.weights(&bias_ctx(), WeightAdjust::default());
    close(w[2], 0.02 + 0.92 * 0.5);
    close(w[0], 0.02 + 0.92 / 6.0);
    // urgency 0.3 on E: v = [1/6, 1/6+0.3, 0.5, 1/6], Σ = 1.3
    let w = o.weights(
        &bias_ctx(),
        WeightAdjust {
            urgency_bias: 0.3,
            choke_shift: 0.0,
        },
    );
    close(w[1], 0.350_256_4);
    // choke 0.2: P 0.5 → 0.3, O 1/6 → 0.3667
    let w = o.weights(
        &bias_ctx(),
        WeightAdjust {
            urgency_bias: 0.0,
            choke_shift: 0.2,
        },
    );
    close(w[2], 0.02 + 0.92 * 0.3);
    close(w[3], 0.02 + 0.92 * (1.0 / 6.0 + 0.2));
    // a choke larger than P's share moves only what P has
    let w = o.weights(
        &bias_ctx(),
        WeightAdjust {
            urgency_bias: 0.0,
            choke_shift: 2.0,
        },
    );
    close(w[2], 0.02);
}

#[test]
fn oracle_credit_exact() {
    // products: 0.1, 0.108, 0.162, 0.1 → Performing
    assert_eq!(
        Oracle::credited(&[0.4, 0.3, 0.2, 0.1], &[0.5, 0.6, 0.9, 1.0]),
        Mode::Performing
    );
    // a veto never earns credit over a positive score
    assert_eq!(
        Oracle::credited(&[0.1, 0.7, 0.1, 0.1], &[0.2, -0.9, 0.0, 0.0]),
        Mode::Learning
    );
    // ties go to the first mode
    assert_eq!(Oracle::credited(&[0.25; 4], &[0.5; 4]), Mode::Learning);
}

#[test]
fn oracle_update_exact() {
    let x = bias_ctx();
    let mut o = Oracle::default();
    let w = [0.25; 4];
    // R = 1, R̄ = 0: Θ_i += 0.05·1·(1[i=P] − 0.25)
    o.update(&x, &w, Mode::Performing, 1.0, &[0.5; 4]);
    close(o.theta[2][0], 0.0375);
    close(o.theta[0][0], -0.0125);
    close(o.r_bar, 0.01);
    assert_eq!(o.theta[2][1], 0.0, "zero context entries are not updated");
    // setback: R = −1 scaled by (1 − T_i)
    let mut o = Oracle::default();
    o.update(&x, &w, Mode::Performing, -1.0, &[0.2, 0.4, 0.6, 0.8]);
    close(o.theta[0][0], 0.01); // −0.05·(−0.25)·0.8
    close(o.theta[1][0], 0.0075);
    close(o.theta[2][0], -0.015); // −0.05·0.75·0.4
    close(o.theta[3][0], 0.0025);
    close(o.r_bar, -0.01);
}

#[test]
fn floor_projection_exact() {
    let w = with_floor([1.0, 1.0, 2.0, 0.0], 0.02);
    close(w[0], 0.02 + 0.92 * 0.25);
    close(w[2], 0.02 + 0.92 * 0.5);
    close(w[3], 0.02);
}

#[test]
fn kick_threshold_is_strict() {
    let mut rng = ChaCha8Rng::seed_from_u64(1);
    let mut m = vec![0.9f32, -0.9, 0.900_001];
    let fired = kick_extremes(&mut m, 0.9, 1.0, &mut rng);
    assert_eq!(fired, 1, "|m| = θ does not fire; just above does");
    assert_eq!(m, vec![0.9, -0.9, -0.900_001]);
    let mut never = vec![1.0f32; 1000];
    assert_eq!(kick_extremes(&mut never, 0.9, 0.0, &mut rng), 0, "p = 0");
}

#[test]
fn stuck_entropy_exact() {
    // 9:1 split = 0.469 bits (< 0.5, stuck); 7:3 = 0.881 bits (not stuck)
    let run = |a: usize| {
        let mut s = StuckDetector::new(10);
        for i in 0..10 {
            s.record([0.25; 4], 0.3, u32::from(i < a));
        }
        s.is_stuck()
    };
    assert!(run(1));
    assert!(!run(3));
}

#[test]
fn stuck_window_slides() {
    let mut s = StuckDetector::new(5);
    for i in 0..10 {
        s.record([0.25; 4], 0.3, i); // varied: not stuck
    }
    for _ in 0..5 {
        s.record([0.25; 4], 0.3, 99);
    }
    assert!(s.is_stuck(), "old ticks leave the window");
}

#[test]
fn tenacity_exact() {
    let mut t = Tenacity::new(1.0, 1.0, 0.5);
    t.record(1.0); // succ 1, att 1 → 2/3
    close(t.value(), 2.0 / 3.0);
    t.record(0.0); // succ 0.5, att 1.5 → 1.5/3.5
    close(t.value(), 1.5 / 3.5);
}

#[test]
fn permeability_exact() {
    let x = PermeabilityInputs {
        threat: 0.5,
        stakes: 0.5,
        fatigue: 0.5,
        tenacity_p: 0.3,
        routine: 0.5,
    };
    let mut p = Permeability::new(PermeabilityParams::default());
    // 0.4 + 0.15 + 0.2 + 0.1 + 0.4·0.2 − 0.15 = 0.78
    close(p.target(x), 0.78);
    p.step(x);
    close(p.value(), 0.438); // 0.4 + 0.1·0.38
    for _ in 0..500 {
        p.step(x);
    }
    close(p.choke_shift(), 0.09); // 0.5·(0.78 − 0.6)
    close(p.focus(true), 0.856); // 1 − 0.8·0.18
}

#[test]
fn meta_reward_exact() {
    // 0.1 − 0.2 + 0.15 + 0.2 + 0.5·0.2 = 0.35
    close(
        meta_reward(&[0.1, 0.2, 0.3, 0.4], &[1.0, -1.0, 0.5, 0.5], 0.2, 0.5),
        0.35,
    );
    close(meta_reward(&[0.25; 4], &[0.0; 4], -0.4, 0.5), -0.2);
}

#[test]
fn development_schedules_exact() {
    let d = Development::new(0.25);
    close(d.lr_multiplier(), 1.75);
    close(d.kick_rate(0.05), 0.1625);
    close(d.threshold(0.3, 0.9), 0.45);
    close(d.subconscious_budget(2.0, false), 3.5);
    close(d.subconscious_budget(2.0, true), 10.5);
    assert_eq!(Development::new(0.2).stage(), Stage::RapidDevelopment);
    assert_eq!(Development::new(0.6).stage(), Stage::Maturation);
    assert_eq!(Development::new(0.199).stage(), Stage::Initialization);
}

#[test]
fn time_exact() {
    close(age(2.0, 10, 3.0, 5), 35.0);
    // 0.05·e^(−0.01·(1 − 0.25)·100) = 0.05·e^(−0.75)
    close(plasticity(0.05, 0.01, 0.5, 100.0), 0.023_618_33);
    close(urgency(25.0, 100.0), 0.5625);
}

// ---------- 01 §5 Memory ----------

#[test]
fn strength_exact() {
    let item = MemoryItem {
        content: String::new(),
        tier: Tier::Semantic,
        priority: 0.7,
        v_enc: 0.0,
        habituation: 0.0,
        t_created: 0,
        t_last_access: 0,
        access_count: 3,
    };
    // 0.7·0.8·e^(−1e-5·1000) + 0.05·ln 4
    close(item.strength(1000, &MemoryParams::default()), 0.623_742_6);
}

#[test]
fn priority_exact() {
    // 0.1 + 0.15 + 0.25 + 0.06 + 0.06 + 0.07 + 0.2·0.5·0.5 = 0.74
    let p = initial_priority(PriorityInputs {
        v_enc: -0.5,
        survival: 1.0,
        repetition: 0.6,
        connection: 0.4,
        c_enc: 0.7,
        urgency: 0.5,
    });
    close(p, 0.74);
}

#[test]
fn equal_tier_rates_break_the_order() {
    let mut m = MemoryParams::default();
    m.semantic.lambda = m.procedural.lambda;
    assert!(!m.respects_tier_order());
}

#[test]
fn habituation_exact() {
    let mut m = MemoryItem {
        content: String::new(),
        tier: Tier::Episodic,
        priority: 0.5,
        v_enc: 0.0,
        habituation: 0.5,
        t_created: 0,
        t_last_access: 0,
        access_count: 0,
    };
    m.habituate();
    close(m.habituation, 0.55);
    m.dishabituate();
    close(m.habituation, 0.549_45);
}

// ---------- Body and derived ----------

#[test]
fn fatigue_and_energy_exact() {
    let mut f = Fatigue::default();
    f.step(0.5, true, false); // 1e-4 + 2e-3·0.5
    close(f.value(), 0.0011);
    f.step(1.0, false, false); // + 1e-4 only
    close(f.value(), 0.0012);
    for _ in 0..1000 {
        f.step(1.0, true, false);
    }
    let tired = f.value();
    f.step(0.0, false, true); // + 1e-4 − 5e-3
    close(f.value(), tired + 1e-4 - 5e-3);
    close(energy(0.5, 0.7, 0.3), 0.633_333_3);
}

#[test]
fn creativity_boundaries_are_strict() {
    assert!(!is_creative(0.5, 0.5, 0.3), "novelty must exceed θ");
    assert!(!is_creative(0.6, 0.5, 0.0), "m_O must be positive");
    assert!(is_creative(0.51, 0.5, 0.01));
}

#[test]
fn affect_threshold_is_strict() {
    // V exactly at −0.3 must not raise D. With v_rate = 1, V jumps straight to
    // V_target = −0.3·threat = −0.3.
    let mut a = Affect::new(AffectParams {
        v_rate: 1.0,
        ..AffectParams::default()
    });
    a.step(ValenceInputs {
        threat: 1.0,
        ..Default::default()
    });
    close(a.v(), -0.3);
    assert_eq!(a.d(), 0.0, "V = −0.3 is not below the threshold");
}

#[test]
fn oracle_update_with_nonzero_baseline_exact() {
    let x = bias_ctx();
    let w = [0.25; 4];
    let mut o = Oracle {
        r_bar: 0.2,
        ..Oracle::default()
    };
    // adv = 0.6 − 0.2 = 0.4: Θ_P += 0.05·0.4·0.75 = 0.015
    o.update(&x, &w, Mode::Performing, 0.6, &[0.5; 4]);
    close(o.theta[2][0], 0.015);
    close(o.theta[0][0], -0.005);
    // R̄ ← 0.2 + 0.01·(0.6 − 0.2) = 0.204
    close(o.r_bar, 0.204);
}

#[test]
fn oracle_reads_the_whole_context() {
    // Θ·x must be a dot product: two context entries with different values.
    let mut o = Oracle::default();
    let mut x = [0.0; CONTEXT_DIM];
    x[0] = 1.0;
    x[9] = 0.5;
    o.theta[Mode::Exploring.index()][9] = 2.0f32.ln() / 0.5; // pref_E = ln 2
    let w = o.weights(&x, WeightAdjust::default());
    // softmax = [1, 2, 1, 1] / 5
    close(w[1], 0.02 + 0.92 * 0.4);
    close(w[0], 0.02 + 0.92 * 0.2);
}

#[test]
fn oracle_survives_extreme_preferences() {
    let mut o = Oracle::default();
    o.theta[Mode::Learning.index()][0] = 500.0;
    o.theta[Mode::Optimizing.index()][0] = -500.0;
    let w = o.weights(&bias_ctx(), WeightAdjust::default());
    assert!(w.iter().all(|v| v.is_finite()));
    close(w[0], 1.0 - 3.0 * 0.02);
}
