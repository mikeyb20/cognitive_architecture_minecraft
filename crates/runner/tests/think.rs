// LLM thinking tools against a scripted LLM (spec §6, §23).

use std::collections::VecDeque;

use cognitive_core::agent::{Need, ThinkTools};
use cognitive_core::direction::PlanStep;
use host_api::{AgentId, Inventory, Observation, Pos, Seen, SeenKind, Skill, TaskType, ThingId};
use llm::{LlmError, LlmParams, Prompt, Provider, Reply};
use runner::brain::SimTools;
use runner::think::{LlmTools, ThinkConfig, parse_goal, parse_step};

#[derive(Default)]
struct Script {
    replies: VecDeque<Result<Reply, LlmError>>,
    sent: Vec<Prompt>,
}

impl Provider for Script {
    fn id(&self) -> String {
        "script/m".into()
    }
    fn complete(&mut self, p: &Prompt) -> Result<Reply, LlmError> {
        self.sent.push(p.clone());
        self.replies
            .pop_front()
            .unwrap_or(Err(LlmError::Transport("no more".into())))
    }
}

/// A Script that also lets the test read what was sent.
struct Shared<'a>(&'a mut Script);

impl Provider for Shared<'_> {
    fn id(&self) -> String {
        self.0.id()
    }
    fn complete(&mut self, p: &Prompt) -> Result<Reply, LlmError> {
        self.0.complete(p)
    }
}

fn script(texts: &[&str]) -> Script {
    let reply = |t: &&str| {
        Ok(Reply {
            text: (*t).into(),
            input_tokens: 300,
            output_tokens: 10,
            cost_usd: 0.1,
            cached: false,
        })
    };
    Script {
        replies: texts.iter().map(reply).collect(),
        sent: vec![],
    }
}

