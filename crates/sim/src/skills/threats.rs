//! Flee from and fight the nearest hostile (spec §20: flee before health is
//! critical is a brain choice). Fleeing runs in `travelling`, attacking in
//! `hunting`.

use host_api::{Skill, Target, TaskType};

use super::{Ctx, Handler, Out};
use crate::world::SimWorld;

/// How close a hostile must be to offer flee and fight, in tiles.
pub(crate) const THREAT_RANGE: i32 = 6;

pub(crate) struct Threats;

impl Handler for Threats {
    fn candidates(&self, w: &SimWorld, cx: &Ctx, out: &mut Out) {
        let Some(i) = w.nearest_mob(|m| m.kind.hostile(), THREAT_RANGE) else {
            return;
        };
        let (me, reach) = (cx.me, &cx.reach);
        let h = w.mobs[i].pos;
        let away = ((me.x - h.x).signum(), (me.z - h.z).signum());
        let away = if away == (0, 0) { (1, 0) } else { away };
        if let Some(p) = w.point_toward(reach, away.0, away.1) {
            let steps = reach.steps(p).unwrap_or(0);
            out.add(
                Skill::Flee,
                Some(Target::Pos(p)),
                TaskType::Travel,
                w.cost(steps, 0.1),
                0.2,
            );
        }
        if let Some(steps) = reach.steps_adjacent(h) {
            out.add(
                Skill::Attack,
                Some(Target::Pos(h)),
                TaskType::Combat,
                w.cost(steps, 0.2),
                0.6,
            );
        }
    }
}
