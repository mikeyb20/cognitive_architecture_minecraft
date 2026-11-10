//! Felt needs (01 §1.6; decision #47). Hunger is read from the hunger bar.
//! A felt need does not raise C: it gives the actions that met it before an
//! effort floor, the way a command does (#41). A teacher (the host's
//! planner, through `ThinkTools::teach`) points at the action that meets
//! the need; its hints fade as ρ rises, and what the agent remembers
//! relieved the need takes over.

use super::{Brain, Pending};
use crate::direction::{PlanStep, target_key};
use host_api::{Candidate, Observation, Outcome, Skill};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The needs a teacher can be asked about. Danger and nightfall may follow
/// by their own DEF CHANGEs (01 §1.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    Hunger,
}

/// Spec §16 felt needs (proposed values).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NeedParams {
    /// Felt needs on (the default since #47; off is the "no need" ablation).
    pub on: bool,
    /// Hunger starts below this food level and is full at 0. 0.9 since #54:
    /// above it the body heals (sim `regen_food_min`), so a full bar pays.
    pub food_at: f32,
    /// Relief added each time an action met the need, capped at 1.
    pub relief_step: f32,
    /// The teacher's hints fade as ρ rises (#47); off keeps them at full
    /// strength (an ablation).
    pub fades: bool,
    /// w_N: choice-score bonus per unit of need push (#51), so a need
    /// steers an active agent too; 0 is the "floor only" ablation.
    pub steer: f32,
}

impl Default for NeedParams {
    fn default() -> Self {
        Self {
            on: true,
            food_at: 0.9,
            relief_step: 0.5,
            fades: true,
            steer: 1.0,
        }
    }
}

/// Felt hunger `N = clamp((food_at − food)/food_at, 0, 1)` (01 §1.6).
pub fn hunger(food: f32, food_at: f32) -> f32 {
    ((food_at - food) / food_at.max(f32::EPSILON)).clamp(0.0, 1.0)
}

/// `push = N × max(relief, taught × fade)` (01 §1.6).
pub fn push(need: f32, relief: f32, taught: bool, fade: f32) -> f32 {
    need * relief.max(if taught { fade } else { 0.0 })
}

/// A candidate's choice score with the need's bonus: `score + w_N × push`
/// (01 §1.6 steers choice, #51).
pub fn steered(score: f32, steer: f32, need_push: f32) -> f32 {
    score + steer * need_push
}

/// What the agent remembers relieved a need, per action key
/// (`Skill:target`), in [0, 1] (01 §1.6 relief).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReliefTable(BTreeMap<String, f32>);

impl ReliefTable {
    pub fn get(&self, key: &str) -> f32 {
        self.0.get(key).copied().unwrap_or(0.0)
    }

    /// Raise an action's relief by `step`, capped at 1.
    pub fn learn(&mut self, key: &str, step: f32) {
        let r = self.0.entry(key.to_string()).or_insert(0.0);
        *r = (*r + step).min(1.0);
    }
}

/// An action's key in the relief table.
pub fn relief_key(skill: Skill, target: Option<&str>) -> String {
    format!("{skill:?}:{}", target.unwrap_or(""))
}

/// The need this tick: how strong it is and the teacher's hint.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Felt {
    pub hunger: f32,
    pub hint: Option<PlanStep>,
}

impl Brain {
    /// Read the hunger bar and, while hungry, ask the teacher (01 §1.6).
    pub(crate) fn feel(&self, obs: &Observation, tools: &mut dyn super::ThinkTools) -> Felt {
        let np = self.config.needs;
        let hunger = if np.on {
            hunger(obs.food, np.food_at)
        } else {
            0.0
        };
        let hint = (hunger > 0.0)
            .then(|| tools.teach(Need::Hunger, obs))
            .flatten();
        Felt { hunger, hint }
    }

