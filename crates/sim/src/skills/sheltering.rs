//! Build a shelter, walk to one, and rest in it (spec §21: rest in a shelter
//! at night counts as sleep).

use host_api::{Candidate, Intensity, Skill, Target, TaskType, World};

use super::{BLOCKED, Ctx, Exec, Handler, Out};
use crate::craft;
use crate::map::Tile;
use crate::world::SimWorld;

/// Blocks that build a shelter quickly, and how many it takes.
const SHELTER_BLOCKS: [&str; 2] = ["cobblestone", "oak_planks"];
const BLOCKS_PER_SHELTER: u32 = 4;

pub(crate) struct Sheltering;

impl Handler for Sheltering {
    fn candidates(&self, w: &SimWorld, cx: &Ctx, out: &mut Out) {
        if cx.sheltered {
            out.add(Skill::Rest, None, TaskType::Building, 0.1, 0.0);
            return;
        }
        let has_blocks = SHELTER_BLOCKS
            .iter()
            .any(|b| w.body.inv.count(b) >= BLOCKS_PER_SHELTER);
        let cost = if has_blocks { 0.1 } else { 0.25 };
        out.add(
            Skill::Shelter,
            None,
            TaskType::Building,
            cost,
            w.risk_at(cx.me),
        );
        let shelter = w
            .nearest_tile(&cx.reach, |t| t == Tile::Shelter)
            .and_then(|(p, _)| cx.reach.steps(p).map(|d| (p, d)));
        if let Some((p, d)) = shelter {
            out.add(
                Skill::MoveTo,
                Some(Target::Pos(p)),
                TaskType::Travel,
                w.cost(d, 0.0),
                w.risk_at(p),
            );
        }
    }

    fn owns(&self, skill: Skill) -> bool {
        matches!(skill, Skill::Shelter | Skill::Rest)
    }

    fn execute(&self, w: &mut SimWorld, a: &Candidate, _: Intensity) -> Exec {
        if a.skill == Skill::Rest {
            return rest(w);
        }
        let here = w.body.pos;
        let blocks = SHELTER_BLOCKS
            .into_iter()
            .find(|b| w.body.inv.count(b) >= BLOCKS_PER_SHELTER);
        let ticks = match blocks {
            Some(b) => {
                craft::take(&mut w.body.inv, b, BLOCKS_PER_SHELTER);
                w.rules.shelter_ticks_blocks
            }
            None => w.rules.shelter_ticks_dig,
        };
        w.step_world(ticks);
        if !w.alive() {
            return (false, 0.0, false);
        }
        w.map.set(here, Tile::Shelter);
        w.notice("block:shelter");
        (true, 1.0, false)
    }
}

/// Rest in a shelter: until dawn at night (at most `rest_ticks`), else
/// `rest_ticks`.
fn rest(w: &mut SimWorld) -> Exec {
    if !w.sheltered() {
        return BLOCKED;
    }
    w.body.resting = true;
    let d = w.rules.day_ticks;
    let to_dawn = (d - w.tick % d) as u32;
    let ticks = if w.is_night() {
        to_dawn.min(w.rules.rest_ticks)
    } else {
        w.rules.rest_ticks
    };
    w.step_world(ticks);
    (true, 1.0, false)
}
