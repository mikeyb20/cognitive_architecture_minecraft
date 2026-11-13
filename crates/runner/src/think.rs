//! LLM thinking tools (spec §6, §23): `plan` and `imagine` ask the LLM when
//! the agent chooses them; the code tools (the scripted planner and the
//! tech-tree recombiner, [`SimTools`]) run instead when the budget guard
//! says no, the call fails, or the replies stay unusable after two replans.
//! Every fallback is counted. Progress is always the code count.

use cognitive_core::agent::{Need, ThinkTools};
use cognitive_core::direction::PlanStep;
use host_api::craft::RECIPES;
use host_api::planner::in_tech_tree;
use host_api::{Observation, SeenKind, Skill, TaskType};
use llm::{BudgetGuard, LlmParams, LlmStats, Prompt, Provider, Tier, describe};

use crate::brain::SimTools;

/// Settings shared by every arm in a comparison (spec §23; proposed until
/// the pilot).
#[derive(Debug, Clone, PartialEq)]
pub struct ThinkConfig {
    pub tier: Tier,
    /// Input and reply caps, calls per hour, world time.
    pub llm: LlmParams,
    /// Extra tries after an unusable plan (spec §23: up to 2 replans).
    pub replans: u32,
}

impl Default for ThinkConfig {
    fn default() -> Self {
        Self {
            tier: Tier::Cheap,
            llm: LlmParams::default(),
            replans: 2,
        }
    }
}

const PLAN_SYSTEM: &str = "You plan for one player in a Minecraft world. Reply with the single \
next step as `skill target` and nothing else. Skills: explore, move_to, mine, chop, craft, place, \
attack, flee, eat, farm, shelter, rest, follow_player, chat. Targets: block:<name> to mine, chop \
or farm; item:<name> to craft or eat; entity:<name> to attack; none for shelter and rest.";

const GOAL_SYSTEM: &str = "You choose goals for one player in a Minecraft world. Reply with the \
name of one item worth making next and nothing else.";

/// The tenacity task a planned step belongs to (#36). Attacking is combat
/// when the target is a hostile in view, hunting otherwise.
fn task_of(skill: Skill, thing: &str, obs: &Observation) -> TaskType {
    let hostile = || {
        obs.visible
            .iter()
            .any(|s| s.thing.0 == thing && s.kind == SeenKind::Entity { hostile: true })
    };
    match skill {
        Skill::Chop | Skill::Mine => TaskType::Mining,
        Skill::Craft => TaskType::Crafting,
        Skill::Farm | Skill::Eat => TaskType::Farming,
        Skill::Attack if hostile() => TaskType::Combat,
        Skill::Attack => TaskType::Farming,
        Skill::Shelter | Skill::Rest | Skill::Place => TaskType::Building,
        Skill::FollowPlayer | Skill::Chat => TaskType::Helping,
        Skill::Explore | Skill::MoveTo | Skill::Flee => TaskType::Travel,
    }
}

/// Words of a reply, lower-cased, without quotes or trailing punctuation.
fn words(reply: &str) -> impl Iterator<Item = String> + '_ {
    reply.split_whitespace().map(|w| {
        w.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | '.' | ',' | ':' | ';'))
            .to_lowercase()
    })
}

/// A plan step from a reply, checked against the skill list (spec §23).
pub fn parse_step(reply: &str, obs: &Observation) -> Option<PlanStep> {
    let mut w = words(reply);
    let first = w.next()?;
    let skill = *Skill::ALL.iter().find(|s| s.name() == first)?;
    let thing = w.next().unwrap_or_default();
    let ok = match skill {
        Skill::Chop | Skill::Mine | Skill::Farm => thing.starts_with("block:"),
        Skill::Craft => thing
            .strip_prefix("item:")
            .is_some_and(|i| RECIPES.iter().any(|r| r.output == i)),
        Skill::Eat => thing.starts_with("item:"),
        Skill::Attack => thing.starts_with("entity:"),
        _ => true,
    };
    let thing = match skill {
        Skill::Shelter | Skill::Rest => String::new(),
        _ => thing,
    };
    ok.then(|| PlanStep {
        task: task_of(skill, &thing, obs),
        skill,
        thing,
    })
}

/// An own goal from a reply: the first word that is in the tech tree.
pub fn parse_goal(reply: &str) -> Option<String> {
    words(reply)
        .map(|w| w.trim_start_matches("item:").to_string())
        .find(|w| in_tech_tree(w))
}

pub struct LlmTools<P> {
    pub config: ThinkConfig,
    llm: P,
    guard: BudgetGuard,
    code: SimTools,
    pub stats: LlmStats,
}

impl<P: Provider> LlmTools<P> {
    pub fn new(config: ThinkConfig, llm: P) -> Self {
        let guard = BudgetGuard::new(config.llm.calls_per_hour(config.tier));
        Self {
            config,
            llm,
            guard,
            code: SimTools::default(),
            stats: LlmStats::default(),
        }
    }

    /// One call, or `None` without spending when the guard or the
    /// provider says no.
    fn ask(&mut self, obs: &Observation, system: &str, tail: &[String]) -> Option<String> {
        if !self.guard.try_spend(self.config.llm.seconds_at(obs.tick)) {
            return None;
        }
        let cap = self.config.llm.max_input_tokens;
        let prompt = Prompt {
            system: system.into(),
            user: describe::fit(obs, tail, system.chars().count(), cap),
            max_tokens: self.config.llm.max_reply_tokens,
        };
        let reply = self.llm.complete(&prompt).ok()?;
        self.stats.record(&reply);
        Some(reply.text)
    }

    /// Ask until a reply parses, up to `1 + replans` times.
    fn ask_until<T>(
        &mut self,
        obs: &Observation,
        system: &str,
        tail: Vec<String>,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Option<T> {
        let mut tail = tail;
        for _ in 0..=self.config.replans {
            let text = self.ask(obs, system, &tail)?;
            if let Some(v) = parse(&text) {
                return Some(v);
            }
            self.stats.unparsed += 1;
            tail.push(format!(
                "Your answer \"{}\" was not usable. Try again.",
                text.trim()
            ));
        }
        None
    }
}

impl<P: Provider> ThinkTools for LlmTools<P> {
    fn plan(&mut self, goal: &str, obs: &Observation) -> Option<PlanStep> {
        let tail = vec![format!("Goal: {goal}")];
        let step = self.ask_until(obs, PLAN_SYSTEM, tail, |r| parse_step(r, obs));
        step.or_else(|| {
            self.stats.fallbacks += 1;
            self.code.plan(goal, obs)
        })
    }

    fn progress(&mut self, goal: &str, obs: &Observation) -> f32 {
        self.code.progress(goal, obs)
    }

    /// The teacher stays code: the LLM is a tool for chosen thinking (#40),
    /// and hints are asked for every hungry tick (01 §1.6).
    fn teach(&mut self, need: Need, obs: &Observation) -> Option<PlanStep> {
        self.code.teach(need, obs)
    }

    fn propose_goal(&mut self, obs: &Observation) -> Option<String> {
        let known: Vec<&str> = RECIPES.iter().map(|r| r.output).collect();
        let tail = vec![format!("Items you know how to make: {}", known.join(", "))];
        let goal = self.ask_until(obs, GOAL_SYSTEM, tail, parse_goal);
        goal.or_else(|| {
            self.stats.fallbacks += 1;
            self.code.propose_goal(obs)
        })
    }
}
