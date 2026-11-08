//! Arms B and B+N (spec §24, #30): our own minimal LLM loop. Each decision
//! the LLM picks one candidate id from the same list every arm gets; B+N
//! first ranks the list by a count bonus. No C, no modes, no learning.
//!
//! When the budget guard says no (spec §23), the call fails or the reply
//! names no offered id, B takes the cheapest, safest candidate instead and
//! counts the fallback.

mod novelty;
mod prompt;

pub use novelty::Counts;

use host_api::{Candidate, Observation};
use llm::{BudgetGuard, LlmParams, LlmStats, Prompt, Provider, Tier};

use crate::Policy;
use prompt::{candidate_line, chosen_id, user_prompt};

/// Arm B's settings (proposed until the pilot).
#[derive(Debug, Clone, PartialEq)]
pub struct ArmBConfig {
    /// Goal named in the system prompt (arm A's goal too).
    pub goal: String,
    /// `Some(β)` makes this B+N.
    pub novelty: Option<f32>,
    /// Spend tier; sets the hourly call cap.
    pub tier: Tier,
    pub llm: LlmParams,
    /// Cap on each reply, thinking included.
    pub max_reply_tokens: u32,
    /// Seconds per world tick, for the hourly cap (Minecraft: 20 ticks/s).
    pub seconds_per_tick: f64,
}

impl Default for ArmBConfig {
    fn default() -> Self {
        Self {
            goal: "iron_pickaxe".into(),
            novelty: None,
            tier: Tier::Cheap,
            llm: LlmParams::default(),
            max_reply_tokens: 1000,
            seconds_per_tick: 0.05,
        }
    }
}

pub struct ArmB<P> {
    pub config: ArmBConfig,
    llm: P,
    guard: BudgetGuard,
    counts: Counts,
    pub stats: LlmStats,
}

impl<P: Provider> ArmB<P> {
    pub fn new(config: ArmBConfig, llm: P) -> Self {
        let guard = BudgetGuard::new(config.llm.calls_per_hour(config.tier));
        Self {
            config,
            llm,
            guard,
            counts: Counts::default(),
            stats: LlmStats::default(),
        }
    }

    /// Visit counts so far (B+N).
    pub fn counts(&self) -> &Counts {
        &self.counts
    }

    /// The fixed part of every prompt.
    pub fn system(&self) -> String {
        let ranked = if self.config.novelty.is_some() {
            " Options are ranked; novelty is higher for targets you have chosen less often."
        } else {
            ""
        };
        format!(
            "You control one player in a Minecraft world. Stay alive and work toward: {}. \
             Each turn you get your status and a numbered list of options.{ranked} \
             Reply with the number of one option and nothing else.",
            self.config.goal
        )
    }

    /// The prompt for this decision: candidates in the order shown.
    pub fn prompt(&self, obs: &Observation, c: &[Candidate]) -> Prompt {
        let order: Vec<usize> = match self.config.novelty {
            Some(beta) => novelty::ranked(c, &self.counts, beta),
            None => (0..c.len()).collect(),
        };
        let bonus = |x: &Candidate| self.config.novelty.map(|b| self.counts.bonus(x, b));
        let lines: Vec<String> = order
            .iter()
            .map(|&i| candidate_line(obs, &c[i], bonus(&c[i])))
            .collect();
        let system = self.system();
        let cap = self.config.llm.max_input_tokens;
        Prompt {
            user: user_prompt(obs, &lines, system.chars().count(), cap),
            system,
            max_tokens: self.config.max_reply_tokens,
        }
    }

    /// The LLM's pick, or `None` when it was not asked or gave no usable id.
    fn ask(&mut self, obs: &Observation, c: &[Candidate]) -> Option<usize> {
        let now = obs.tick as f64 * self.config.seconds_per_tick;
        if !self.guard.try_spend(now) {
            return None;
        }
        let reply = self.llm.complete(&self.prompt(obs, c)).ok()?;
        self.stats.record(&reply);
        let ids: Vec<u32> = c.iter().map(|x| x.id).collect();
        let id = chosen_id(&reply.text, &ids);
        if id.is_none() {
            self.stats.unparsed += 1;
        }
        c.iter().position(|x| Some(x.id) == id)
    }
}

/// The fallback: lowest cost plus risk, first on ties.
fn cheapest(c: &[Candidate]) -> Option<usize> {
    let worth = |x: &Candidate| x.est_cost + x.est_risk;
    (0..c.len()).min_by(|&a, &b| worth(&c[a]).total_cmp(&worth(&c[b])))
}

impl<P: Provider> Policy for ArmB<P> {
    fn choose(&mut self, obs: &Observation, c: &[Candidate]) -> Option<usize> {
        if c.is_empty() {
            return None;
        }
        let pick = match self.ask(obs, c) {
            Some(i) => i,
            None => {
                self.stats.fallbacks += 1;
                cheapest(c)?
            }
        };
        if self.config.novelty.is_some() {
            self.counts.visit(&c[pick]);
        }
        Some(pick)
    }
}
