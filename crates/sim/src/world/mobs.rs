//! Hostile spawning and mob movement (spec §21; sim audit 14 Nov 2026:
//! spawns 24–40 tiles out, chasers follow a walkable path).

use host_api::Pos;
use rand::Rng;

use super::{Reach, SimWorld};
use crate::geom::{cheb, offset};
use crate::map::Biome;
use crate::mob::{Mob, MobKind, chase_steps};

impl SimWorld {
    fn walkable_free(&self, p: Pos) -> bool {
        self.map.get(p).mob_walkable()
    }

    /// Whether mob `i` is a hostile within `chase_radius` of the agent.
    pub(super) fn chases(&self, i: usize) -> bool {
        let m = &self.mobs[i];
        m.kind.hostile() && cheb(m.pos, self.body.pos) <= self.rules.chase_radius
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
                self.mobs.push(Mob::new(MobKind::Zombie, p, self.tick));
                return;
            }
        }
    }

    /// A chaser (`reach` given: a hostile within `chase_radius`) follows a
    /// shortest walkable path to the agent (#62), or the greedy steps when
    /// no path exists; every other mob wanders.
    pub(super) fn move_mob(&mut self, i: usize, reach: Option<&Reach>) {
        let m = &self.mobs[i];
        let me = self.body.pos;
        let steps: Vec<(i32, i32)> = if let Some(reach) = reach {
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
}
