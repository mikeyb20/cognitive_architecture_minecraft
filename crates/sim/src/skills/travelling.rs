//! Explore, move to a place, and flee (spec §21). Explore offers four
//! directions, least-visited region first.

use host_api::{Candidate, Intensity, Pos, Skill, Target, TaskType};

use super::{BLOCKED, Ctx, Exec, Handler, Out, target_pos};
use crate::world::SimWorld;

pub(crate) struct Travelling;

impl Handler for Travelling {
    fn candidates(&self, w: &SimWorld, cx: &Ctx, out: &mut Out) {
        let mut dirs: Vec<(u32, Pos)> = [(0, -1), (1, 0), (0, 1), (-1, 0)]
            .into_iter()
            .filter_map(|(dx, dz)| w.point_toward(&cx.reach, dx, dz))
            .map(|p| (w.region_visits(p), p))
            .collect();
        dirs.sort_by_key(|(v, _)| *v);
        dirs.dedup_by_key(|(_, p)| *p);
        for (_, p) in dirs {
            let steps = cx.reach.steps(p).unwrap_or(0);
            out.add(
                Skill::Explore,
                Some(Target::Pos(p)),
                TaskType::Travel,
                w.cost(steps, 0.0),
                w.risk_at(p),
            );
        }
    }

    fn owns(&self, skill: Skill) -> bool {
        matches!(skill, Skill::Explore | Skill::MoveTo | Skill::Flee)
    }

    fn execute(&self, w: &mut SimWorld, a: &Candidate, it: Intensity) -> Exec {
        let Some(goal) = target_pos(a) else {
            return BLOCKED;
        };
        if a.skill == Skill::Flee {
            w.shock_lever();
        }
        // Fleeing is always at full speed, once there is effort to walk.
        let effort = if a.skill == Skill::Flee && it.effort >= w.rules.min_effort {
            1.0
        } else {
            it.effort
        };
        let ok = w.walk_to(goal, false, effort);
        (ok, if ok { 1.0 } else { 0.0 }, !ok && w.alive())
    }
}
