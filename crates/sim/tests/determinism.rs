//! A seed replays exactly, and the body stays in range whatever the agent does.

use host_api::{Host, Intensity, Observation};
use proptest::prelude::*;
use sim::{Rules, SimWorld};

/// Run `steps` decisions, picking candidate `choices[i] % len` each time.
fn trace(seed: u64, choices: &[usize], effort: f32) -> Vec<Observation> {
    let mut w = SimWorld::new(Rules::default(), seed);
    let mut out = Vec::new();
    for &i in choices {
        let o = w.perceive();
        out.push(o.clone());
        if !w.alive() {
            break;
        }
        let c = w.candidate_actions(&o);
        let pick = &c[i % c.len()];
        let before = w.tick;
        let r = w.execute(
            pick,
            Intensity {
                effort,
                focus: 1.0,
                external_drive: false,
            },
        );
        assert_eq!(u64::from(r.duration_ticks), w.tick - before);
        assert!(r.damage >= 0.0 && r.progress >= 0.0 && r.progress <= 1.0);
    }
    out
}

#[test]
fn same_seed_same_run() {
    let choices: Vec<usize> = (0..60).map(|i| i * 7 + 3).collect();
    assert_eq!(trace(11, &choices, 1.0), trace(11, &choices, 1.0));
    assert_ne!(trace(11, &choices, 1.0), trace(12, &choices, 1.0));
}

#[test]
fn worlds_differ_by_seed_but_start_fair() {
    for seed in 0..10 {
        let mut w = SimWorld::new(Rules::default(), seed);
        let o = w.perceive();
        assert_eq!((o.health, o.food, o.tick), (1.0, 1.0, 0));
        assert!(w.map.get(o.position).walkable());
        assert!(o.inventory.items.is_empty());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn body_stays_in_range(seed in 0u64..1000, choices in prop::collection::vec(0usize..40, 1..80), effort in 0.0f32..1.0) {
        for o in trace(seed, &choices, effort) {
            prop_assert!((0.0..=1.0).contains(&o.health));
            prop_assert!((0.0..=1.0).contains(&o.food));
            prop_assert!((0.0..1.0).contains(&o.day_phase));
            prop_assert!(o.damage_taken >= 0.0);
        }
    }
}