    /// The push a felt need gives one candidate, in [0, 1].
    pub(crate) fn need_push(&self, obs: &Observation, c: &Candidate, felt: &Felt) -> f32 {
        if felt.hunger <= 0.0 {
            return 0.0;
        }
        let key = target_key(obs, c);
        let taught = felt.hint.as_ref().is_some_and(|s| {
            c.skill == s.skill && (s.thing.is_empty() || key.as_deref() == Some(s.thing.as_str()))
        });
        let fade = if self.config.needs.fades {
            1.0 - self.dev.rho_eff()
        } else {
            1.0
        };
        let relief = self.relief.get(&relief_key(c.skill, key.as_deref()));
        push(felt.hunger, relief, taught, fade)
    }

    /// After an external action: it relieved hunger if the bar rose, or if
    /// it gained an item whose eating relieved hunger before (01 §1.6).
    pub(crate) fn learn_relief(&mut self, p: &Pending, out: &Outcome, obs: &Observation) {
        let Some(skill) = p.skill else {
            return;
        };
        let np = self.config.needs;
        if !np.on {
            return;
        }
        let direct = obs.food > p.food_before;
        let via_item = out.items_gained.items.iter().any(|(item, n)| {
            *n > 0
                && self
                    .relief
                    .get(&relief_key(Skill::Eat, Some(&format!("item:{item}"))))
                    > 0.0
        });
        if direct || via_item {
            let key = relief_key(skill, p.target.as_deref());
            self.relief.learn(&key, np.relief_step);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn hunger_and_push_stay_in_range(
            food in 0.0f32..=1.0, at in 0.01f32..=1.0,
            relief in 0.0f32..=1.0, taught: bool, fade in 0.0f32..=1.0,
        ) {
            let n = hunger(food, at);
            prop_assert!((0.0..=1.0).contains(&n));
            let p = push(n, relief, taught, fade);
            prop_assert!((0.0..=n).contains(&p));
            // A fuller bar never feels hungrier.
            prop_assert!(hunger((food + 0.1).min(1.0), at) <= n);
        }
    }

    #[test]
    fn hunger_and_push_by_hand() {
        assert_eq!(hunger(0.75, 0.75), 0.0);
        assert_eq!(hunger(0.9, 0.75), 0.0);
        assert_eq!(hunger(0.0, 0.75), 1.0);
        assert!((hunger(0.3, 0.75) - 0.6).abs() < 1e-6, "(0.75 − 0.3)/0.75");
        // Taught at full strength, then faded; relief wins when larger.
        assert_eq!(push(0.5, 0.0, true, 1.0), 0.5);
        assert_eq!(push(0.5, 0.0, true, 0.4), 0.2);
        assert_eq!(push(0.5, 0.8, true, 0.4), 0.4);
        assert_eq!(push(0.5, 0.0, false, 1.0), 0.0);
        assert_eq!(push(0.0, 1.0, true, 1.0), 0.0);
        // The bonus: 0.2 + 1.0 × 0.6, and nothing at w_N = 0.
        assert!((steered(0.2, 1.0, 0.6) - 0.8).abs() < 1e-6);
        assert_eq!(steered(0.2, 0.0, 0.6), 0.2);
        assert_eq!(steered(-0.3, 0.5, 0.0), -0.3);
    }

    #[test]
    fn relief_grows_by_steps_to_one() {
        let mut t = ReliefTable::default();
        assert_eq!(t.get("Eat:item:bread"), 0.0);
        t.learn("Eat:item:bread", 0.5);
        assert_eq!(t.get("Eat:item:bread"), 0.5);
        t.learn("Eat:item:bread", 0.5);
        t.learn("Eat:item:bread", 0.5);
        assert_eq!(t.get("Eat:item:bread"), 1.0);
        assert_eq!(relief_key(Skill::Eat, Some("item:bread")), "Eat:item:bread");
        assert_eq!(relief_key(Skill::Rest, None), "Rest:");
    }
}
