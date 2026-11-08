//! Scripted planner: the code tool that stands in for the LLM planner when the
//! LLM is off (spec §21). Given the inventory and a goal item, it names the
//! next step. It re-plans from scratch every call, so it never holds stale
//! state.

use host_api::{Inventory, Skill, TaskType};

use crate::craft::{self, pick_tier};

/// Raw materials gathered from the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Log,
    Stone,
    IronOre,
    Wheat,
}

/// The next thing to do toward a goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Craft(&'static str),
    Gather(Resource),
}

fn raw(item: &str) -> Option<(Resource, u8)> {
    match item {
        "oak_log" => Some((Resource::Log, 0)),
        "cobblestone" => Some((Resource::Stone, 1)),
        "iron_ore" => Some((Resource::IronOre, 2)),
        "wheat" => Some((Resource::Wheat, 0)),
        _ => None,
    }
}

/// The pickaxe that unlocks a tier.
fn pick_for(tier: u8) -> &'static str {
    if tier >= 2 {
        "stone_pickaxe"
    } else {
        "wooden_pickaxe"
    }
}

/// Next step to have at least `qty` of `item`, or `None` if already held or
/// the item has no recipe.
pub fn need(inv: &Inventory, item: &str, qty: u32) -> Option<Step> {
    let have = inv.count(item);
    if have >= qty {
        return None;
    }
    if let Some((res, tier)) = raw(item) {
        if pick_tier(inv) < tier {
            return need(inv, pick_for(tier), 1);
        }
        return Some(Step::Gather(res));
    }
    let r = craft::recipe(item)?;
    if let Some(station) = r.needs {
        if let Some(s) = need(inv, station, 1) {
            return Some(s);
        }
    }
    let crafts = (qty - have).div_ceil(r.count);
    for (input, n) in r.inputs {
        if let Some(s) = need(inv, input, n * crafts) {
            return Some(s);
        }
    }
    Some(Step::Craft(r.output))
}

/// Next step toward one `goal` item.
pub fn next_step(inv: &Inventory, goal: &str) -> Option<Step> {
    need(inv, goal, 1)
}

/// A step in shared-ontology terms: the skill, the thing it acts on (memory
/// key) and its task type. This is how the brain sees a plan (spec §6).
pub fn in_ontology(step: Step) -> (Skill, String, TaskType) {
    match step {
        Step::Craft(item) => (Skill::Craft, format!("item:{item}"), TaskType::Crafting),
        Step::Gather(r) => {
            let (skill, block, task) = match r {
                Resource::Log => (Skill::Chop, "oak_log", TaskType::Mining),
                Resource::Stone => (Skill::Mine, "stone", TaskType::Mining),
                Resource::IronOre => (Skill::Mine, "iron_ore", TaskType::Mining),
                Resource::Wheat => (Skill::Farm, "wheat", TaskType::Farming),
            };
            (skill, format!("block:{block}"), task)
        }
    }
}

/// Most steps simulated when counting what is left.
pub const MAX_STEPS: u32 = 200;

/// Steps left to hold `goal`, simulating each step's result. `None` if the
/// goal has no route or takes more than `MAX_STEPS`.
pub fn steps_left(inv: &Inventory, goal: &str) -> Option<u32> {
    let mut inv = inv.clone();
    for n in 0..=MAX_STEPS {
        let Some(step) = next_step(&inv, goal) else {
            return (inv.count(goal) > 0).then_some(n);
        };
        match step {
            Step::Craft(item) => {
                craft::craft(&mut inv, craft::recipe(item)?);
            }
            Step::Gather(r) => {
                let item = match r {
                    Resource::Log => "oak_log",
                    Resource::Stone => "cobblestone",
                    Resource::IronOre => "iron_ore",
                    Resource::Wheat => "wheat",
                };
                craft::give(&mut inv, item, 1);
            }
        }
    }
    None
}

/// Goal progress (spec §13): `1 − left/left_from_nothing`, in [0, 1].
pub fn progress(inv: &Inventory, goal: &str) -> f32 {
    let (Some(left), Some(total)) = (
        steps_left(inv, goal),
        steps_left(&Inventory::default(), goal),
    ) else {
        return 0.0;
    };
    // An empty inventory never holds the goal, so total ≥ 1.
    (1.0 - left as f32 / total.max(1) as f32).clamp(0.0, 1.0)
}

/// The tech tree in order (spec §21); the code recombiner proposes the first
/// one not yet held as an own goal.
pub const TECH_GOALS: [&str; 4] = ["wooden_pickaxe", "stone_pickaxe", "furnace", "iron_pickaxe"];

