//! The tech tree (spec §21): log → planks → sticks → table → wood pick →
//! stone → stone pick → furnace → iron → iron pick, plus bread. Item names
//! and drops follow Minecraft 1.21, so the sim and Minecraft share one tree.

use crate::Inventory;

/// What mining a block puts in the inventory, where it differs from the
/// block's own name (Minecraft 1.21 without Silk Touch). Ores found deeper
/// down (`deepslate_*`) drop the same as their plain form.
pub const DROPS: [(&str, &str); 10] = [
    ("stone", "cobblestone"),
    ("deepslate", "cobbled_deepslate"),
    ("iron_ore", "raw_iron"),
    ("deepslate_iron_ore", "raw_iron"),
    ("coal_ore", "coal"),
    ("deepslate_coal_ore", "coal"),
    ("gold_ore", "raw_gold"),
    ("deepslate_gold_ore", "raw_gold"),
    ("diamond_ore", "diamond"),
    ("deepslate_diamond_ore", "diamond"),
];

/// The item a mined block gives (see [`DROPS`]); the block itself otherwise.
pub fn drop_of(block: &str) -> &str {
    DROPS
        .iter()
        .find(|(b, _)| *b == block)
        .map_or(block, |(_, item)| item)
}

/// A recipe: inputs, output, and whether it needs a crafting table or furnace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recipe {
    pub output: &'static str,
    pub count: u32,
    pub inputs: &'static [(&'static str, u32)],
    pub needs: Option<&'static str>,
}

pub const RECIPES: &[Recipe] = &[
    Recipe {
        output: "oak_planks",
        count: 4,
        inputs: &[("oak_log", 1)],
        needs: None,
    },
    Recipe {
        output: "stick",
        count: 4,
        inputs: &[("oak_planks", 2)],
        needs: None,
    },
    Recipe {
        output: "crafting_table",
        count: 1,
        inputs: &[("oak_planks", 4)],
        needs: None,
    },
    Recipe {
        output: "wooden_pickaxe",
        count: 1,
        inputs: &[("oak_planks", 3), ("stick", 2)],
        needs: Some("crafting_table"),
    },
    Recipe {
        output: "stone_pickaxe",
        count: 1,
        inputs: &[("cobblestone", 3), ("stick", 2)],
        needs: Some("crafting_table"),
    },
    Recipe {
        output: "furnace",
        count: 1,
        inputs: &[("cobblestone", 8)],
        needs: Some("crafting_table"),
    },
    Recipe {
        output: "iron_ingot",
        count: 1,
        inputs: &[("raw_iron", 1)],
        needs: Some("furnace"),
    },
    Recipe {
        output: "iron_pickaxe",
        count: 1,
        inputs: &[("iron_ingot", 3), ("stick", 2)],
        needs: Some("crafting_table"),
    },
    Recipe {
        output: "bread",
        count: 1,
        inputs: &[("wheat", 3)],
        needs: None,
    },
];

pub fn recipe(output: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| r.output == output)
}

pub fn can_craft(inv: &Inventory, r: &Recipe) -> bool {
    r.needs.is_none_or(|n| inv.count(n) > 0) && r.inputs.iter().all(|(i, n)| inv.count(i) >= *n)
}

/// Take `n` of an item; returns false (and changes nothing) if short.
pub fn take(inv: &mut Inventory, item: &str, n: u32) -> bool {
    if inv.count(item) < n {
        return false;
    }
    let mut left = n;
    for (name, count) in inv.items.iter_mut() {
        if name == item {
            let used = (*count).min(left);
            *count -= used;
            left -= used;
        }
    }
    inv.items.retain(|(_, c)| *c > 0);
    true
}

pub fn give(inv: &mut Inventory, item: &str, n: u32) {
    match inv.items.iter_mut().find(|(name, _)| name == item) {
        Some((_, c)) => *c += n,
        None => inv.items.push((item.to_string(), n)),
    }
}

