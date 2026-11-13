//! Attack a mob: pigs for food, or a hostile (offered by `threats`).

use host_api::{Candidate, Intensity, Skill, Target, TaskType, World};
use rand::Rng;

use super::{BLOCKED, Ctx, Exec, Handler, Out, target_pos};
use crate::geom::cheb;
use crate::mob::MobKind;
use crate::world::SimWorld;

pub(crate) struct Hunting;

impl Handler for Hunting {
    fn candidates(&self, w: &SimWorld, cx: &Ctx, out: &mut Out) {
        let pig = w
            .mobs
            .iter()
            .filter(|m| m.kind == MobKind::Pig && cheb(m.pos, cx.me) <= w.rules.view_radius)
            .filter_map(|m| cx.reach.steps_adjacent(m.pos).map(|d| (d, m.pos)))
            .min_by_key(|(d, _)| *d);
        if let Some((d, p)) = pig {
            out.add(
                Skill::Attack,
                Some(Target::Pos(p)),
                TaskType::Farming,
                w.cost(d, 0.15),
                w.risk_at(p),
            );
        }
    }

    fn owns(&self, skill: Skill) -> bool {
        skill == Skill::Attack
    }

    fn execute(&self, w: &mut SimWorld, a: &Candidate, it: Intensity) -> Exec {
        let Some(goal) = target_pos(a) else {
            return BLOCKED;
        };
        let Some(i) = w.mobs.iter().position(|m| cheb(m.pos, goal) <= 1) else {
            return BLOCKED;
        };
        let kind = w.mobs[i].kind;
        let mut idx = i;
        for _ in 0..w.rules.max_swings {
            let at = w.mobs[idx].pos;
            if !w.walk_to(at, true, it.effort) {
                return (false, 0.0, w.alive());
            }
            w.step_world(w.rules.swing_ticks);
            if !w.alive() {
                return (false, 0.0, false);
            }
            // The mob list may have changed (despawns); find it again.
            let Some(j) = w
                .mobs
                .iter()
                .position(|m| m.kind == kind && cheb(m.pos, at) <= 1)
            else {
                return (false, 0.0, false);
            };
            idx = j;
            if w.roll(it.focus) {
                w.mobs[idx].health -= w.rules.swing_damage;
                if w.mobs[idx].health <= 0.0 {
                    w.mobs.remove(idx);
                    if kind == MobKind::Pig {
                        let n = w.rng.random_range(1..=2);
                        w.give("porkchop", n);
                    }
                    return (true, 1.0, false);
                }
            }
        }
        (false, 0.5, false)
    }
}
