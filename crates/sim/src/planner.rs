//! Scripted planner: the code tool that stands in for the LLM planner when the
//! LLM is off (spec §21). Given the inventory and a goal item, it names the
//! next step. It re-plans from scratch every call, so it never holds stale
//! state.

use host_api::Inventory;

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
}
