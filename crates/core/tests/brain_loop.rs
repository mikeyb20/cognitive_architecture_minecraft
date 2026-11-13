//! The brain loop (spec §4) against a small scripted world, through the
//! public API only. The world has a log to chop, planks to craft, a pig to
//! hunt, new ground to explore, a player who gives a command, a zombie that
//! hurts, lava at the edge and a night. It is small on purpose: every path of
//! the loop runs, and the golden trace pins what the brain did on each tick.

mod common;

use cognitive_core::activation::GShape;
use cognitive_core::agent::digest::{float_sums, sums_close, trace_digest};
use cognitive_core::agent::{Brain, BrainConfig, BudgetPolicy, Goal, TickLog};
use cognitive_core::control::oracle::Exp3;
use cognitive_core::modes::Mode;
use common::{DAY, busy, in_range, low_commanded, run};

#[test]
fn every_row_stays_in_range() {
    // The OQ sweep variants too: log g(C), fixed budget, EXP3 Oracle.
    let variants = BrainConfig {
        g_shape: GShape::Log,
        budget_policy: BudgetPolicy::Fixed,
        oracle_exp3: Some(Exp3::new(0.1)),
        ..busy()
    };
    for cfg in [BrainConfig::default(), busy(), variants] {
        let mut rows = vec![];
        run(Brain::new(cfg, 7), 3 * DAY, &mut rows);
        for r in &rows {
            assert!(
                in_range(r.c, 0.0, 1.0) && in_range(r.c_target, 0.0, 1.0),
                "{r:?}"
            );
            assert!(in_range(r.g, 0.0, 1.0) && in_range(r.d, 0.0, 1.0));
            assert!(in_range(r.v, -1.0, 1.0) && in_range(r.pi, 0.0, 1.0));
            assert!(in_range(r.fatigue, 0.0, 1.0) && in_range(r.threat, 0.0, 1.0));
            assert!(in_range(r.conflict, 0.0, 1.0 + 1e-6) && in_range(r.u, 0.0, 1.0));
            assert!(in_range(r.w.iter().sum(), 0.999, 1.001), "weights sum to 1");
            assert!(r.w.iter().all(|w| in_range(*w, 0.0, 1.0)));
            assert!(r.top5.len() <= 5 && r.top5.len() <= r.n_choices);
            assert!(r.top5.windows(2).all(|p| p[0].score >= p[1].score));
            assert!(r.success.iter().all(|s| in_range(*s, -1.0, 1.0)));
            assert!(
                r.richness.is_finite() && r.richness >= 0.0,
                "{}",
                r.richness
            );
            assert!(in_range(r.rho_eff, 0.0, 1.0), "{}", r.rho_eff);
            assert!(r.rho_eff >= r.rho, "ρ_eff = max(ρ, ρ_pre)");
        }
    }
}

#[test]
fn rho_tracks_the_useful_share_of_memory() {
    // 01 §4.7 (#61): after each tick ρ is useful retained / retained, so it
    // falls only when useful items are lost or new items are learned.
    let mut b = Brain::new(busy(), 3);
    let mut rows = vec![];
    for _ in 0..DAY / 20 {
        b = run(b, 20, &mut rows).0;
        let m = b.memory();
        let want = m.useful_len() as f32 / m.len().max(1) as f32;
        assert!(m.useful_len() <= m.len());
        assert!(
            (b.last_log().rho - want).abs() < 1e-6,
            "{want} vs {}",
            b.last_log().rho
        );
    }
    assert!(
        rows.iter().any(|r| r.rho > 0.0),
        "some memory became useful"
    );
}

#[test]
fn the_loop_uses_every_part() {
    let mut rows = vec![];
    let (b, n) = run(Brain::new(busy(), 3), 3 * DAY, &mut rows);
    run(low_commanded(3), DAY, &mut rows);
    assert!(n.acts > 100 && n.thoughts > 0, "{n:?}");
    let has = |f: &dyn Fn(&TickLog) -> bool| rows.iter().any(f);
    assert!(has(&|r| r.goal_commanded), "the command became the goal");
    assert!(
        has(&|r| r.goal.as_deref() == Some("stick")),
        "own goal after"
    );
    assert!(has(&|r| r.chosen == "Chat:steve"));
    assert!(has(
        &|r| r.chosen == "Explore:16,8" || r.chosen.starts_with("Explore:")
    ));
    assert!(has(&|r| r.d_goal > 0.0), "progress toward the goal");
    assert!(has(&|r| r.threat > 0.0 && r.nearest_hostile.is_some()));
    assert!(has(&|r| r.social > 0.0));
    assert!(has(&|r| r.health == 0.0), "the brain saw its death");
    assert!(has(&|r| r.success[1] == -1.0), "a blocked trip");
    assert!(has(&|r| r.creative || r.new_items > 0));
    assert!(has(&|r| r.sigma_tiers.is_some()));
    assert!(has(&|r| r.stage > 1), "development moves with experience");
    assert!(b.memory().len() > 20);
    assert!(b.lifespan().is_some());
    assert!(b.lens(Mode::Performing).theta != b.lens(Mode::Learning).theta);
}

#[test]
fn a_command_wins_over_an_own_goal() {
    let mut b = Brain::new(busy(), 1);
    b.set_goal(Some(Goal {
        item: "stick".into(),
        commanded: false,
    }));
    let mut rows = vec![];
    let (b, _) = run(b, 200, &mut rows);
    let g = rows.iter().find(|r| r.tick >= 50).map(|r| r.goal.clone());
    assert_eq!(g, Some(Some("oak_planks".into())));
    assert!(b.goal().is_some());
}

