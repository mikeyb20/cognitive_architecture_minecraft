//! Learning writes (spec §14, §7.1) and the reflex lesson (spec §20).

use super::*;

#[test]
fn damage_leaves_an_episode_unresolved_even_on_success() {
    for (damage, open) in [(0.1, true), (0.0, false)] {
        let mut b = brain();
        let out = Outcome {
            success: true,
            damage,
            ..Outcome::default()
        };
        learn_at(&mut b, external(out), 0);
        let ep = b.mem.get("ep:0:test").unwrap();
        assert_eq!(b.mem.entry(ep).unresolved, open, "damage {damage}");
    }
}

#[test]
fn an_informative_visit_habituates_the_target_less() {
    let h = |new: bool| {
        let mut b = brain();
        let out = Outcome {
            success: true,
            new_things: if new {
                vec![ThingId("thing:a".into())]
            } else {
                vec![]
            },
            ..Outcome::default()
        };
        let p = Pending {
            target: Some("block:x".into()),
            ..external(out)
        };
        learn_at(&mut b, p, 0);
        let t = b.mem.get("block:x").unwrap();
        let sk = b.mem.get("skill:test:Mining").unwrap();
        (b.mem.habituation(t, 0), b.mem.habituation(sk, 0))
    };
    let (new, old) = (h(true), h(false));
    assert!(new.0 < old.0, "target {new:?} vs {old:?}");
    assert!(new.1 < old.1, "and the skill: {new:?} vs {old:?}");
}

#[test]
fn a_cheaper_route_is_an_optimizing_success() {
    let mut b = brain();
    let o = Mode::Optimizing.index();
    let mut credit = |cost: f32| {
        let p = Pending {
            route: Some(("route:a".into(), cost)),
            ..external(Outcome {
                success: true,
                ..Outcome::default()
            })
        };
        learn_at(&mut b, p, 0);
        b.log.success[o]
    };
    assert_eq!(credit(5.0), 0.0, "first time: nothing to compare");
    assert_eq!(credit(3.0), 1.0, "cheaper");
    assert_eq!(credit(4.0), 0.0, "dearer than the best");
}

#[test]
fn episode_records_are_capped() {
    let mut b = brain();
    for t in 0..(EPISODES_KEPT as u64 + 10) {
        learn_at(&mut b, external(Outcome::default()), t);
    }
    assert_eq!(b.episodes.len(), EPISODES_KEPT);
}

#[test]
fn a_rehearsal_is_a_partial_performing_success() {
    let mut b = brain();
    let p = Pending {
        internal: Some(ThoughtResult {
            success: true,
            ..ThoughtResult::default()
        }),
        ..pending(Choice::Internal(InternalAction::Rehearse))
    };
    learn_at(&mut b, p, 0);
    assert_eq!(b.log.success[Mode::Performing.index()], PARTIAL);
}

#[test]
fn only_positive_reward_marks_memories_useful() {
    // w = O only and no O decision: R = 0 exactly.
    for (route, useful) in [(false, 0.0), (true, 1.0)] {
        let mut b = brain();
        let (id, _) = b.mem.encode(
            "fact",
            crate::memory::Tier::Semantic,
            crate::memory::PriorityInputs::default(),
            0,
            crate::memory::Source::Perception,
        );
        b.tm.add(id, 1.0);
        b.mem.entry_mut(id).last_tm = Some(10);
        if route {
            // A recorded route, then a cheaper one: O succeeds, R > 0.
            let first = Pending {
                route: Some(("r".into(), 5.0)),
                w: [0.0, 0.0, 0.0, 1.0],
                ..external(Outcome {
                    success: true,
                    ..Outcome::default()
                })
            };
            learn_at(&mut b, first, 10);
        }
        let p = Pending {
            route: route.then(|| ("r".into(), 1.0)),
            w: [0.0, 0.0, 0.0, 1.0],
            ..external(Outcome {
                success: true,
                ..Outcome::default()
            })
        };
        learn_at(&mut b, p, 10);
        assert_eq!(b.log.r > 0.0, route, "R = {}", b.log.r);
        assert_eq!(b.mem.entry(id).useful, useful);
    }
}

#[test]
fn the_reflex_lesson_scales_with_the_o_learning_rate() {
    let mut phi = [0.0; N_FEATURES];
    phi[f::RISK] = 1.0;
    let change = |lr_o: f32| {
        let mut config = BrainConfig::default();
        config.lens_lr[Mode::Optimizing.index()] = lr_o;
        let mut b = Brain::new(config, 0);
        let o = Mode::Optimizing.index();
        let before = b.lenses[o].score(&phi);
        b.last_voluntary = Some((0, phi, before));
        b.reflex_fired(50);
        assert!(b.last_voluntary.is_none() && b.log.reflex);
        before - b.lenses[o].score(&phi)
    };
    let (slow, fast) = (change(0.5), change(1.0));
    assert!(slow > 0.0 && slow < fast, "{slow} vs {fast}");
}

#[test]
fn routine_bonus_adds_to_routine() {
    let mut b = brain();
    let base = b.perm_inputs(&test_obs()).routine;
    b.routine_bonus = [0.25; 7];
    assert_eq!(b.perm_inputs(&test_obs()).routine, (base + 0.25).min(1.0));
}
