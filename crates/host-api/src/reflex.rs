//! Emergency reflexes (spec §20, decision #42). Only the emergency level is a
//! reflex: flee at critical health and eat when starving. Everything else
//! (sheltering, resting, routine eating, early fleeing) is the agent's choice.
//! Every arm and both worlds use these same rules, so comparisons stay fair.

use crate::{Candidate, Observation, Skill};

/// Thresholds (spec §20; the pilot sets the final values).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReflexThresholds {
    /// Flee when health ≤ this (0.2 = 4 of 20 in Minecraft).
    pub critical_health: f32,
    /// Eat when food ≤ this (0.05 = 1 of 20 in Minecraft).
    pub starving_food: f32,
}

impl Default for ReflexThresholds {
    fn default() -> Self {
        Self {
            critical_health: 0.2,
            starving_food: 0.05,
        }
    }
}

/// Which reflex fired, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reflex {
    Flee,
    Eat,
}

/// Returns the reflex to run instead of the agent's choice, if any.
/// Fleeing needs a hostile in view and the agent not sheltered; eating needs an
/// `eat` candidate (food in the inventory). Fleeing wins when both fire.
pub fn emergency_reflex<'a>(
    obs: &Observation,
    candidates: &'a [Candidate],
    t: ReflexThresholds,
) -> Option<(Reflex, &'a Candidate)> {
    let find = |skill: Skill| candidates.iter().find(|c| c.skill == skill);
    if obs.health <= t.critical_health && !obs.sheltered && obs.nearest_hostile().is_some() {
        if let Some(c) = find(Skill::Flee) {
            return Some((Reflex::Flee, c));
        }
    }
    if obs.food <= t.starving_food {
        if let Some(c) = find(Skill::Eat) {
            return Some((Reflex::Eat, c));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentId, Inventory, Pos, Seen, SeenKind, TaskType, ThingId};

    fn obs(health: f32, food: f32, hostile: bool, sheltered: bool) -> Observation {
        let visible = if hostile {
            vec![Seen {
                thing: ThingId("zombie".into()),
                kind: SeenKind::Entity { hostile: true },
                pos: Pos::default(),
                distance: 3.0,
            }]
        } else {
            vec![]
        };
        Observation {
            agent: AgentId(0),
            tick: 0,
            day_phase: 0.6,
            health,
            food,
            position: Pos::default(),
            inventory: Inventory::default(),
            visible,
            chat: vec![],
            damage_taken: 0.0,
            resting: false,
            sheltered,
        }
    }

    fn cand(id: u32, skill: Skill) -> Candidate {
        Candidate {
            id,
            skill,
            target: None,
            task_type: TaskType::Travel,
            est_cost: 0.1,
            est_risk: 0.0,
            commanded: false,
        }
    }

    fn cands() -> Vec<Candidate> {
        vec![
            cand(1, Skill::Explore),
            cand(2, Skill::Flee),
            cand(3, Skill::Eat),
        ]
    }

    #[test]
    fn flee_at_critical_health_with_hostile() {
        let c = cands();
        let r = emergency_reflex(&obs(0.2, 1.0, true, false), &c, ReflexThresholds::default());
        assert_eq!(r.map(|(k, c)| (k, c.id)), Some((Reflex::Flee, 2)));
    }

    #[test]
    fn no_flee_above_threshold_without_hostile_or_when_sheltered() {
        let c = cands();
        let t = ReflexThresholds::default();
        assert!(emergency_reflex(&obs(0.21, 1.0, true, false), &c, t).is_none());
        assert!(emergency_reflex(&obs(0.1, 1.0, false, false), &c, t).is_none());
        assert!(emergency_reflex(&obs(0.1, 1.0, true, true), &c, t).is_none());
    }

    #[test]
    fn eat_when_starving_if_food_available() {
        let c = cands();
        let t = ReflexThresholds::default();
        let r = emergency_reflex(&obs(1.0, 0.05, false, false), &c, t);
        assert_eq!(r.map(|(k, c)| (k, c.id)), Some((Reflex::Eat, 3)));
        assert!(emergency_reflex(&obs(1.0, 0.06, false, false), &c, t).is_none());
        let no_food = vec![cand(1, Skill::Explore)];
        assert!(emergency_reflex(&obs(1.0, 0.0, false, false), &no_food, t).is_none());
    }

    #[test]
    fn flee_wins_over_eat_and_needs_a_flee_candidate() {
        let c = cands();
        let t = ReflexThresholds::default();
        let r = emergency_reflex(&obs(0.1, 0.0, true, false), &c, t);
        assert_eq!(r.map(|(k, _)| k), Some(Reflex::Flee));
        let no_flee = vec![cand(3, Skill::Eat)];
        let r = emergency_reflex(&obs(0.1, 0.0, true, false), &no_flee, t);
        assert_eq!(r.map(|(k, _)| k), Some(Reflex::Eat));
    }

    #[test]
    fn default_thresholds_match_spec_20() {
        let t = ReflexThresholds::default();
        assert_eq!((t.critical_health, t.starving_food), (0.2, 0.05));
    }

    #[test]
    fn observation_helpers() {
        let o = obs(1.0, 1.0, true, false);
        assert!(o.is_night());
        assert_eq!(o.nearest_hostile(), Some(3.0));
        let mut day = obs(1.0, 1.0, false, false);
        day.day_phase = 0.49;
        assert!(!day.is_night());
        assert_eq!(day.nearest_hostile(), None);
    }
}
