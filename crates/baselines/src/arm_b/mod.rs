//! Arms B and B+N (spec §24, #30): our own minimal LLM loop. Each decision
//! the LLM picks one candidate id from the same list every arm gets; B+N
//! first ranks the list by a count bonus. No C, no modes, no learning.
//!
//! B may also pick an internal action, plan or reflect (`think`, spec §24),
//! which keeps a note for later prompts and then picks again.
//!
//! When the budget guard says no (spec §23), the call fails or the reply
//! names no offered id, B takes the cheapest, safest candidate instead and
//! counts the fallback.

pub(crate) mod novelty;
mod prompt;
mod think;

pub use novelty::Counts;

use host_api::{Candidate, Observation};
use llm::{BudgetGuard, LlmParams, LlmStats, Prompt, Provider, Tier};

use crate::Policy;
use prompt::{candidate_line, chosen_id, user_prompt};
use think::{Mind, Thought};

/// B+N's β (spec §24 gives no value; proposed until the pilot).
pub const DEFAULT_BETA: f32 = 1.0;

/// Arm B's settings (proposed until the pilot).
#[derive(Debug, Clone, PartialEq)]
pub struct ArmBConfig {
    /// Goal named in the system prompt (arm A's goal too).
    pub goal: String,
    /// `Some(β)` makes this B+N.
    pub novelty: Option<f32>,
    /// Spend tier; sets the hourly call cap.
    pub tier: Tier,
    /// Input and reply caps, calls per hour, world time.
    pub llm: LlmParams,
    /// Offer plan and reflect after the external options (spec §24, D6).
    pub think: bool,
    /// Longest note kept from a thinking call, in characters.
    pub note_chars: usize,
    /// Recent choices shown to a thinking call.
    pub history: usize,
}

impl Default for ArmBConfig {
    fn default() -> Self {
        Self {
            goal: "iron_pickaxe".into(),
            novelty: None,
            tier: Tier::Cheap,
            llm: LlmParams::default(),
            think: true,
            note_chars: 200,
            history: 5,
        }
    }
}

pub struct ArmB<P> {
    pub config: ArmBConfig,
    llm: P,
    guard: BudgetGuard,
    counts: Counts,
    mind: Mind,
    pub stats: LlmStats,
}

/// What one ask came back with.
enum Pick {
    External(usize),
    Think(Thought),
}

impl<P: Provider> ArmB<P> {
    pub fn new(config: ArmBConfig, llm: P) -> Self {
        let guard = BudgetGuard::new(config.llm.calls_per_hour(config.tier));
        Self {
            config,
            llm,
            guard,
            counts: Counts::default(),
            mind: Mind::default(),
            stats: LlmStats::default(),
        }
    }

    /// The LLM provider, e.g. to read what a scripted one was sent.
    pub fn llm(&self) -> &P {
        &self.llm
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

    /// The prompt for this decision: candidates in the order shown, then
    /// plan and reflect when `think` is on.
    pub fn prompt(&self, obs: &Observation, c: &[Candidate]) -> Prompt {
        self.prompt_with(obs, c, self.config.think)
    }

    /// Ids of the internal options, after the highest external id.
    fn thought_ids(c: &[Candidate]) -> impl Iterator<Item = (u32, Thought)> {
        let next = c.iter().map(|x| x.id + 1).max().unwrap_or(0);
        Thought::ALL.into_iter().zip(next..).map(|(t, id)| (id, t))
    }

    fn prompt_with(&self, obs: &Observation, c: &[Candidate], think: bool) -> Prompt {
        let order: Vec<usize> = match self.config.novelty {
            Some(beta) => novelty::ranked(c, &self.counts, beta),
            None => (0..c.len()).collect(),
        };
        let bonus = |x: &Candidate| self.config.novelty.map(|b| self.counts.bonus(x, b));
        let mut lines: Vec<String> = order
            .iter()
            .map(|&i| candidate_line(obs, &c[i], bonus(&c[i])))
            .collect();
        if think {
            lines.extend(Self::thought_ids(c).map(|(id, t)| format!("{id}: {}", t.option())));
        }
        let system = self.system();
        let cap = self.config.llm.max_input_tokens;
        Prompt {
            user: user_prompt(obs, self.mind.notes(), &lines, system.chars().count(), cap),
            system,
            max_tokens: self.config.llm.max_reply_tokens,
        }
    }

    /// One LLM call within the budget; `None` when not asked or it failed.
    fn call(&mut self, obs: &Observation, prompt: &Prompt) -> Option<String> {
        if !self.guard.try_spend(self.config.llm.seconds_at(obs.tick)) {
            return None;
        }
        let reply = self.llm.complete(prompt).ok()?;
        self.stats.record(&reply);
        Some(reply.text)
    }

    /// The LLM's pick, or `None` when it was not asked or gave no usable id.
    fn ask(&mut self, obs: &Observation, c: &[Candidate], think: bool) -> Option<Pick> {
        let text = self.call(obs, &self.prompt_with(obs, c, think))?;
        let mut ids: Vec<u32> = c.iter().map(|x| x.id).collect();
        let thoughts: Vec<(u32, Thought)> = if think {
            Self::thought_ids(c).collect()
        } else {
            Vec::new()
        };
        ids.extend(thoughts.iter().map(|(id, _)| *id));
        let Some(id) = chosen_id(&text, &ids) else {
            self.stats.unparsed += 1;
            return None;
        };
        match thoughts.iter().find(|(t, _)| *t == id) {
            Some((_, t)) => Some(Pick::Think(*t)),
            None => c.iter().position(|x| x.id == id).map(Pick::External),
        }
    }

    /// Run one thinking call and keep its answer as a note.
    fn think(&mut self, t: Thought, obs: &Observation) {
        let cfg = &self.config;
        let prompt = self.mind.prompt(
            t,
            &cfg.goal,
            obs,
            cfg.llm.max_input_tokens,
            cfg.llm.max_reply_tokens,
        );
        if let Some(text) = self.call(obs, &prompt) {
            self.mind.note(t, &text, self.config.note_chars);
        }
    }

    /// The external pick for this decision, thinking first if B chose to.
    fn decide(&mut self, obs: &Observation, c: &[Candidate]) -> Option<usize> {
        match self.ask(obs, c, self.config.think)? {
            Pick::External(i) => Some(i),
            Pick::Think(t) => {
                self.think(t, obs);
                match self.ask(obs, c, false) {
                    Some(Pick::External(i)) => Some(i),
                    _ => None,
                }
            }
        }
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
        self.mind.update(obs);
        let pick = match self.decide(obs, c) {
            Some(i) => i,
            None => {
                self.stats.fallbacks += 1;
                cheapest(c)?
            }
        };
        if self.config.novelty.is_some() {
            self.counts.visit(&c[pick]);
        }
        let line = candidate_line(obs, &c[pick], None);
        self.mind.chose(&line, obs, self.config.history);
        Some(pick)
    }

    fn llm_stats(&self) -> Option<&LlmStats> {
        Some(&self.stats)
    }
}
