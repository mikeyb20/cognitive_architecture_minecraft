//! Arm B's internal actions (spec §24, debt D6): besides the host's options,
//! B may pick **plan** or **reflect**. Either makes one extra LLM call whose
//! answer is kept as a note (`your plan:` or `your lesson:`) in every later
//! prompt, then B picks again from the external options only. Michael chose
//! plan and reflect as notes, without imagine (14 Nov 2026).

use std::collections::VecDeque;

use host_api::Observation;
use llm::Prompt;
use llm::describe;

/// An internal action B can pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Thought {
    Plan,
    Reflect,
}

impl Thought {
    /// Both, in the order they are listed after the external options.
    pub(crate) const ALL: [Thought; 2] = [Thought::Plan, Thought::Reflect];

    /// The option line's text after the id.
    pub(crate) fn option(self) -> &'static str {
        match self {
            Thought::Plan => "plan: think about your next steps toward the goal (no action)",
            Thought::Reflect => "reflect: think about what has and hasn't worked (no action)",
        }
    }

    /// The system prompt for the thinking call.
    pub(crate) fn system(self, goal: &str) -> String {
        let ask = match self {
            Thought::Plan => {
                "In at most two sentences, say what to do next and in what order. \
                 This plan will be shown to you on later turns."
            }
            Thought::Reflect => {
                "In at most two sentences, say what has worked and what hasn't in your \
                 recent choices. This lesson will be shown to you on later turns."
            }
        };
        format!(
            "You control one player in a Minecraft world. Stay alive and work toward: {goal}. {ask}"
        )
    }
}

/// One past choice and how the body changed by the next decision.
#[derive(Debug, Clone, PartialEq)]
struct Step {
    choice: String,
    health: f32,
    food: f32,
    result: Option<String>,
}

/// B's notes and its recent choices.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Mind {
    plan: Option<String>,
    lesson: Option<String>,
    recent: VecDeque<Step>,
}

/// Percent change, as `90%→85%`.
fn change(before: f32, after: f32) -> String {
    let pct = |x: f32| (x * 100.0).round() as i32;
    format!("{}%→{}%", pct(before), pct(after))
}

impl Mind {
    /// Close the last choice's entry with how health and food moved since.
    pub(crate) fn update(&mut self, obs: &Observation) {
        // Called once per decision, before that decision's choice is
        // recorded, so the last entry is always still open here.
        if let Some(last) = self.recent.back_mut() {
            last.result = Some(format!(
                "health {}, food {}",
                change(last.health, obs.health),
                change(last.food, obs.food)
            ));
        }
    }

    /// Record a choice, keeping the `keep` most recent.
    pub(crate) fn chose(&mut self, line: &str, obs: &Observation, keep: usize) {
        let choice = line.split_once(": ").map_or(line, |(_, rest)| rest);
        self.recent.push_back(Step {
            choice: choice.to_string(),
            health: obs.health,
            food: obs.food,
            result: None,
        });
        let excess = self.recent.len().saturating_sub(keep);
        self.recent.drain(..excess);
    }

    /// Keep a thought's answer as a one-line note of at most `chars`.
    pub(crate) fn note(&mut self, t: Thought, text: &str, chars: usize) {
        let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let line: String = line.chars().take(chars).collect();
        if line.is_empty() {
            return;
        }
        match t {
            Thought::Plan => self.plan = Some(line),
            Thought::Reflect => self.lesson = Some(line),
        }
    }

    /// The note lines shown before the options.
    pub(crate) fn notes(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(p) = &self.plan {
            out.push(format!("your plan: {p}"));
        }
        if let Some(l) = &self.lesson {
            out.push(format!("your lesson: {l}"));
        }
        out
    }

    /// The recent choices, oldest first, for the thinking call.
    fn history(&self) -> Vec<String> {
        if self.recent.is_empty() {
            return vec!["recent choices: none yet".into()];
        }
        let mut out = vec!["recent choices:".to_string()];
        for s in &self.recent {
            let result = s.result.as_deref().unwrap_or("just now");
            out.push(format!("- {} → {result}", s.choice));
        }
        out
    }

    /// The prompt for one thinking call.
    pub(crate) fn prompt(
        &self,
        t: Thought,
        goal: &str,
        obs: &Observation,
        max_input: u32,
        max_reply: u32,
    ) -> Prompt {
        let system = t.system(goal);
        let mut tail = self.notes();
        tail.extend(self.history());
        Prompt {
            user: describe::fit(obs, &tail, system.chars().count(), max_input),
            system,
            max_tokens: max_reply,
        }
    }
}
