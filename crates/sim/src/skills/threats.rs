//! Flee from and fight hostiles (spec §20: fleeing before health is
//! critical is a brain choice). Fleeing runs in `travelling`, attacking in
//! `hunting`. During a shock bout with no hostile near, flee is offered as
//! the lever (benchmark B3).

use host_api::{Pos, Skill, Target, TaskType};

use super::{Ctx, Handler, Out};
use crate::geom::{cheb, dist, square};
use crate::world::{Reach, SimWorld};

/// How close a hostile must be to offer flee and fight, in tiles.
pub(crate) const THREAT_RANGE: i32 = 6;

pub(crate) struct Threats;

impl Handler for Threats {
    fn candidates(&self, w: &SimWorld, cx: &Ctx, out: &mut Out) {
        let Some(i) = w.nearest_mob(|m| m.kind.hostile(), THREAT_RANGE) else {
            if w.in_shock_bout() {
                lever(w, cx, out);
            }
            return;
        };
        let reach = &cx.reach;
        let h = w.mobs[i].pos;
        let hostiles: Vec<Pos> = w
            .mobs
            .iter()
            .filter(|m| m.kind.hostile() && cheb(m.pos, cx.me) <= w.rules.chase_radius)
            .map(|m| m.pos)
            .collect();
        if let Some((p, steps)) = refuge(cx.me, reach, &hostiles, w.rules.max_move) {
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

/// Flee as the shock lever (benchmark B3): one step to the first tile one
/// step away, or on the spot if there is none.
fn lever(w: &SimWorld, cx: &Ctx, out: &mut Out) {
    let (p, steps) = square(cx.me, 1)
        .find(|p| cx.reach.steps(*p) == Some(1))
        .map_or((cx.me, 0), |p| (p, 1));
    out.add(
        Skill::Flee,
        Some(Target::Pos(p)),
        TaskType::Travel,
        w.cost(steps, 0.1),
        0.2,
    );
}

/// Distance from `p` to the nearest of `hostiles`.
fn clearance(p: Pos, hostiles: &[Pos]) -> f32 {
    hostiles
        .iter()
        .map(|h| dist(p, *h))
        .fold(f32::INFINITY, f32::min)
}

/// Where to flee (debt D5): of the tiles reachable within `max_move` steps,
/// the one furthest from the nearest hostile; ties go to fewer steps, then
/// to the first tile in scan order. `None` if no tile is further from the
/// hostiles than where the agent stands.
pub(crate) fn refuge(
    me: Pos,
    reach: &Reach,
    hostiles: &[Pos],
    max_move: i32,
) -> Option<(Pos, u16)> {
    let here = clearance(me, hostiles);
    let mut best: Option<(Pos, u16, f32)> = None;
    for p in square(me, max_move) {
        let Some(steps) = reach.steps(p).filter(|s| i32::from(*s) <= max_move) else {
            continue;
        };
        let c = clearance(p, hostiles);
        let better = match best {
            None => c > here,
            Some((_, s, b)) => c > b || (c == b && steps < s),
        };
        if better {
            best = Some((p, steps, c));
        }
    }
    best.map(|(p, s, _)| (p, s))
}
