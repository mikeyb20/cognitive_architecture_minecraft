//! Pins every default parameter to the value written in 06-sim-spec.md, so a
//! silent change to a default breaks a test. If a default changes on purpose,
//! change the spec (and log the decision) and then this file.

use cognitive_core::activation::{
    AffectParams, CuriosityParams, GShape, PowerActivation, SignalWeights,
};
use cognitive_core::agent::{BrainConfig, BudgetPolicy};
use cognitive_core::control::kick::KickParams;
use cognitive_core::control::oracle::Oracle;
use cognitive_core::control::permeability::PermeabilityParams;
use cognitive_core::control::subconscious::SubconsciousParams;
use cognitive_core::memory::MemoryParams;
use cognitive_core::modes::{Lens, LensParams, Mode, ModeNetwork};

#[test]
fn curiosity_defaults_match_spec_19() {
    let p = CuriosityParams::default();
    assert_eq!((p.base, p.rate), (0.40, 0.10));
}

#[test]
fn g_default_is_c_squared_spec_16() {
    assert_eq!(PowerActivation::default().alpha, 2.0);
}

#[test]
fn affect_defaults_match_spec_12() {
    let p = AffectParams::default();
    assert_eq!(
        (p.v_rate, p.d_threshold, p.d_rise, p.d_recover),
        (0.2, -0.3, 0.01, 0.0005)
    );
}

#[test]
fn kick_defaults_match_spec_19() {
    let k = KickParams::default();
    assert_eq!((k.theta, k.p_base, k.n, k.kappa), (0.9, 0.05, 300, 0.15));
}

#[test]
fn oracle_floor_matches_01_4_1() {
    assert_eq!(Oracle::default().floor, 0.02);
}

#[test]
fn permeability_defaults_match_spec_11() {
    let p = PermeabilityParams::default();
    assert_eq!(
        (
            p.base,
            p.rate,
            p.h_threat,
            p.h_stakes,
            p.h_fatigue,
            p.h_tenacity,
            p.h_routine
        ),
        (0.40, 0.1, 0.3, 0.4, 0.2, 0.4, 0.3)
    );
    assert_eq!((p.choke, p.delta, p.c_leak), (0.6, 0.5, 0.8));
}

#[test]
fn memory_defaults_match_spec_17() {
    let m = MemoryParams::default();
    assert_eq!((m.procedural.lambda, m.procedural.base), (1e-6, 1.0));
    assert_eq!((m.semantic.lambda, m.semantic.base), (1e-5, 0.8));
    assert_eq!((m.episodic.lambda, m.episodic.base), (1e-4, 0.6));
    assert_eq!((m.sensory.lambda, m.sensory.base), (1e-2, 0.2));
    assert_eq!(m.c_r, 0.05);
}

#[test]
fn lens_rates_and_randomness_match_spec_7() {
    let expect = [
        (Mode::Learning, 0.05, 0.5),
        (Mode::Exploring, 0.02, 1.0),
        (Mode::Performing, 0.005, 0.1),
        (Mode::Optimizing, 0.01, 0.2),
    ];
    for (mode, lr, temp) in expect {
        let p = LensParams::default_for(mode);
        assert_eq!((p.base_lr, p.temperature), (lr, temp), "{mode:?}");
        let lens = Lens::with_priors(mode);
        assert_eq!(lens.temperature(), temp);
        assert!(!lens.frozen);
    }
    // 01 §3.1 ordering: Learning learns fastest, Performing slowest.
    let lr = |m| LensParams::default_for(m).base_lr;
    assert!(lr(Mode::Learning) > lr(Mode::Exploring));
    assert!(lr(Mode::Exploring) > lr(Mode::Optimizing));
    assert!(lr(Mode::Optimizing) > lr(Mode::Performing));
}

#[test]
fn signal_weights_match_spec_5() {
    let k = SignalWeights::default();
    assert_eq!(
        (
            k.novelty,
            k.learning_progress,
            k.energy,
            k.threat,
            k.habituation,
            k.social
        ),
        (0.40, 0.30, -0.40, -0.40, -0.30, 0.20)
    );
}

#[test]
fn subconscious_defaults_match_spec_17() {
    let p = SubconsciousParams::default();
    assert_eq!(
        (
            p.b_base,
            p.sigma_min,
            p.flag_threshold,
            p.nudge_threshold,
            p.nudge
        ),
        (4.0, 0.05, 0.5, 0.6, 0.05)
    );
    assert_eq!((p.mix, p.rescore_rate), ([0.4, 0.3, 0.3], 0.05));
}

#[test]
fn brain_defaults_match_spec_19() {
    let b = BrainConfig::default();
    assert!(b.kick_enabled && b.threat_drive && b.horizon.is_none());
    assert_eq!((b.tenacity_ab, b.tenacity_gamma), ([(1.0, 1.0); 4], 0.98));
    assert_eq!(
        (b.oracle_bias, b.oracle_eta, b.oracle_floor),
        ([0.0; 4], 0.05, 0.02)
    );
    assert!(b.lens_priors.is_empty());
    assert_eq!((b.lens_lr, b.lens_frozen), ([1.0; 4], [false; 4]));
    assert_eq!((b.tm_k, b.tm_tau), (7, 50.0));
    assert_eq!((b.beta_u, b.beta_c, b.eps_ext), (0.2, 0.1, 0.20));
    assert_eq!(
        (b.aging_rate, b.age_per_tick, b.age_per_experience),
        (1e-6, 1.0, 0.0)
    );
    assert_eq!(
        (b.alpha, b.tau_scale, b.k_goal, b.rho_pre),
        (2.0, 1.0, 0.5, 0.0)
    );
    assert_eq!(
        (b.thr_l, b.thr_e, b.thr_p),
        ((0.1, 0.4), (0.1, 0.5), (0.3, 0.9))
    );
    assert_eq!((b.g_shape, b.g_k), (GShape::Power, 10.0));
    assert_eq!(
        (b.budget_policy, b.oracle_exp3),
        (BudgetPolicy::RestBoost, None)
    );
    // The nested params are the module defaults pinned above.
    assert_eq!(b.curiosity, CuriosityParams::default());
    assert_eq!(b.k, SignalWeights::default());
    assert_eq!(b.subconscious, SubconsciousParams::default());
}

#[test]
fn version_constants() {
    assert_eq!(cognitive_core::DEFS_VERSION, "01 v2");
    assert_eq!(cognitive_core::SPEC_VERSION, "0.2");
}