fn obs() -> Observation {
    let seen = |t: &str, hostile| Seen {
        thing: ThingId(t.into()),
        kind: SeenKind::Entity { hostile },
        pos: Pos { x: 4, y: 64, z: 0 },
        distance: 4.0,
    };
    Observation {
        agent: AgentId(0),
        tick: 100,
        day_phase: 0.25,
        health: 1.0,
        food: 0.9,
        position: Pos { x: 0, y: 64, z: 0 },
        inventory: Inventory {
            items: vec![("oak_log".into(), 2)],
        },
        visible: vec![seen("entity:zombie", true), seen("entity:pig", false)],
        chat: vec![],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

fn step(skill: Skill, thing: &str, task: TaskType) -> Option<PlanStep> {
    Some(PlanStep {
        skill,
        thing: thing.into(),
        task,
    })
}

#[test]
fn steps_are_checked_against_the_skill_list() {
    let o = obs();
    let p = |r: &str| parse_step(r, &o);
    use Skill::*;
    use TaskType::*;
    assert_eq!(p("chop block:oak_log"), step(Chop, "block:oak_log", Mining));
    assert_eq!(p("Mine Block:Stone."), step(Mine, "block:stone", Mining));
    assert_eq!(p("farm block:wheat"), step(Farm, "block:wheat", Farming));
    assert_eq!(p("`craft item:stick`"), step(Craft, "item:stick", Crafting));
    assert_eq!(p("eat item:bread"), step(Eat, "item:bread", Farming));
    assert_eq!(
        p("attack entity:zombie"),
        step(Attack, "entity:zombie", Combat)
    );
    assert_eq!(p("attack entity:pig"), step(Attack, "entity:pig", Farming));
    assert_eq!(p("rest in the bed"), step(Rest, "", Building));
    assert_eq!(p("shelter"), step(Shelter, "", Building));
    assert_eq!(p("place item:dirt"), step(Place, "item:dirt", Building));
    assert_eq!(p("follow_player alex"), step(FollowPlayer, "alex", Helping));
    assert_eq!(p("chat"), step(Chat, "", Helping));
    assert_eq!(p("explore"), step(Explore, "", Travel));
    assert_eq!(p("move_to"), step(MoveTo, "", Travel));
    assert_eq!(p("flee"), step(Flee, "", Travel));
    for bad in [
        "",
        "dance",
        "mine stone",
        "craft item:diamond",
        "craft stick",
        "eat bread",
        "attack zombie",
        "I would chop block:oak_log",
    ] {
        assert_eq!(p(bad), None, "{bad:?}");
    }
}

#[test]
fn goals_must_be_in_the_tech_tree() {
    assert_eq!(
        parse_goal("I'd make an iron_pickaxe."),
        Some("iron_pickaxe".into())
    );
    assert_eq!(parse_goal("item:bread"), Some("bread".into()));
    assert_eq!(parse_goal("raw_iron"), Some("raw_iron".into()));
    assert_eq!(parse_goal("a diamond_sword"), None);
}

#[test]
fn a_good_plan_reply_is_used_and_logged() {
    let mut s = script(&["chop block:oak_log"]);
    let mut t = LlmTools::new(ThinkConfig::default(), Shared(&mut s));
    let got = t.plan("iron_pickaxe", &obs());
    assert_eq!(got, step(Skill::Chop, "block:oak_log", TaskType::Mining));
    assert_eq!(
        (t.stats.calls, t.stats.fallbacks, t.stats.unparsed),
        (1, 0, 0)
    );
    assert!((t.stats.cost_usd - 0.1).abs() < 1e-12);
    let sent = &s.sent[0];
    assert!(sent.system.starts_with("You plan for one player"));
    assert!(sent.user.starts_with("health 100%, food 90%, day"));
    assert!(sent.user.ends_with("\nGoal: iron_pickaxe"));
    assert_eq!(sent.max_tokens, 1000);
}

#[test]
fn up_to_two_replans_then_the_code_planner() {
    let mut s = script(&["hmm", "dance", "mine block:stone"]);
    let mut t = LlmTools::new(ThinkConfig::default(), Shared(&mut s));
    assert_eq!(
        t.plan("iron_pickaxe", &obs()),
        step(Skill::Mine, "block:stone", TaskType::Mining)
    );
    assert_eq!(
        (t.stats.calls, t.stats.unparsed, t.stats.fallbacks),
        (3, 2, 0)
    );
    assert!(s.sent[2].user.ends_with(
        "Goal: iron_pickaxe\nYour answer \"hmm\" was not usable. Try again.\n\
         Your answer \"dance\" was not usable. Try again."
    ));

    let mut s = script(&["a", "b", "c", "chop block:oak_log"]);
    let mut t = LlmTools::new(ThinkConfig::default(), Shared(&mut s));
    let code = SimTools::default().plan("iron_pickaxe", &obs());
    assert_eq!(t.plan("iron_pickaxe", &obs()), code);
    assert_eq!((t.stats.calls, t.stats.fallbacks), (3, 1));
}

#[test]
fn errors_and_the_budget_fall_back_without_spending() {
    let mut t = LlmTools::new(ThinkConfig::default(), script(&[]));
    assert_eq!(
        t.plan("iron_pickaxe", &obs()),
        SimTools::default().plan("iron_pickaxe", &obs())
    );
    assert_eq!((t.stats.calls, t.stats.fallbacks), (0, 1));

    let cfg = ThinkConfig {
        llm: LlmParams {
            cheap_calls_per_hour: 1,
            seconds_per_tick: 0.5,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut s = script(&["shelter", "shelter", "shelter"]);
    let mut t = LlmTools::new(cfg, Shared(&mut s));
    let mut o = obs();
    t.plan("iron_pickaxe", &o);
    o.tick += 3600; // half an hour at 0.5 s a tick
    t.plan("iron_pickaxe", &o);
    o.tick += 3600;
    t.plan("iron_pickaxe", &o);
    assert_eq!((t.stats.calls, t.stats.fallbacks), (2, 1));
    assert_eq!(s.sent.len(), 2);
}

#[test]
fn own_goals_come_from_the_llm_or_the_recombiner() {
    let mut s = script(&["bread"]);
    let mut t = LlmTools::new(ThinkConfig::default(), Shared(&mut s));
    assert_eq!(t.propose_goal(&obs()), Some("bread".into()));
    assert!(s.sent[0].system.starts_with("You choose goals"));
    assert!(s.sent[0].user.ends_with(
        "Items you know how to make: oak_planks, stick, crafting_table, wooden_pickaxe, \
         stone_pickaxe, furnace, iron_ingot, iron_pickaxe, bread"
    ));
    let mut t = LlmTools::new(ThinkConfig::default(), script(&["x", "y", "z"]));
    assert_eq!(
        t.propose_goal(&obs()),
        SimTools::default().propose_goal(&obs())
    );
    assert_eq!((t.stats.unparsed, t.stats.fallbacks), (3, 1));
}

#[test]
fn progress_is_the_code_count() {
    let mut t = LlmTools::new(ThinkConfig::default(), script(&[]));
    let o = obs();
    assert_eq!(
        t.progress("iron_pickaxe", &o),
        SimTools::default().progress("iron_pickaxe", &o)
    );
    assert_eq!(t.stats.calls, 0);
}

#[test]
fn the_teacher_stays_code() {
    let mut t = LlmTools::new(ThinkConfig::default(), script(&[]));
    let o = Observation { food: 0.3, ..obs() };
    let hint = t.teach(Need::Hunger, &o);
    assert_eq!(hint, SimTools::default().teach(Need::Hunger, &o));
    assert_eq!(
        hint.map(|s| (s.skill, s.thing)),
        Some((Skill::Attack, "entity:pig".into()))
    );
    assert_eq!(t.stats.calls, 0);
}

#[test]
fn think_defaults() {
    let c = ThinkConfig::default();
    assert_eq!(c.replans, 2, "spec §23: up to 2 replans");
    assert_eq!(c.llm, LlmParams::default());
}
