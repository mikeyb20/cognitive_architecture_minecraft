//! Spec values that live in host-api, pinned so a change is deliberate
//! (CLAUDE.md: every spec value is a default in a params struct).

use host_api::command::QueueParams;
use host_api::reflex::ReflexThresholds;

#[test]
fn reflex_thresholds_match_spec_20() {
    let t = ReflexThresholds::default();
    assert_eq!((t.critical_health, t.starving_food), (0.2, 0.05));
}

#[test]
fn queue_limits_match_v0_2_step_18_and_20() {
    let p = QueueParams::default();
    assert_eq!((p.max_queued, p.stall_limit, p.come_distance), (5, 30, 3.0));
}
