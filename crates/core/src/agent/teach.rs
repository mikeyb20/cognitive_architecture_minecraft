//! Asking the teacher (01 §4.11, #59): while there is a goal, the teacher
//! shows a recipe for it once and explains its test every tick. Both are
//! stored as taught memory items (source `Teacher`); the recipe's next step
//! gets a choice bonus that fades with ρ, the test never fades.

use super::{Brain, Pending};
use crate::control::teacher::{Lesson, shown, step_matches};
use crate::direction::target_key;
use crate::memory::{PriorityInputs, Source, Tier};
use host_api::{Candidate, Observation, Outcome};

impl Brain {
    /// Ask the teacher about the current goal (01 §4.11): a recipe once,
    /// the test every tick. Logs the next step and the test result.
    pub(crate) fn consult_teacher(&mut self, obs: &Observation, tools: &mut dyn super::ThinkTools) {
        let Some(goal) = self.goal.as_ref().map(|g| g.item.clone()) else {
            return;
        };
        if !self.config.teacher.show {
            return;
        }
        if self.shown_for.as_ref() != Some(&goal) {
            self.shown_for = Some(goal.clone());
            if let Some(r) = tools.show(&goal, obs) {
                self.remember_taught(&format!("recipe:{goal}"), Tier::Procedural, obs.tick);
                self.lesson = Some(Lesson::new(r));
            }
        }
        self.log.goal_test = tools.explain(&goal, obs);
        if self.log.goal_test.is_some() {
            self.remember_taught(&format!("test:{goal}"), Tier::Semantic, obs.tick);
        }
        self.log.recipe_step = self
            .lesson
            .as_ref()
            .and_then(Lesson::step)
            .map(|s| format!("{:?}:{}", s.skill, s.thing));
    }

    /// Taught items start from the base priority: they come from the
    /// teacher, not from the agent's state (01 §4.11, §5.3).
    fn remember_taught(&mut self, key: &str, tier: Tier, now: u64) {
        let x = PriorityInputs::default();
        self.mem.encode(key, tier, x, now, Source::Teacher);
    }

    /// The bonus the recipe's next step gives candidate `c`:
    /// `w_R·(1 − ρ_eff)` if `c` is that step, else 0 (01 §4.11).
    pub(crate) fn shown_bonus(&self, obs: &Observation, c: &Candidate) -> f32 {
        let Some(step) = self.lesson.as_ref().and_then(Lesson::step) else {
            return 0.0;
        };
        let key = target_key(obs, c);
        if step_matches(step, c.skill, key.as_deref()) {
            shown(self.config.teacher.weight, self.dev.rho_eff())
        } else {
            0.0
        }
    }

    /// A successful action that was the recipe's next step moves it on.
    pub(crate) fn learn_lesson(&mut self, p: &Pending, out: &Outcome) {
        if let (Some(l), Some(skill)) = (self.lesson.as_mut(), p.skill) {
            l.record(skill, p.target.as_deref(), out.success);
        }
    }
}