#[test]
fn same_seed_same_trace_and_the_trace_is_pinned() {
    let mut a = vec![];
    let (_, na) = run(Brain::new(busy(), 11), 2 * DAY, &mut a);
    let mut b = vec![];
    let (_, nb) = run(Brain::new(busy(), 11), 2 * DAY, &mut b);
    assert_eq!(trace_digest(&a), trace_digest(&b));
    assert_eq!(na, nb);
    let mut d = vec![];
    run(Brain::new(BrainConfig::default(), 11), DAY, &mut d);
    let mut e = vec![];
    run(low_commanded(11), DAY, &mut e);
    assert!(e.iter().any(|r| r.chosen == "Plan"), "the command plans");
    assert!(e.iter().any(|r| r.plan_step.is_some()));
    assert!(e.iter().any(|r| r.chosen == "idle"), "nothing else starts");
    // Pinned. Change only on a deliberate behaviour change, and say why in
    // the PR (CLAUDE.md "Testing").
    assert_eq!(
        (na.acts, na.idle, na.thoughts, na.reflexes),
        GOLDEN_COUNTS,
        "{na:?}"
    );
    assert_eq!(
        (trace_digest(&a), trace_digest(&d), trace_digest(&e)),
        GOLDEN_HASHES,
        "{:?}",
        (trace_digest(&a), trace_digest(&d), trace_digest(&e))
    );
    for (rows, want) in [
        (&a, GOLDEN_SUMS[0]),
        (&d, GOLDEN_SUMS[1]),
        (&e, GOLDEN_SUMS[2]),
    ] {
        let got = float_sums(rows);
        assert!(sums_close(got, want, 1e-3), "{got:?} vs {want:?}");
    }
}

/// Float sums per golden run (busy, default, low-commanded).
const GOLDEN_SUMS: [[f64; 5]; 3] = [
    [664.7568, 0.0, 338.2163, 549.8511, 603.3524],
    [129.4128, 0.0, 53.9221, 68.0641, 242.1633],
    [112.1142, 0.0, 53.0408, 69.7169, 602.8526],
];

const GOLDEN_COUNTS: (u32, u32, u32, u32) = (715, 0, 8, 2);
const GOLDEN_HASHES: (u64, u64, u64) = (
    8_215_857_933_761_119_103,
    17_938_787_185_883_113_790,
    5_455_370_578_165_343_641,
);

#[test]
fn intensity_is_used() {
    // Effort and focus come from C and the chosen mode (spec §16); a busy
    // brain uses more than the minimum at least once.
    let mut rows = vec![];
    run(Brain::new(busy(), 5), DAY, &mut rows);
    assert!(rows.iter().any(|r| r.effort > 0.0 && r.focus > 0.0));
}

#[test]
fn accessors_report_the_state() {
    let mut rows = vec![];
    let (b, _) = run(Brain::new(busy(), 3), DAY, &mut rows);
    let last = rows.last().unwrap();
    assert_eq!(
        (b.c(), b.d(), b.v(), b.pi()),
        (last.c, last.d, last.v, last.pi)
    );
    assert!(b.pi() > 0.0 && b.pi() < 1.0);
    let o = b.oracle();
    assert_eq!(o.floor, BrainConfig::default().oracle_floor);
    assert_ne!(o.theta, Brain::new(busy(), 3).oracle().theta, "Θ learned");
    assert_eq!(b.lifespan(), LIFESPAN, "hidden lifespan for seed 3");
    assert_eq!(Brain::new(BrainConfig::default(), 3).lifespan(), None);
}

/// Seed 3's hidden lifespan for a 6,000-tick horizon (spec §15). Pinned.
const LIFESPAN: Option<u64> = Some(5094);

#[test]
fn force_state_sets_c_v_and_d() {
    let mut b = Brain::new(BrainConfig::default(), 0);
    b.force_state(Some(0.7), None);
    assert!((b.c() - 0.7).abs() < 1e-6, "{}", b.c());
    assert_eq!((b.v(), b.d()), (0.0, 0.0));
    b.force_state(None, Some((-0.5, 0.6)));
    assert!((b.c() - 0.7).abs() < 1e-6);
    assert_eq!((b.v(), b.d()), (-0.5, 0.6));
}

#[test]
fn variants_change_behaviour() {
    // Each OQ variant is wired in: the trace differs from the default's.
    let base = {
        let mut r = vec![];
        run(Brain::new(busy(), 2), DAY, &mut r);
        trace_digest(&r)
    };
    let cfgs = [
        BrainConfig {
            g_shape: GShape::Log,
            ..busy()
        },
        BrainConfig {
            budget_policy: BudgetPolicy::Fixed,
            ..busy()
        },
        BrainConfig {
            oracle_exp3: Some(Exp3::new(0.1)),
            ..busy()
        },
    ];
    for cfg in cfgs {
        let mut r = vec![];
        let (b, _) = run(Brain::new(cfg.clone(), 2), DAY, &mut r);
        assert_ne!(trace_digest(&r), base, "{cfg:?}");
        if cfg.oracle_exp3.is_some() {
            assert_ne!(b.oracle().exp3.unwrap().log_w, [0.0; 4]);
        }
    }
    assert!(BudgetPolicy::RestBoost.boost(true) && !BudgetPolicy::Fixed.boost(true));
    assert!(!BudgetPolicy::RestBoost.boost(false));
}
