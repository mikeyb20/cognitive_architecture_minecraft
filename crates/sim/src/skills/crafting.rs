//! Craft what the inventory allows (spec §21 tech tree).

use host_api::{Candidate, Intensity, Skill, Target, TaskType, World};

use super::{BLOCKED, Ctx, Exec, Handler, Out, target_item};
use crate::craft;
use crate::world::SimWorld;

pub(crate) struct Crafting;

impl Handler for Crafting {
    /// Every recipe the inventory allows, skipping stations already owned
    /// and pickaxes no better than the one held.
    fn candidates(&self, w: &SimWorld, _: &Ctx, out: &mut Out) {
        for r in craft::worth_crafting(&w.body.inv) {
            out.add(
                Skill::Craft,
                Some(Target::Item(r.output.into())),
                TaskType::Crafting,
                0.05,
                0.0,
            );
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
