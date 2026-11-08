// What is worth crafting at each pickaxe tier (spec §21), worked by hand.

use host_api::Inventory;
use host_api::craft::worth_crafting;

fn offered(items: &[(&str, u32)]) -> Vec<&'static str> {
    let inv = Inventory {
        items: items.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
    };
    worth_crafting(&inv).map(|r| r.output).collect()
}

#[test]
fn a_station_is_offered_until_owned() {
    let start = offered(&[("oak_log", 1), ("oak_planks", 4)]);
    assert_eq!(start, ["oak_planks", "stick", "crafting_table"]);
    let owned = offered(&[("oak_planks", 4), ("crafting_table", 1), ("stick", 2)]);
    assert_eq!(owned, ["stick", "wooden_pickaxe"], "no second table");
}

#[test]
fn only_a_better_pickaxe_is_offered() {
    let wood = offered(&[
        ("wooden_pickaxe", 1),
        ("crafting_table", 1),
        ("oak_planks", 3),
        ("stick", 4),
        ("cobblestone", 3),
    ]);
    assert_eq!(wood, ["stick", "stone_pickaxe"]);
    let stone = offered(&[
        ("stone_pickaxe", 1),
        ("crafting_table", 1),
        ("oak_planks", 3),
        ("stick", 2),
        ("cobblestone", 8),
        ("iron_ingot", 3),
    ]);
    assert_eq!(stone, ["stick", "furnace", "iron_pickaxe"]);
    let iron = offered(&[
        ("iron_pickaxe", 1),
        ("crafting_table", 1),
        ("furnace", 1),
        ("iron_ingot", 3),
        ("stick", 2),
        ("cobblestone", 11),
        ("raw_iron", 1),
    ]);
    assert_eq!(iron, ["iron_ingot"], "no worse pick, no second furnace");
}
