//! Choice helpers (spec §8), log tags and sensing (spec §25).

use super::*;

#[test]
fn near_zero_temperature_picks_the_best() {
    let mut b = brain();
    for _ in 0..20 {
        assert_eq!(b.sample(&[0, 1], &[1.0, 0.5], 0.0), Some(0));
    }
    assert_eq!(b.sample(&[], &[], 1.0), None);
}

#[test]
fn dominant_and_leads_break_ties_toward_the_first() {
    assert_eq!(dominant(&[0.25; 4]), 0);
    assert_eq!(dominant(&[0.1, 0.4, 0.4, 0.1]), 1);
    assert_eq!(dominant(&[0.1, 0.2, 0.3, 0.4]), 3);
    assert!(leads(&[0.25; 4], Mode::Performing));
    assert!(leads(&[0.1, 0.2, 0.6, 0.1], Mode::Performing));
    assert!(!leads(&[0.1, 0.2, 0.3, 0.4], Mode::Performing));
}

fn scored(risk: f32, m_o: f32) -> Scored {
    let mut phi = [0.0; N_FEATURES];
    phi[f::RISK] = risk;
    Scored {
        choice: Choice::External(0),
        phi,
        m: [0.0, 0.0, 0.0, m_o],
        kicked: [0.0; 4],
        push: 0.0,
        need: 0.0,
        shown: 0.0,
    }
}

#[test]
fn hazard_veto_needs_a_negative_o_and_a_safer_choice() {
    let safe = scored(0.0, 0.0);
    assert!(hazard_vetoed(&[safe.clone(), scored(HAZARD, -0.1)], 0));
    assert!(!hazard_vetoed(&[safe.clone(), scored(HAZARD, 0.0)], 0));
    assert!(!hazard_vetoed(&[safe, scored(HAZARD * 0.9, -0.1)], 0));
    let risky = scored(HAZARD, -0.1);
    assert!(!hazard_vetoed(&[risky.clone(), risky], 0), "no safer pick");
}

#[test]
fn preferences_are_theta_dot_x() {
    let mut b = brain();
    let mut x = [0.0; crate::control::oracle::CONTEXT_DIM];
    (x[0], x[1]) = (2.0, 3.0);
    b.oracle.theta[1][0] = 0.5;
    b.oracle.theta[1][1] = -1.0;
    b.oracle.theta[2][1] = 4.0;
    assert_eq!(preferences(&b.oracle, &x), [0.0, -2.0, 12.0, 0.0]);
}

fn cand(skill: Skill, target: Option<Target>) -> Candidate {
    Candidate {
        id: 0,
        skill,
        target,
        task_type: TaskType::Crafting,
        est_cost: 0.0,
        est_risk: 0.0,
        commanded: false,
    }
}

#[test]
fn action_tags_set_skill_kind_task_night_and_hostile_bits() {
    let day = test_obs();
    let base = |kind: u32| 1 << (Skill::Chop as u32) | 1 << (14 + kind) | 1 << (20 + 3);
    let c = cand(Skill::Chop, None);
    for (k, key) in ["block:a", "entity:b", "item:c", "region:d"]
        .iter()
        .enumerate()
    {
        assert_eq!(action_tags(&c, Some(key), &day), base(k as u32), "{key}");
    }
    assert_eq!(action_tags(&c, Some("other"), &day), base(5));
    assert_eq!(action_tags(&c, None, &day), base(5));
    let to_player = cand(Skill::Chop, Some(Target::Player("p".into())));
    assert_eq!(action_tags(&to_player, None, &day), base(4));
    assert_eq!(action_tags(&to_player, Some("block:a"), &day), base(0));
    let mut night = test_obs();
    night.day_phase = 0.6;
    night.visible.push(host_api::Seen {
        thing: ThingId("entity:zombie".into()),
        kind: host_api::SeenKind::Entity { hostile: true },
        pos: host_api::Pos::default(),
        distance: 3.0,
    });
    assert_eq!(action_tags(&c, None, &night), base(5) | 1 << 27 | 1 << 28);
}

#[test]
fn conflict_and_pull_come_from_the_kicked_scores() {
    let mut a = scored(0.0, 0.0);
    a.kicked = [1.0, -0.5, 0.0, 0.0];
    let w = [0.5, 0.5, 0.0, 0.0];
    let mut l = TickLog::default();
    l.fill_choice(&[a.clone()], &[0.7], &w, 2, true, Some(0));
    // parts = w·m|m| = [0.5, −0.125, 0, 0].
    assert_eq!((l.pull, l.conflict), (0.5, 1.0 - 0.375 / 0.625));
    assert_eq!((l.kicks, l.stuck_kick, l.n_choices), (2, true, 1));
    assert_eq!((l.chosen_score, l.w), (0.7, w));
    a.kicked = [0.0; 4];
    l.fill_choice(&[a.clone()], &[0.7], &w, 0, false, Some(0));
    assert_eq!((l.pull, l.conflict), (0.0, 0.0), "no pull, no conflict");
    a.kicked = [0.5, 0.5, 0.0, 0.0];
    l.fill_choice(&[a], &[0.7], &w, 0, false, Some(0));
    assert_eq!(l.conflict, 0.0, "modes agree");
    l.fill_choice(&[], &[], &w, 0, false, None);
    assert_eq!(l.chosen, "idle");
}

#[test]
fn sigma_tiers_are_logged_when_a_hundred_tick_boundary_passes() {
    let mut b = brain();
    let logged = |b: &mut Brain, tick: u64, dt: u64| {
        let mut obs = test_obs();
        obs.tick = tick;
        let s = perceive::Sensed {
            dt,
            ..Default::default()
        };
        b.fill_state(&obs, &s, &[]);
        b.log.sigma_tiers.take().is_some()
    };
    assert!(logged(&mut b, 0, 0), "first tick");
    assert!(!logged(&mut b, 50, 50));
    assert!(logged(&mut b, 120, 70), "crossed 100");
    assert!(!logged(&mut b, 150, 30));
    assert!(logged(&mut b, 200, 1), "landed on 200");
}

