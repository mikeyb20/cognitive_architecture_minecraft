//! Freeze on demand (01 §1.5; spec §27 step 4): high C, V < 0, and opposing
//! mode outputs that cancel. A tempting but dangerous option (new ground next
//! to a hostile) splits Exploring (go) against Optimizing (veto). The kick
//! flips one of them now and then: the "sudden release" (00 §3.5.2).

use cognitive_core::activation::{AffectParams, CuriosityParams};
use cognitive_core::agent::{Brain, BrainConfig, Decision, NoTools, TickLog};
use cognitive_core::control::kick::KickParams;
use cognitive_core::modes::f_sign;
use host_api::{
    AgentId, Candidate, Inventory, Observation, Outcome, Pos, Seen, SeenKind, Skill, Target,
    TaskType, ThingId,
};

fn p(x: i32, z: i32) -> Pos {
    Pos { x, y: 0, z }
}

fn obs(tick: u64) -> Observation {
    Observation {
        agent: AgentId(0),
        tick,
        day_phase: 0.1,
        health: 1.0,
        food: 1.0,
        position: p(0, 0),
        inventory: Inventory::default(),
        visible: vec![Seen {
            thing: ThingId("entity:zombie".into()),
            kind: SeenKind::Entity { hostile: true },
            pos: p(4, 0),
            distance: 4.0,
        }],
        chat: vec![],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

/// New ground in four directions, each next to the danger.
fn cands(tick: u64) -> Vec<Candidate> {
    (0..4)
        .map(|i| Candidate {
            id: i,
            skill: Skill::Explore,
            // Fresh regions every tick, so novelty never wears off.
            target: Some(Target::Pos(p(1000 + 8 * (tick as i32 * 4 + i as i32), 0))),
            task_type: TaskType::Travel,
            est_cost: 0.2,
            est_risk: 0.4,
            commanded: false,
        })
        .collect()
}

fn config(kick: bool) -> BrainConfig {
    BrainConfig {
        curiosity: CuriosityParams {
            base: 0.9,
            rate: 0.1,
        },
        // Hold V where it is put.
        affect: AffectParams {
            v_rate: 0.0,
            d_rise: 0.0,
            ..AffectParams::default()
        },
        oracle_bias: [-2.0, 2.3, -2.0, 2.0],
        oracle_eta: 0.0,
        // Hold the lenses too, so the standoff is the same every tick.
        lens_frozen: [true; 4],
        kick_enabled: kick,
        kick: KickParams {
            theta: 0.7,
            p_base: 0.1,
            ..KickParams::default()
        },
        ..BrainConfig::default()
    }
}

/// Conflict of the best external option: 1 − |Σ w·f(m)| / Σ |w·f(m)|.
fn explore_conflict(row: &TickLog) -> Option<f32> {
    let c = row.top5.iter().find(|c| c.action == "Explore")?;
    let parts: Vec<f32> = (0..4).map(|i| row.w[i] * f_sign(c.m[i])).collect();
    let abs: f32 = parts.iter().map(|x| x.abs()).sum();
    Some(1.0 - parts.iter().sum::<f32>().abs() / abs)
}

fn run(kick: bool, ticks: u64) -> Vec<TickLog> {
    let mut b = Brain::new(config(kick), 3);
    b.force_state(Some(0.9), Some((-0.5, 0.0)));
    let mut rows = Vec::new();
    for t in 0..ticks {
        let c = cands(t);
        if let Decision::Act(..) = b.step(&obs(t), &c, &mut NoTools) {
            // It edges forward and nothing happens either way.
            b.outcome(Outcome {
                success: true,
                progress: 1.0,
                duration_ticks: 1,
                ..Outcome::default()
            });
        }
        rows.push(b.last_log().clone());
    }
    rows
}

#[test]
fn freeze_high_c_negative_v_modes_cancel() {
    let rows = run(false, 40);
    let conflict: Vec<f32> = rows
        .iter()
        .map(|r| explore_conflict(r).expect("explore is scored"))
        .collect();
    for r in &rows {
        assert!(r.c > 0.6, "C stays high: {}", r.c);
        assert!(r.v < 0.0, "V stays negative: {}", r.v);
    }
    assert!(conflict.iter().all(|&k| k > 0.9), "{conflict:?}");
}

#[test]
fn the_kick_releases_a_freeze() {
    let frozen = run(false, 40);
    let kicked = run(true, 40);
    let released = |rows: &[TickLog]| {
        rows.iter()
            .filter(|r| r.kicks > 0 && r.chosen.starts_with("Explore") && r.conflict < 0.3)
            .count()
    };
    assert_eq!(released(&frozen), 0);
    assert!(
        released(&kicked) >= 3,
        "kicks break the tie: {}",
        released(&kicked)
    );
}
