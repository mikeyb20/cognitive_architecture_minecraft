//! Learned control in the loop (01 §1.2, #64): which decisions count as
//! taken in pain, and how the next observation scores them.

use super::*;

fn brain_with_control() -> Brain {
    let mut c = BrainConfig::default();
    c.affect.control.enabled = true;
    Brain::new(c, 0)
}

fn decided(b: &mut Brain, skill: Option<Skill>, pain: f32) {
    b.pending = Some(Pending {
        skill,
        ..pending(Choice::External(0))
    });
    b.note_escape(pain);
}

#[test]
fn only_decisions_in_pain_are_scored() {
    let mut b = brain_with_control();
    decided(&mut b, Some(Skill::Flee), 0.0);
    assert_eq!(b.in_pain, None);
    decided(&mut b, Some(Skill::Flee), 0.005);
    assert_eq!(b.in_pain, Some((true, 0.005)));
    decided(&mut b, Some(Skill::Explore), 0.005);
    assert_eq!(b.in_pain, Some((false, 0.005)));
    b.pending = None;
    b.note_escape(0.005);
    assert_eq!(b.in_pain, Some((false, 0.005)), "no action is no escape");
}

#[test]
fn an_escape_that_eases_pain_earns_control() {
    let mut b = brain_with_control();
    decided(&mut b, Some(Skill::Flee), 0.005);
    b.score_escape(0.0025);
    assert!((b.affect.kappa() - 0.2).abs() < 1e-6);
    assert_eq!(b.in_pain, None, "scored once");
    b.score_escape(0.0);
    assert!((b.affect.kappa() - 0.2).abs() < 1e-6, "nothing pending");
    // Pain that does not fall is no relief: κ unchanged by an equal rate.
    decided(&mut b, Some(Skill::Flee), 0.005);
    b.score_escape(0.005);
    assert!(
        (b.affect.kappa() - 0.16).abs() < 1e-6,
        "0.2 + 0.2·(0 − 0.2)"
    );
    // Relief after doing something else takes control away.
    decided(&mut b, Some(Skill::Explore), 0.005);
    b.score_escape(0.0);
    assert_eq!(b.affect.kappa(), 0.0, "0.16 − 0.2 < 0");
}

#[test]
fn pain_rate_is_damage_per_tick() {
    assert_eq!(pain_rate(0.01, 4), 0.0025);
    assert_eq!(pain_rate(0.03, 2), 0.015);
}
