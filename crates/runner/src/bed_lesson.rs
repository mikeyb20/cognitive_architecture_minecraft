//! The bed lesson (Michael, 15 Nov 2026: "sleeping in a bed at night should
//! also be a priority ... inside some sort of shelter"). Minecraft only:
//! the sim has no sheep or beds, so its runs are unchanged. The shelter
//! recipe the teacher shows (01 §4.11, #59) gains one step before Shelter:
//! hunt a sheep in view for wool, while the agent has no bed and not wool
//! for one. The bot's Shelter makes the bed from that wool and puts it in
//! the home; Rest takes the offered bed. The step fades with ρ like the
//! rest of the recipe.

use cognitive_core::control::teacher::Recipe;
use cognitive_core::direction::PlanStep;
use host_api::{Observation, Skill, TaskType};

/// Wool of one colour a bed takes (Minecraft 1.21's recipe, with three planks).
pub const BED_WOOL: u32 = 3;
const SHEEP: &str = "entity:sheep";

/// The wool step (module doc), if the agent needs wool and a sheep is in view.
pub fn wool_step(obs: &Observation) -> Option<PlanStep> {
    let inv = &obs.inventory;
    let ready = inv.items.iter().any(|(name, _)| {
        let n = inv.count(name);
        (name.ends_with("_bed") && n > 0) || (name.ends_with("_wool") && n >= BED_WOOL)
    });
    let sheep = obs.visible.iter().any(|s| s.thing.0 == SHEEP);
    (!ready && sheep).then(|| PlanStep {
        skill: Skill::Attack,
        thing: SHEEP.to_string(),
        task: TaskType::Farming,
    })
}

/// Put the wool step (if any) just before the recipe's last step, Shelter (01 §4.11).
pub fn add_to(recipe: &mut Recipe, obs: &Observation) {
    if let Some(s) = wool_step(obs) {
        let at = recipe.steps.len().saturating_sub(1);
        recipe.steps.insert(at, s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taught::tests::obs;
    use host_api::{Pos, Seen, SeenKind, ThingId};

    /// A view holding `items`, with `seen` entities in it.
    fn view(items: &[(&str, u32)], seen: &[&str]) -> Observation {
        let mut o = obs(&[], items);
        o.visible = seen
            .iter()
            .map(|t| Seen {
                thing: ThingId(t.to_string()),
                kind: SeenKind::Entity { hostile: false },
                pos: Pos { x: 3, y: 64, z: 0 },
                distance: 3.0,
            })
            .collect();
        o
    }

    fn wants(items: &[(&str, u32)]) -> bool {
        wool_step(&view(items, &[SHEEP, "entity:pig"])).is_some()
    }

    #[test]
    fn hunt_a_sheep_until_there_is_wool_for_a_bed_or_a_bed() {
        let s = wool_step(&view(&[], &["entity:pig", SHEEP])).unwrap();
        assert_eq!(
            (s.skill, s.thing.as_str(), s.task),
            (Skill::Attack, "entity:sheep", TaskType::Farming)
        );
        assert_eq!(
            wool_step(&view(&[], &["entity:pig"])),
            None,
            "no sheep in view"
        );
        assert!(wants(&[("white_wool", 2), ("red_wool", 2)]), "two colours");
        assert!(!wants(&[("lime_wool", 3)]));
        assert!(
            !wants(&[("white_wool", 2), ("white_wool", 1)]),
            "one colour, two stacks"
        );
        assert!(!wants(&[("red_bed", 1)]));
        assert!(wants(&[("red_bed", 0), ("white_wool", 2)]));
        assert!(
            wants(&[("bedrock", 5), ("woolly", 5)]),
            "names that only look alike"
        );
    }

    #[test]
    fn the_step_is_wanted_exactly_without_a_bed_or_three_of_one_wool() {
        // Property over a grid of holdings: wanted iff no bed and no colour at BED_WOOL.
        for bed in 0..3 {
            for white in 0..5 {
                for red in 0..5 {
                    let items = [("white_bed", bed), ("white_wool", white), ("red_wool", red)];
                    let expect = bed == 0 && white.max(red) < BED_WOOL;
                    assert_eq!(wants(&items), expect, "{items:?}");
                }
            }
        }
    }

    #[test]
    fn the_wool_step_goes_just_before_shelter() {
        let shelter = PlanStep {
            skill: Skill::Shelter,
            thing: String::new(),
            task: TaskType::Building,
        };
        let craft = PlanStep {
            skill: Skill::Craft,
            thing: "item:oak_planks".into(),
            task: TaskType::Crafting,
        };
        let mut r = Recipe {
            goal: "shelter".into(),
            steps: vec![craft.clone(), shelter.clone()],
        };
        add_to(&mut r, &view(&[], &[SHEEP]));
        let skills: Vec<Skill> = r.steps.iter().map(|s| s.skill).collect();
        assert_eq!(skills, [Skill::Craft, Skill::Attack, Skill::Shelter]);
        let before = r.clone();
        add_to(&mut r, &view(&[("white_wool", 3)], &[SHEEP]));
        assert_eq!(r, before, "nothing to add");
        let mut empty = Recipe {
            goal: "shelter".into(),
            steps: vec![],
        };
        add_to(&mut empty, &view(&[], &[SHEEP]));
        assert_eq!(empty.steps.len(), 1);
    }
}
