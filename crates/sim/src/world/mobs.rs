//! Hostile spawning and mob movement (spec §21; sim audit 14 Nov 2026:
//! spawns 24–40 tiles out, chasers follow a walkable path; skeletons hold
//! at range, see `ranged`).

use host_api::Pos;
use rand::Rng;

use super::{Reach, SimWorld};
use crate::geom::{cheb, dist, neighbours4, offset};
use crate::map::Biome;
use crate::mob::{Mob, MobKind, chase_steps};

impl SimWorld {
    fn walkable_free(&self, p: Pos) -> bool {
        self.map.get(p).mob_walkable()
    }

    /// Whether mob `i` is a hostile within its follow range of the agent
    /// (`chase_radius` for zombies, `skeleton_follow` for skeletons).
    pub(super) fn chases(&self, i: usize) -> bool {
        let m = &self.mobs[i];
        let r = if m.kind.ranged() {
            self.rules.skeleton_follow
        } else {
            self.rules.chase_radius
        };
        m.kind.hostile() && cheb(m.pos, self.body.pos) <= r
    }

    pub(super) fn hostile_count(&self) -> usize {
        self.mobs.iter().filter(|m| m.kind.hostile()).count()
    }

    pub(super) fn spawn_hostile(&mut self) {
        let (lo, hi) = (self.rules.spawn_min, self.rules.spawn_max);
        for _ in 0..10 {
            let dx = self.rng.random_range(-hi..=hi);
            let dz = self.rng.random_range(-hi..=hi);
            if dx.abs().max(dz.abs()) < lo {
                continue;
            }
            let p = offset(self.body.pos, dx, dz);
            if self.walkable_free(p) {
                let share = self.rules.skeleton_share;
                let kind = if share > 0.0 && self.chance(share) {
                    MobKind::Skeleton
                } else {
                    MobKind::Zombie
                };
                self.mobs.push(Mob::new(kind, p, self.tick));
                return;
            }
        }
    }

    /// A chaser (`reach` given: a hostile within its follow range) follows
    /// a shortest walkable path to the agent (#62), or the greedy steps when
    /// no path exists; a skeleton holds or backs off instead when it can
    /// (`skeleton_steps`); every other mob wanders.
    pub(super) fn move_mob(&mut self, i: usize, reach: Option<&Reach>) {
        let m = &self.mobs[i];
        let me = self.body.pos;
        let held = reach.and_then(|_| self.skeleton_steps(i));
        let steps: Vec<(i32, i32)> = if let Some(s) = held {
            s
        } else if let Some(reach) = reach {
            if cheb(m.pos, me) <= 1 {
                return;
            }
            let greedy = chase_steps(m.pos, me);
            reach
                .step_toward_origin(m.pos, greedy)
                .map_or_else(|| greedy.to_vec(), |s| vec![s])
        } else {
            let d = [(1, 0), (-1, 0), (0, 1), (0, -1)][self.rng.random_range(0..4)];
            vec![d]
        };
        for (dx, dz) in steps {
            if dx == 0 && dz == 0 {
                continue;
            }
            let p = offset(self.mobs[i].pos, dx, dz);
            // By day, hostiles keep to the dark: they never step out of a cave.
            let sunlit = !self.is_night() && self.map.biome(p) != Biome::Cave;
            if self.walkable_free(p) && p != me && !(self.mobs[i].kind.hostile() && sunlit) {
                self.mobs[i].pos = p;
                return;
            }
        }
    }

    /// The mob of `kind` nearest `last`, within 2 tiles of it: the one the
    /// agent is fighting, after it moved or was pushed.
    pub(crate) fn track(&self, kind: MobKind, last: Pos) -> Option<usize> {
        self.mobs
            .iter()
            .enumerate()
            .filter(|(_, m)| m.kind == kind && cheb(m.pos, last) <= 2)
            .min_by(|a, b| dist(a.1.pos, last).total_cmp(&dist(b.1.pos, last)))
            .map(|(i, _)| i)
    }

    /// A landed hit pushes mob `i` up to `Rules::knockback` tiles straight
    /// away from the agent, stopping at anything a mob cannot stand on, and
    /// restarts its move timer (Minecraft: a hit knocks the mob back).
    pub(crate) fn knock_back(&mut self, i: usize) {
        let me = self.body.pos;
        let m = self.mobs[i].pos;
        let (dx, dz) = ((m.x - me.x).signum(), (m.z - me.z).signum());
        for _ in 0..self.rules.knockback {
            let p = offset(self.mobs[i].pos, dx, dz);
            if !self.walkable_free(p) {
                break;
            }
            self.mobs[i].pos = p;
        }
        self.mobs[i].next_move = self.tick + self.rules.mob_move_ticks;
    }

    /// The free neighbouring tile that takes the agent furthest from
    /// `threat`, if any is further than where it stands (stepping back).
    pub(crate) fn back_off(&self, threat: Pos) -> Option<Pos> {
        let me = self.body.pos;
        let here = dist(me, threat);
        neighbours4(me)
            .into_iter()
            .filter(|p| self.map.get(*p).walkable())
            .filter(|p| self.mobs.iter().all(|m| m.pos != *p))
            .map(|p| (p, dist(p, threat)))
            .filter(|(_, d)| *d > here)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(p, _)| p)
    }
}
