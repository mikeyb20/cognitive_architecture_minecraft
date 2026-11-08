//! Unit tests for the brain loop's internals: learning writes (spec §14,
//! §7.1), the reflex lesson (spec §20) and the choice helpers (spec §8).

use super::choose::{HAZARD, Scored, dominant, hazard_vetoed, leads, preferences};
use super::learn::EPISODES_KEPT;
use super::log::{DAY_TICKS, action_tags, ticks_to_night};
use super::think::{NoTools, ThinkTools, ThoughtResult};
use super::*;
use crate::memory::InternalAction;
use crate::modes::feature as f;
use crate::modes::success::PARTIAL;
use crate::modes::{ModeNetwork, N_FEATURES};
use host_api::{Candidate, Skill, Target, TaskType, ThingId};

fn pending(choice: Choice) -> Pending {
    Pending {
        tick: 0,
        choice,
        phi: [0.0; N_FEATURES],
        m: [0.0; 4],
        w: [0.25; 4],
        x: [0.0; crate::control::oracle::CONTEXT_DIM],
        task: TaskType::Mining,
        target: None,
        route: None,
        skill: None,
        skill_label: "test".into(),
        progress_before: 0.0,
        hazard_vetoed: false,
        tags: 0,
        internal: None,
        outcome: None,
    }
}

fn external(out: Outcome) -> Pending {
    Pending {
        outcome: Some(out),
        ..pending(Choice::External(0))
    }
}

fn learn_at(b: &mut Brain, p: Pending, tick: u64) {
    b.pending = Some(p);
    let mut obs = test_obs();
    obs.tick = tick;
    b.learn(&obs, &mut NoTools);
}

fn brain() -> Brain {
    Brain::new(BrainConfig::default(), 0)
}

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
        b.mem.habituation(t, 0)
    };
    assert!(h(true) < h(false), "{} vs {}", h(true), h(false));
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
        commanded: false,
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

fn encode(b: &mut Brain, key: &str, tier: crate::memory::Tier) -> usize {
    use crate::memory::{PriorityInputs, Source};
    let x = PriorityInputs::default();
    b.mem.encode(key, tier, x, 0, Source::Perception).0
}

fn think(b: &mut Brain, a: InternalAction, tools: &mut dyn ThinkTools) -> ThoughtResult {
    b.think(a, 0.5, &test_obs(), tools)
}

#[test]
fn rehearsal_needs_a_step_and_boosts_its_task() {
    let mut b = brain();
    assert!(!think(&mut b, InternalAction::Rehearse, &mut NoTools).success);
    b.step = Some(crate::direction::PlanStep {
        skill: Skill::Craft,
        thing: "stick".into(),
        task: TaskType::Crafting,
    });
    assert!(think(&mut b, InternalAction::Rehearse, &mut NoTools).success);
    assert!(b.routine_bonus[TaskType::Crafting.index()] > 0.0);
}

#[test]
fn consolidation_reports_what_it_pruned() {
    let mut b = brain();
    for i in 0..30 {
        encode(&mut b, &format!("weak:{i}"), crate::memory::Tier::Episodic);
    }
    let mut obs = test_obs();
    obs.tick = 1_000_000;
    let before = b.mem.len();
    let r = b.think(InternalAction::Consolidate, 1.0, &obs, &mut NoTools);
    assert!(r.success && r.pruned > 0, "{r:?}");
    assert_eq!(b.mem.len(), before - r.pruned as usize);
}

#[test]
fn reflection_links_facts_only_when_there_is_something_to_link() {
    use crate::memory::Tier;
    let mut b = brain();
    let ep = encode(&mut b, "ep:a", Tier::Episodic);
    b.episodes.push_back(EpisodeRecord {
        mem_id: ep,
        skill: "Explore".into(),
        target: None,
        success: true,
        gained: vec![],
    });
    let r = think(&mut b, InternalAction::Reflect, &mut NoTools);
    assert_eq!((r.success, r.new_items, r.new_linked), (true, 1, 0));
    assert!(b.mem.get("fact:Explore:none:ok").is_some());
    let ep = encode(&mut b, "ep:b", Tier::Episodic);
    let t = encode(&mut b, "block:oak", Tier::Semantic);
    b.episodes.push_back(EpisodeRecord {
        mem_id: ep,
        skill: "Chop".into(),
        target: Some(t),
        success: false,
        gained: vec![],
    });
    let r = think(&mut b, InternalAction::Reflect, &mut NoTools);
    assert_eq!((r.new_items, r.new_linked), (1, 1));
    let fact = b.mem.get("fact:Chop:block:oak:fail").unwrap();
    assert!(b.mem.linked(fact, t));
}

struct Propose;

impl ThinkTools for Propose {
    fn plan(&mut self, _: &str, _: &Observation) -> Option<crate::direction::PlanStep> {
        None
    }
    fn progress(&mut self, _: &str, _: &Observation) -> f32 {
        0.0
    }
    fn propose_goal(&mut self, _: &Observation) -> Option<String> {
        Some("bread".into())
    }
}

