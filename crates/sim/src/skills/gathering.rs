//! Chop, mine and farm the nearest resource in view (spec §21 tech tree).

use host_api::{Candidate, Intensity, Pos, Skill, Target, TaskType, World};

use super::{BLOCKED, Ctx, Exec, Handler, Out, target_pos};
use crate::craft;
use crate::map::Tile;
use crate::world::SimWorld;

/// (skill, task type, tile kind, pickaxe tier needed), in candidate order.
const RESOURCES: [(Skill, TaskType, Tile, u8); 4] = [
    (
        Skill::Chop,
        TaskType::Mining,
        Tile::Tree { chops_left: 0 },
        0,
    ),
    (Skill::Mine, TaskType::Mining, Tile::Stone, 1),
    (Skill::Mine, TaskType::Mining, Tile::IronOre, 2),
    (Skill::Farm, TaskType::Farming, Tile::Wheat, 0),
];

pub(crate) struct Gathering;

impl Handler for Gathering {
    fn candidates(&self, w: &SimWorld, cx: &Ctx, out: &mut Out) {
        let tier = craft::pick_tier(&w.body.inv);
        for (skill, task, kind, need) in RESOURCES {
            if tier < need {
                continue;
            }
            let want = |t: Tile| std::mem::discriminant(&t) == std::mem::discriminant(&kind);
            if let Some((p, d)) = w.nearest_tile(&cx.reach, want) {
                out.add(
                    skill,
                    Some(Target::Pos(p)),
                    task,
                    w.cost(d, 0.1),
                    w.risk_at(p),
                );
            }
        }
    }

    fn owns(&self, skill: Skill) -> bool {
        matches!(skill, Skill::Chop | Skill::Mine | Skill::Farm)
    }

    fn execute(&self, w: &mut SimWorld, a: &Candidate, it: Intensity) -> Exec {
        let Some(goal) = target_pos(a) else {
            return BLOCKED;
        };
        let tile = w.map.get(goal);
        let ticks = match (a.skill, tile) {
            (Skill::Chop, Tile::Tree { .. }) => w.rules.chop_ticks,
            (Skill::Farm, Tile::Wheat) => w.rules.farm_ticks,
            (Skill::Mine, Tile::Stone | Tile::IronOre) => {
                let tier = craft::pick_tier(&w.body.inv);
                let need = if tile == Tile::IronOre { 2 } else { 1 };
                if tier < need {
                    return BLOCKED;
                }
                w.rules.mine_ticks[(tier - 1) as usize]
            }
            _ => return BLOCKED,
        };
        if !w.walk_to(goal, true, it.effort) {
            return (false, 0.0, w.alive());
        }
        w.step_world(ticks);
        if !w.alive() || !w.roll(it.focus) {
            return (false, 0.5, false);
        }
        harvest(w, goal, tile);
        (true, 1.0, false)
    }
}

/// Take what a gathered tile gives and leave what is left behind.
fn harvest(w: &mut SimWorld, at: Pos, tile: Tile) {
    match tile {
        Tile::Tree { chops_left } => {
            w.give("oak_log", 1);
            let next = if chops_left > 1 {
                Tile::Tree {
                    chops_left: chops_left - 1,
                }
            } else {
                w.map.ground(at)
            };
            w.map.set(at, next);
        }
        Tile::Wheat => {
            w.give("wheat", 1);
            w.map.set(at, Tile::Grass);
        }
        Tile::Stone => {
            w.give("cobblestone", 1);
            w.map.set(at, w.map.ground(at));
        }
        _ => {
            w.give("iron_ore", 1);
            w.map.set(at, w.map.ground(at));
        }
    }
}
