//! The teacher's "show" and "explain" hints (01 §4.11, #59; spec §27 step
//! 13). The teacher is scaffolding outside the agent. A shown recipe enters
//! the choice for its next step with weight `w_R·(1 − ρ_eff)`, so it fades
//! as the agent develops; the goal's test is kept and never fades.

use crate::direction::PlanStep;
use host_api::Skill;
use serde::{Deserialize, Serialize};

/// A recipe: the steps a teacher shows for a goal, in order (01 §4.11).
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    pub goal: String,
    pub steps: Vec<PlanStep>,
}

/// Spec §16 teacher values (proposed).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TeacherParams {
    /// Ask the teacher to show and explain a goal (01 §4.11). Off is the
    /// teacher-off rerun (plan §5.0).
    pub show: bool,
    /// w_R: choice-score bonus for the recipe's next step at ρ_eff = 0,
    /// the same scale as a felt need's steer (#51).
    pub weight: f32,
}

impl Default for TeacherParams {
    fn default() -> Self {
        Self {
            show: true,
            weight: 1.0,
        }
    }
}

/// The bonus a shown step gets: `w_R·(1 − ρ_eff)` (01 §4.11).
pub fn shown(weight: f32, rho_eff: f32) -> f32 {
    weight * (1.0 - rho_eff.clamp(0.0, 1.0))
}

/// Whether an action (`skill` on target key `key`) is plan step `s`: same
/// skill, and the same target unless the step names none (01 §1.6, §4.11).
pub fn step_matches(s: &PlanStep, skill: Skill, key: Option<&str>) -> bool {
    skill == s.skill && (s.thing.is_empty() || key == Some(s.thing.as_str()))
}

/// A recipe being followed: the next step is `steps[next]` (01 §4.11).
#[derive(Debug, Clone, PartialEq)]
pub struct Lesson {
    pub recipe: Recipe,
    pub next: usize,
}

impl Lesson {
    pub fn new(recipe: Recipe) -> Self {
        Self { recipe, next: 0 }
    }

    /// The step to take now, or `None` once the recipe is done.
    pub fn step(&self) -> Option<&PlanStep> {
        self.recipe.steps.get(self.next)
    }

    /// A successful action that was the next step moves the lesson on.
    pub fn record(&mut self, skill: Skill, key: Option<&str>, success: bool) {
        if success && self.step().is_some_and(|s| step_matches(s, skill, key)) {
            self.next += 1;
        }
    }
}
