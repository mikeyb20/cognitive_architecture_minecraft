//! Plan §5.0 recovery (spec §27 step 16): setbacks read from the host's
//! health, recovery from the arm's next work action, for every arm.

use host_api::Skill;
use runner::brain::{BrainResult, run_brain, setup};
use runner::metrics::{RunMetrics, recovery, summarise};
use runner::recovery::{COPING, Setbacks};

#[test]
fn setbacks_by_hand() {
    assert_eq!(
        COPING,
        [Skill::Flee, Skill::Shelter, Skill::Rest, Skill::Eat]
    );
    let mut s = Setbacks::default();
    s.health(1.0, 0);
    s.acted(Skill::Chop, 5);
    assert!(s.recoveries.is_empty(), "work with no setback is nothing");
    s.health(0.8, 10);
    assert_eq!(s.open, Some(10));
    for coping in COPING {
        s.acted(coping, 12);
    }
    assert_eq!(s.open, Some(10), "coping is not work");
    s.acted(Skill::Chop, 40);
    assert_eq!((s.recoveries.clone(), s.open), (vec![30], None));
    s.health(0.8, 50);
    s.health(0.9, 60);
    assert_eq!(s.open, None, "no drop, no setback");
    s.health(0.7, 70);
    s.health(0.6, 80);
    assert_eq!(
        (s.open, s.unrecovered()),
        (Some(80), 1),
        "a drop extends it"
    );
    s.acted(Skill::Mine, 100);
    assert_eq!((s.recoveries.clone(), s.unrecovered()), (vec![30, 20], 0));
}

#[test]
fn every_setback_is_recovered_or_open_and_recoveries_fit_the_run() {
    // Property over pseudo-random health and action sequences.
    let mut x: u64 = 7;
    let mut next = || {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        x >> 33
    };
    for _ in 0..200 {
        let mut s = Setbacks::default();
        let (mut health, mut tick, mut drops) = (1.0f32, 0u64, 0u32);
        s.health(health, tick);
        for _ in 0..50 {
            tick += next() % 20;
            if next() % 2 == 0 {
                let h = (next() % 11) as f32 / 10.0;
                drops += u32::from(h < health && s.open.is_none());
                health = h;
                s.health(h, tick);
            } else {
                s.acted(Skill::ALL[(next() % 14) as usize], tick);
            }
        }
        // A drop with no setback open starts one; each is either closed
        // by work or still open at the end.
        let closed = s.recoveries.len() as u32;
        assert_eq!(closed + s.unrecovered(), drops);
        assert!(s.recoveries.iter().all(|&t| t <= tick));
    }
}

#[test]
fn recovery_metrics_by_hand() {
    let r = BrainResult {
        recoveries: vec![30, 20],
        unrecovered: 1,
        ..BrainResult::default()
    };
    let m = recovery(&r);
    assert_eq!((m.recovery, m.setbacks, m.recovered), (25.0, 3, 2));
    let none = recovery(&BrainResult::default());
    assert!(none.recovery.is_nan() && none.setbacks == 0);
    let run = |recovery, setbacks, recovered| RunMetrics {
        recovery,
        setbacks,
        recovered,
        ..RunMetrics::default()
    };
    let s = summarise(&[run(25.0, 3, 2), run(f32::NAN, 1, 0), run(10.0, 1, 1)]);
    assert_eq!((s.recovery, s.recovered), (17.5, 0.6));
    let s = summarise(&[run(f32::NAN, 0, 0)]);
    assert!(s.recovery.is_nan() && s.recovered.is_nan(), "no setbacks");
}

#[test]
fn a_harsh_run_has_setbacks_for_every_arm_to_recover_from() {
    // The harsh episode hurts the agent (spec §21), so the run sees
    // setbacks; each recovery is a tick count inside the run.
    let mut s = setup("harsh", 0);
    s.max_ticks = sim::scenario::DAY;
    let r = run_brain(s, Default::default(), 0, |_| {});
    let n = r.recoveries.len() as u32 + r.unrecovered;
    assert!(n > 0, "{r:?}");
    assert!(r.recoveries.iter().all(|&t| t <= r.ticks - r.start_tick));
}
