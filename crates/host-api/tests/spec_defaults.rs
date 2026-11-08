//! Spec values that live in host-api, pinned so a change is deliberate
//! (CLAUDE.md: every spec value is a default in a params struct).

use host_api::command::QueueParams;
use host_api::link::{BotConfig, Limits, SkillConfig};
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

#[test]
fn bot_settings_sent_at_connect() {
    let s = SkillConfig::default();
    assert_eq!(
        (s.search_radius, s.ore_depth, s.follow_range),
        (64, 16, 3),
        "proposed until the pilot"
    );
    assert_eq!((s.follow_ms, s.rest_ms, s.search_step), (5000, 5000, 24));
    let l = Limits::default();
    assert_eq!((l.max_visible_blocks, l.max_per_kind), (32, 4));
    let c = BotConfig::default();
    assert_eq!(c.thresholds, ReflexThresholds::default().into());
}