#[test]
fn imagining_a_goal_links_it_to_the_known_item() {
    let mut b = brain();
    let item = encode(&mut b, "item:bread", crate::memory::Tier::Semantic);
    let r = think(&mut b, InternalAction::Imagine, &mut Propose);
    assert_eq!((r.success, r.new_items, r.new_linked), (true, 1, 1));
    let goal = b.mem.get("goal:bread").unwrap();
    assert!(b.mem.linked(goal, item));
    assert_eq!(b.goal.as_ref().map(|g| g.item.as_str()), Some("bread"));
    let mut fresh = brain();
    let r = think(&mut fresh, InternalAction::Imagine, &mut Propose);
    assert_eq!((r.new_items, r.new_linked), (1, 0), "no item to link to");
}

#[test]
fn imagining_joins_an_unlinked_pair_into_an_idea() {
    use crate::memory::{PriorityInputs, Tier, initial_priority};
    let mut b = brain();
    b.force_state(Some(0.8), None);
    b.goal = Some(Goal {
        item: "stick".into(),
        commanded: false,
    });
    let a = encode(&mut b, "a", Tier::Semantic);
    let c = encode(&mut b, "c", Tier::Semantic);
    b.tm.add(a, 1.0);
    b.tm.add(c, 0.5);
    let r = think(&mut b, InternalAction::Imagine, &mut Propose);
    assert_eq!((r.success, r.new_items, r.new_linked), (true, 1, 1));
    let idea = b.mem.get("idea:a+c").unwrap();
    assert!(b.mem.linked(idea, a) && b.mem.linked(idea, c));
    let want = initial_priority(PriorityInputs {
        c_enc: b.c(),
        connection: 0.4,
        ..PriorityInputs::default()
    });
    assert_eq!(b.mem.entry(idea).item.priority, want);
    assert!(want > initial_priority(PriorityInputs::default()));
}

#[test]
fn unlinked_pair_skips_linked_procedural_and_already_imagined_pairs() {
    use crate::memory::Tier;
    let mut b = brain();
    let a = encode(&mut b, "a", Tier::Semantic);
    let p = encode(&mut b, "p", Tier::Procedural);
    b.tm.add(a, 1.0);
    b.tm.add(p, 0.9);
    assert_eq!(b.unlinked_pair(), None, "one semantic item is not a pair");
    let c = encode(&mut b, "c", Tier::Semantic);
    b.tm.add(c, 0.8);
    assert_eq!(b.unlinked_pair(), Some((a, c, "idea:a+c".into())));
    let idea = encode(&mut b, "idea:a+c", Tier::Semantic);
    assert_eq!(b.unlinked_pair(), None, "the idea exists");
    b.mem.prune(idea);
    assert_eq!(
        b.unlinked_pair(),
        Some((a, c, "idea:a+c".into())),
        "dead idea"
    );
    b.mem.link(a, c, 0.1);
    assert_eq!(b.unlinked_pair(), None, "already linked");
}

#[test]
fn recall_links_only_live_items_in_thought_momentum() {
    use crate::memory::Tier;
    let mut b = brain();
    let a = encode(&mut b, "a", Tier::Semantic);
    let c = encode(&mut b, "c", Tier::Semantic);
    let dead = encode(&mut b, "dead", Tier::Semantic);
    for id in [a, c, dead] {
        b.tm.add(id, 1.0);
    }
    b.mem.prune(dead);
    b.sense(&test_obs());
    assert!(b.mem.linked(a, c));
    // `linked` hides dead items, so look at the raw link lists.
    let raw = |id: usize| {
        b.mem
            .entry(id)
            .links
            .iter()
            .map(|l| l.0)
            .collect::<Vec<_>>()
    };
    assert!(!raw(a).contains(&dead) && !raw(c).contains(&dead));
    assert!(raw(dead).is_empty());
}

#[test]
fn goals_from_commands_reuse_the_last_command_until_it_is_held() {
    let cfg = BrainConfig {
        goals_from_commands: true,
        ..BrainConfig::default()
    };
    let mut b = Brain::new(cfg, 0);
    let mut o = test_obs();
    o.chat = vec![host_api::ChatMsg {
        from: "alex".into(),
        text: "get stick".into(),
    }];
    b.sense(&o);
    assert_eq!(b.last_command.as_deref(), Some("stick"));
    b.set_goal(None);
    o.inventory.items = vec![("stick".into(), 0)];
    b.think(InternalAction::Imagine, 0.5, &o, &mut Propose);
    assert_eq!(
        b.goal,
        Some(Goal {
            item: "stick".into(),
            commanded: false
        })
    );
    // Held: the tools propose instead. Off: the command is ignored.
    b.set_goal(None);
    o.inventory.items = vec![("stick".into(), 1)];
    b.think(InternalAction::Imagine, 0.5, &o, &mut Propose);
    assert_eq!(b.goal.as_ref().map(|g| g.item.as_str()), Some("bread"));
    let mut off = brain();
    off.sense(&test_obs());
    off.last_command = Some("stick".into());
    think(&mut off, InternalAction::Imagine, &mut Propose);
    assert_eq!(off.goal.as_ref().map(|g| g.item.as_str()), Some("bread"));
}