pub fn next_goal(inv: &Inventory) -> Option<&'static str> {
    TECH_GOALS.into_iter().find(|g| inv.count(g) == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inv(items: &[(&str, u32)]) -> Inventory {
        Inventory {
            items: items.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
        }
    }

    #[test]
    fn empty_inventory_starts_with_logs() {
        assert_eq!(
            next_step(&inv(&[]), "iron_pickaxe"),
            Some(Step::Gather(Resource::Log))
        );
        assert_eq!(
            next_step(&inv(&[]), "bread"),
            Some(Step::Gather(Resource::Wheat))
        );
    }

    #[test]
    fn station_before_inputs_and_tool_before_ore() {
        let i = inv(&[("oak_planks", 4)]);
        assert_eq!(
            next_step(&i, "wooden_pickaxe"),
            Some(Step::Craft("crafting_table"))
        );
        let i = inv(&[("crafting_table", 1), ("stick", 2)]);
        assert_eq!(
            next_step(&i, "stone_pickaxe"),
            Some(Step::Gather(Resource::Log)),
            "needs a wooden pickaxe first, which needs planks"
        );
        let i = inv(&[("crafting_table", 1), ("wooden_pickaxe", 1)]);
        assert_eq!(
            next_step(&i, "furnace"),
            Some(Step::Gather(Resource::Stone))
        );
        let i = inv(&[("crafting_table", 1), ("wooden_pickaxe", 1), ("furnace", 1)]);
        assert_eq!(
            next_step(&i, "iron_ingot"),
            Some(Step::Gather(Resource::Stone)),
            "iron ore needs a stone pickaxe, which needs cobblestone first"
        );
        let i = inv(&[("stone_pickaxe", 1), ("furnace", 1)]);
        assert_eq!(
            next_step(&i, "iron_ingot"),
            Some(Step::Gather(Resource::IronOre))
        );
    }

    #[test]
    fn crafts_when_inputs_are_held_and_counts_batches() {
        let i = inv(&[("oak_log", 1)]);
        assert_eq!(next_step(&i, "oak_planks"), Some(Step::Craft("oak_planks")));
        // 7 planks = 2 crafts = 2 logs; one log is short.
        assert_eq!(need(&i, "oak_planks", 7), Some(Step::Gather(Resource::Log)));
        let i = inv(&[("oak_log", 2)]);
        assert_eq!(need(&i, "oak_planks", 8), Some(Step::Craft("oak_planks")));
        assert_eq!(need(&i, "oak_planks", 9), Some(Step::Gather(Resource::Log)));
        assert_eq!(next_step(&inv(&[("bread", 1)]), "bread"), None);
        assert_eq!(next_step(&i, "diamond"), None);
    }

    #[test]
    fn plan_reaches_iron_pickaxe_with_unlimited_raw_materials() {
        let mut i = Inventory::default();
        let mut steps = 0;
        while let Some(s) = next_step(&i, "iron_pickaxe") {
            match s {
                Step::Gather(Resource::Log) => craft::give(&mut i, "oak_log", 1),
                Step::Gather(Resource::Stone) => craft::give(&mut i, "cobblestone", 1),
                Step::Gather(Resource::IronOre) => craft::give(&mut i, "iron_ore", 1),
                Step::Gather(Resource::Wheat) => panic!("wheat is not on the pickaxe path"),
                Step::Craft(item) => {
                    assert!(craft::craft(&mut i, craft::recipe(item).unwrap()), "{item}");
                }
            }
            steps += 1;
            assert!(steps < 100, "planner looped");
        }
        assert_eq!(pick_tier(&i), 3);
        assert_eq!(steps, 30, "step count pins the plan's shape");
    }

    #[test]
    fn steps_in_ontology_terms() {
        assert_eq!(
            in_ontology(Step::Craft("stick")),
            (Skill::Craft, "item:stick".into(), TaskType::Crafting)
        );
        let g = |r| in_ontology(Step::Gather(r));
        assert_eq!(
            g(Resource::Log),
            (Skill::Chop, "block:oak_log".into(), TaskType::Mining)
        );
        assert_eq!(
            g(Resource::Stone),
            (Skill::Mine, "block:stone".into(), TaskType::Mining)
        );
        assert_eq!(
            g(Resource::IronOre),
            (Skill::Mine, "block:iron_ore".into(), TaskType::Mining)
        );
        assert_eq!(
            g(Resource::Wheat),
            (Skill::Farm, "block:wheat".into(), TaskType::Farming)
        );
    }

    #[test]
    fn steps_left_and_progress() {
        let empty = Inventory::default();
        // Planks: one log, one craft. A wooden pickaxe needs 9 planks (table
        // 4, pickaxe 3, sticks 2) = 3 logs, 3 plank crafts, then table,
        // sticks and pickaxe. Bread: 3 wheat and a craft.
        assert_eq!(steps_left(&empty, "oak_planks"), Some(2));
        assert_eq!(steps_left(&empty, "wooden_pickaxe"), Some(9));
        assert_eq!(steps_left(&empty, "bread"), Some(4));
        assert_eq!(steps_left(&empty, "diamond"), None);
        assert_eq!(progress(&empty, "diamond"), 0.0);
        assert_eq!(progress(&empty, "wooden_pickaxe"), 0.0);
        let mut inv = Inventory::default();
        inv.items.push(("wooden_pickaxe".into(), 1));
        assert_eq!(steps_left(&inv, "wooden_pickaxe"), Some(0));
        assert_eq!(progress(&inv, "wooden_pickaxe"), 1.0);
        let mut part = Inventory::default();
        part.items.push(("oak_log".into(), 3));
        let p = progress(&part, "wooden_pickaxe");
        assert!((p - 3.0 / 9.0).abs() < 1e-6, "{p}");
        let iron = steps_left(&empty, "iron_pickaxe").unwrap();
        assert!(iron > 20 && iron < MAX_STEPS, "{iron}");
    }

    #[test]
    fn next_goal_walks_the_tech_tree() {
        let mut inv = Inventory::default();
        assert_eq!(next_goal(&inv), Some("wooden_pickaxe"));
        for g in TECH_GOALS {
            inv.items.push((g.into(), 1));
        }
        assert_eq!(next_goal(&inv), None);
        inv.items.remove(2);
        assert_eq!(next_goal(&inv), Some("furnace"));
    }
}