/// Craft if possible; returns whether it happened.
pub fn craft(inv: &mut Inventory, r: &Recipe) -> bool {
    if !can_craft(inv, r) {
        return false;
    }
    for (item, n) in r.inputs {
        take(inv, item, *n);
    }
    give(inv, r.output, r.count);
    true
}

/// Best pickaxe tier held: 0 none, 1 wood, 2 stone, 3 iron.
pub fn pick_tier(inv: &Inventory) -> u8 {
    if inv.count("iron_pickaxe") > 0 {
        3
    } else if inv.count("stone_pickaxe") > 0 {
        2
    } else if inv.count("wooden_pickaxe") > 0 {
        1
    } else {
        0
    }
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
    fn full_tech_tree_from_logs_stone_and_ore() {
        let mut i = inv(&[("oak_log", 4), ("cobblestone", 11), ("raw_iron", 3)]);
        for out in [
            "oak_planks",
            "oak_planks",
            "oak_planks",
            "crafting_table",
            "stick",
            "wooden_pickaxe",
            "stone_pickaxe",
            "furnace",
            "iron_ingot",
            "iron_ingot",
            "iron_ingot",
            "stick",
            "iron_pickaxe",
        ] {
            assert!(craft(&mut i, recipe(out).unwrap()), "could not craft {out}");
        }
        assert_eq!(pick_tier(&i), 3);
        assert_eq!(i.count("cobblestone"), 0);
        assert_eq!(i.count("iron_ingot"), 0);
    }

    #[test]
    fn mined_blocks_drop_what_minecraft_drops() {
        assert_eq!(drop_of("iron_ore"), "raw_iron");
        assert_eq!(drop_of("deepslate_iron_ore"), "raw_iron");
        assert_eq!(drop_of("stone"), "cobblestone");
        assert_eq!(drop_of("deepslate_diamond_ore"), "diamond");
        assert_eq!(drop_of("oak_log"), "oak_log", "most blocks drop themselves");
        assert_eq!(drop_of("raw_iron"), "raw_iron");
    }

    #[test]
    fn needs_station_and_inputs() {
        let mut i = inv(&[("oak_planks", 3), ("stick", 2)]);
        let r = recipe("wooden_pickaxe").unwrap();
        assert!(!can_craft(&i, r), "no table");
        assert!(!craft(&mut i, r));
        assert_eq!(i.count("oak_planks"), 3, "failed craft changes nothing");
        give(&mut i, "crafting_table", 1);
        assert!(craft(&mut i, r));
        assert_eq!(pick_tier(&i), 1);
        assert_eq!(i.count("crafting_table"), 1, "stations are not consumed");
        assert!(recipe("diamond").is_none());
    }

    #[test]
    fn take_and_give() {
        let mut i = inv(&[("wheat", 2), ("wheat", 2)]);
        assert!(!take(&mut i, "wheat", 5));
        assert_eq!(i.count("wheat"), 4);
        assert!(take(&mut i, "wheat", 3));
        assert_eq!(i.count("wheat"), 1);
        assert!(take(&mut i, "wheat", 1));
        assert!(i.items.is_empty(), "empty stacks are removed");
        let mut j = inv(&[("wheat", 1), ("wheat", 5)]);
        assert!(take(&mut j, "wheat", 3));
        assert_eq!(
            j.items,
            vec![("wheat".to_string(), 3)],
            "spans stacks in order"
        );
        give(&mut i, "bread", 1);
        give(&mut i, "bread", 2);
        assert_eq!(i.items, vec![("bread".to_string(), 3)]);
    }

    #[test]
    fn bread_and_pick_tiers() {
        let mut i = inv(&[("wheat", 3)]);
        assert!(craft(&mut i, recipe("bread").unwrap()));
        assert_eq!(i.count("bread"), 1);
        assert_eq!(pick_tier(&i), 0);
        give(&mut i, "stone_pickaxe", 1);
        assert_eq!(pick_tier(&i), 2);
        assert_eq!(recipe("oak_planks").unwrap().count, 4);
        assert_eq!(recipe("stick").unwrap().count, 4);
    }
}
