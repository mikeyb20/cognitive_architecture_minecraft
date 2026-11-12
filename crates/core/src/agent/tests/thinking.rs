//! Internal actions: rehearse, consolidate, reflect, imagine, recall and
//! goals from commands (spec §6).

use super::*;

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
    assert!(r.success && r.pruned > 0 && r.o_decision, "{r:?}");
    assert_eq!(b.mem.len(), before - r.pruned as usize);
}

#[test]
fn a_consolidation_that_changes_nothing_is_not_a_decision() {
    let mut b = brain();
    let r = think(&mut b, InternalAction::Consolidate, &mut NoTools);
    assert_eq!(
        (r.success, r.pruned, r.o_decision),
        (true, 0, false),
        "{r:?}"
    );
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
