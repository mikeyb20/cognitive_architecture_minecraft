//! Build a shelter, walk to one, and rest in it (spec §21: rest in a shelter
//! at night counts as sleep).

use host_api::{Candidate, Intensity, Pos, Skill, Target, TaskType, World};

use super::{BLOCKED, Ctx, Exec, Handler, Out};
use crate::craft;
use crate::geom::neighbours4;
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
        if has_blocks || dig_ticks(w).is_some() {
            let cost = if has_blocks { 0.1 } else { 0.25 };
            out.add(
                Skill::Shelter,
                None,
                TaskType::Building,
                cost,
                w.risk_at(cx.me),
            );
        }
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
        let ticks = match (blocks, dig_ticks(w)) {
            (Some(b), _) => {
                craft::take(&mut w.body.inv, b, BLOCKS_PER_SHELTER);
                w.rules.shelter_ticks_blocks
            }
            (None, Some(t)) => t,
            (None, None) => return BLOCKED,
        };
        w.step_world(ticks);
        if !w.alive() {
            return (false, 0.0, false);
        }
        if let Some(b) = blocks {
            wall_in(w, here, b);
        }
        w.map.set(here, Tile::Shelter);
        w.notice("block:shelter");
        (true, 1.0, false)
    }
}

/// Ticks to dig in where the agent stands, if it can (sim audit, 14 Nov
/// 2026): soil (grass, farmland) by hand in `shelter_ticks_dig`; a cave's
/// stone floor only with a pickaxe, as long as mining stone with it; never
/// anywhere else. Minecraft stone needs a pickaxe to dig usefully.
fn dig_ticks(w: &SimWorld) -> Option<u32> {
    match w.map.get(w.body.pos) {
        Tile::Grass | Tile::Wheat => Some(w.rules.shelter_ticks_dig),
        Tile::CaveFloor => {
            let tier = craft::pick_tier(&w.body.inv);
            (tier >= 1).then(|| w.rules.mine_ticks[usize::from(tier - 1)])
        }
        _ => None,
    }
}

/// A hut built with blocks closes every open side of `here` (spec §21, step
/// 13): the first open side gets a door, the others a wall of the blocks
/// used, so the hut passes the shelter test (01 §4.11). A side where a mob
/// stands stays open, as Minecraft won't place a block on a mob.
fn wall_in(w: &mut SimWorld, here: Pos, blocks: &str) {
    let wall = if blocks == "cobblestone" {
        Tile::Stone
    } else {
        Tile::Planks
    };
    let open: Vec<Pos> = neighbours4(here)
        .into_iter()
        .filter(|q| w.map.get(*q).mob_walkable() && !w.mobs.iter().any(|m| m.pos == *q))
        .collect();
    for (i, q) in open.into_iter().enumerate() {
        w.map.set(q, if i == 0 { Tile::Door } else { wall });
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