#[test]
fn richness_counts_distinct_live_semantic_items_over_k() {
    use crate::memory::{PriorityInputs, Source, Tier};
    let mut b = brain();
    for (key, tier) in [
        ("a", Tier::Semantic),
        ("b", Tier::Semantic),
        ("c", Tier::Procedural),
    ] {
        let (id, _) = b
            .mem
            .encode(key, tier, PriorityInputs::default(), 0, Source::Perception);
        b.tm.add(id, 1.0);
    }
    let s = perceive::Sensed {
        retrieved: vec![0, 0],
        ..Default::default()
    };
    b.fill_state(&test_obs(), &s, &[]);
    assert_eq!(b.log.richness, 2.0 / 7.0);
    assert_eq!(b.log.retrieved, 2);
}

#[test]
fn ticks_to_night_counts_down_the_day() {
    assert_eq!(ticks_to_night(0.0), 1200);
    assert_eq!(ticks_to_night(0.25), 600);
    assert_eq!(ticks_to_night(0.49), 24);
    assert_eq!(ticks_to_night(0.5), 0);
    assert_eq!(ticks_to_night(0.9), 0);
    assert_eq!(DAY_TICKS, 2400.0);
}

#[test]
fn sensing_averages_long_run_c_and_fades_rehearsal() {
    use super::perceive::{C_LONG_TICKS, ROUTINE_FADE};
    let mut b = brain();
    b.sense(&test_obs());
    (b.c_long, b.routine_bonus) = (0.9, [0.5; 7]);
    let mut obs = test_obs();
    obs.tick = 500;
    let s = b.sense(&obs);
    assert_eq!(s.dt, 500);
    let a = 1.0 - (-500.0 / C_LONG_TICKS).exp();
    let want = 0.9 + a * (b.c() - 0.9);
    assert!((b.c_long - want).abs() < 1e-6, "{} vs {want}", b.c_long);
    assert!(b.c_long < 0.9 && b.c_long > b.c());
    // powi and the per-tick product round differently in release builds.
    let faded = 0.5 * ROUTINE_FADE.powi(500);
    assert!(b.routine_bonus.iter().all(|r| (r - faded).abs() < 1e-5));
}

#[test]
fn a_plan_is_valid_until_its_ttl_runs_out() {
    let mut b = brain();
    let mut obs = test_obs();
    b.step = Some(crate::direction::PlanStep {
        skill: Skill::Chop,
        thing: "block:oak_log".into(),
        task: TaskType::Building,
    });
    b.plan_inv = Some(obs.inventory.clone());
    b.plan_tick = 10;
    obs.tick = 10 + PLAN_TTL - 1;
    assert!(b.extras(&obs, 0.0).plan_valid, "last valid tick");
    obs.tick = 10 + PLAN_TTL;
    assert!(!b.extras(&obs, 0.0).plan_valid, "expired");
}

#[test]
fn the_stuck_trigger_flips_the_dominant_modes_scores() {
    let scores = |stuck: bool| {
        let mut b = brain();
        let cands = [
            cand(Skill::Chop, None),
            cand(Skill::Explore, Some(Target::Pos(host_api::Pos::default()))),
        ];
        let s = perceive::Sensed {
            stuck,
            ..Default::default()
        };
        let _ = b.choose(&test_obs(), &cands, &s, &mut NoTools);
        assert_eq!(b.log.stuck_kick, stuck);
        let mut v: Vec<(String, f32)> = b
            .log
            .top5
            .iter()
            .map(|c| (c.action.clone(), c.score))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };
    let (calm, stuck) = (scores(false), scores(true));
    assert_eq!(calm.len(), stuck.len());
    assert!(
        calm.iter()
            .zip(&stuck)
            .any(|(a, b)| (a.1 - b.1).abs() > 1e-6),
        "{calm:?} vs {stuck:?}"
    );
}

#[test]
fn things_first_seen_near_the_end_of_life_get_urgent_priority() {
    use crate::agent::config::Horizon;
    // Same hidden lifespan; the estimate says life ends now (U = 1) or a
    // whole horizon away (U = 0) (spec §15).
    let priority = |estimate: f32| {
        let mut b = Brain::new(
            BrainConfig {
                horizon: Some(Horizon { ticks: 1000.0 }),
                ..BrainConfig::default()
            },
            0,
        );
        b.lifespan = Some((100.0, estimate));
        b.force_state(Some(0.5), Some((-0.8, 0.0)));
        let mut obs = test_obs();
        obs.tick = 100;
        obs.visible = vec![host_api::Seen {
            thing: ThingId("block:x".into()),
            kind: host_api::SeenKind::Block,
            pos: host_api::Pos::default(),
            distance: 1.0,
        }];
        let _ = b.sense(&obs);
        let item = &b.mem.entry(b.mem.get("block:x").unwrap()).item;
        (b.urgency(100), item.priority, item.v_enc)
    };
    let (u0, calm, v) = priority(1100.0);
    let (u1, urgent, _) = priority(100.0);
    assert_eq!((u0, u1, v), (0.0, 1.0, -0.8));
    // P_init's urgency term is 0.2·U·|V_enc| (spec §17) = 0.2 · 1 · 0.8.
    // C_enc moves a little with U before encoding (its 0.1 weight shifts P
    // by under 0.01 here), so the gap is close to 0.16, not exact.
    assert!((urgent - calm - 0.16).abs() < 0.01, "{calm} vs {urgent}");
}
