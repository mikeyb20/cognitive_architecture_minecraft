//! Eat food in hand (spec §21).

use host_api::{Candidate, Intensity, Skill, Target, TaskType};

use super::{BLOCKED, Ctx, Exec, Handler, Out, target_item};
use crate::craft;
use crate::world::{SimWorld, food_value};

pub(crate) struct Eating;

impl Handler for Eating {
    fn candidates(&self, w: &SimWorld, _: &Ctx, out: &mut Out) {
        if w.body.food >= 1.0 {
            return;
        }
        let best = ["bread", "porkchop"]
            .into_iter()
            .find(|f| w.body.inv.count(f) > 0);
        if let Some(f) = best {
            out.add(
                Skill::Eat,
                Some(Target::Item(f.into())),
                TaskType::Farming,
                0.05,
                0.0,
            );
        }
    }

    fn owns(&self, skill: Skill) -> bool {
        skill == Skill::Eat
    }

    fn execute(&self, w: &mut SimWorld, a: &Candidate, _: Intensity) -> Exec {
        let Some(item) = target_item(a) else {
            return BLOCKED;
        };
        let Some(v) = food_value(&w.rules, item) else {
            return BLOCKED;
        };
        if !craft::take(&mut w.body.inv, item, 1) {
            return BLOCKED;
        }
        if !w.wait(w.rules.eat_ticks) {
            craft::give(&mut w.body.inv, item, 1);
            return (false, 0.0, false);
        }
        w.body.food = (w.body.food + v).min(1.0);
        (true, 1.0, false)
    }
}
