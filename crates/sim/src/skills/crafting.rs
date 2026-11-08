//! Craft what the inventory allows (spec §21 tech tree).

use host_api::{Candidate, Intensity, Skill, Target, TaskType, World};

use super::{BLOCKED, Ctx, Exec, Handler, Out, target_item};
use crate::craft::{self, RECIPES};
use crate::world::SimWorld;

pub(crate) struct Crafting;

impl Handler for Crafting {
    /// Every recipe the inventory allows, skipping stations already owned
    /// and pickaxes no better than the one held.
    fn candidates(&self, w: &SimWorld, _: &Ctx, out: &mut Out) {
        let inv = &w.body.inv;
        let tier = craft::pick_tier(inv);
        for r in RECIPES {
            let owned_station =
                matches!(r.output, "crafting_table" | "furnace") && inv.count(r.output) > 0;
            let worse_pick = match r.output {
                "wooden_pickaxe" => tier >= 1,
                "stone_pickaxe" => tier >= 2,
                "iron_pickaxe" => tier >= 3,
                _ => false,
            };
            if !owned_station && !worse_pick && craft::can_craft(inv, r) {
                out.add(
                    Skill::Craft,
                    Some(Target::Item(r.output.into())),
                    TaskType::Crafting,
                    0.05,
                    0.0,
                );
            }
        }
    }

    fn owns(&self, skill: Skill) -> bool {
        skill == Skill::Craft
    }

    fn execute(&self, w: &mut SimWorld, a: &Candidate, _: Intensity) -> Exec {
        let Some(r) = target_item(a).and_then(craft::recipe) else {
            return BLOCKED;
        };
        if !craft::craft(&mut w.body.inv, r) {
            return BLOCKED;
        }
        w.step_world(w.rules.craft_ticks);
        w.notice(&format!("recipe:{}", r.output));
        w.notice(&format!("item:{}", r.output));
        (true, 1.0, false)
    }
}
