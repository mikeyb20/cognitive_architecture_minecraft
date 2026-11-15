//! The teacher's mob lessons (01 §4.11, #59; Michael, 15 Nov 2026: "zombie:
//! fight; skeleton: hide"). When a mob threatens, the scripted player says
//! what to do (`sim::player::ScriptedPlayer::call_on_threat`); the teacher
//! shows a one-step recipe and explains its test, like the shelter lesson.
//! The recipe's bonus fades with ρ; the test does not. Nothing here forces
//! the choice: the brain can still flee a zombie or fight a skeleton.

use cognitive_core::control::teacher::Recipe;
use cognitive_core::direction::PlanStep;
use host_api::ontology::{FIGHT_ZOMBIE_GOAL, HIDE_SKELETON_GOAL};
use host_api::{Skill, TaskType};
use sim::world::MobTest;

/// The recipe for a mob lesson, if `goal` is one: attack the zombie, or
/// flee (Flee picks a tile out of the skeleton's sight, sim #84).
pub fn recipe(goal: &str) -> Option<Recipe> {
    let (skill, thing, task) = match goal {
        FIGHT_ZOMBIE_GOAL => (Skill::Attack, "entity:zombie", TaskType::Combat),
        HIDE_SKELETON_GOAL => (Skill::Flee, "", TaskType::Travel),
        _ => return None,
    };
    Some(Recipe {
        goal: goal.to_string(),
        steps: vec![PlanStep {
            skill,
            thing: thing.to_string(),
            task,
        }],
    })
}

/// The lesson's test where the agent stands, if `goal` is one: no zombie
/// near (fight), or no skeleton can shoot it (hide).
pub fn test(goal: &str, t: MobTest) -> Option<bool> {
    match goal {
        FIGHT_ZOMBIE_GOAL => Some(!t.zombie_near),
        HIDE_SKELETON_GOAL => Some(!t.shot_at),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_lesson_has_one_step_and_a_test() {
        let fight = recipe(FIGHT_ZOMBIE_GOAL).unwrap();
        assert_eq!(fight.goal, FIGHT_ZOMBIE_GOAL);
        assert_eq!(fight.steps.len(), 1);
        let s = &fight.steps[0];
        assert_eq!(
            (s.skill, s.thing.as_str(), s.task),
            (Skill::Attack, "entity:zombie", TaskType::Combat)
        );
        let hide = recipe(HIDE_SKELETON_GOAL).unwrap();
        let s = &hide.steps[0];
        assert_eq!(
            (s.skill, s.thing.as_str(), s.task),
            (Skill::Flee, "", TaskType::Travel)
        );
        assert_eq!(recipe("shelter"), None);
        let near = MobTest {
            zombie_near: true,
            shot_at: false,
        };
        let shot = MobTest {
            zombie_near: false,
            shot_at: true,
        };
        assert_eq!(test(FIGHT_ZOMBIE_GOAL, near), Some(false));
        assert_eq!(test(FIGHT_ZOMBIE_GOAL, shot), Some(true));
        assert_eq!(test(HIDE_SKELETON_GOAL, shot), Some(false));
        assert_eq!(test(HIDE_SKELETON_GOAL, near), Some(true));
        assert_eq!(test("shelter", near), None);
    }
}
