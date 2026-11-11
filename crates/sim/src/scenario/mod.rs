//! Named scenarios (spec §21). Each builds a world, a scripted player and a
//! tick limit from a seed. The trip scenarios of spec §20.1 and incubation
//! live in `trips`.

use host_api::Pos;

use crate::geom::ring;
use crate::mob::{Mob, MobKind};
use crate::player::ScriptedPlayer;
use crate::rules::Rules;
use crate::world::SimWorld;

pub mod held_out;
pub mod trips;

/// One day (spec §21).
pub const DAY: u64 = 2400;
/// Hostiles the harsh episode keeps around the agent, and how close.
pub const HARSH_MOBS: usize = 4;
pub const HARSH_RING: i32 = 2;
/// Damage per hit during the harsh episode: it hurts and frightens without
/// killing, so D can build up (proposed).
pub const HARSH_DAMAGE: f32 = 0.01;

/// The scenario names `build` knows.
pub const NAMES: [&str; 11] = [
    "first_night",
    "open_day",
    "commanded",
    "harsh",
    "long_horizon",
    "far_food",
    "dusk_errand",
    "hostile_at_food",
    "expedition",
    "incubation",
    "held_out",
];

pub struct Setup {
    pub name: &'static str,
    pub world: SimWorld,
    pub player: ScriptedPlayer,
    pub max_ticks: u64,
    /// Keep hostiles around the agent until this tick (harsh episode).
    pub harass_until: u64,
    /// Lifespan for urgency (spec §15), if the scenario sets one.
    pub horizon: Option<f32>,
    /// The teacher shows recipes for the first this many days, then stops
    /// (plan §5.0 teachability); `None` is the whole run. Read by the
    /// runner's teachers; the world ignores it.
    pub teach_days: Option<u64>,
}

/// Build a scenario by name.
pub fn build(name: &str, rules: Rules, seed: u64) -> Option<Setup> {
    let others: [(&str, trips::Builder); 1] = [("held_out", held_out::held_out)];
    if let Some((_, make)) = trips::TRIPS.iter().chain(&others).find(|(n, _)| *n == name) {
        return Some(make(rules, seed));
    }
    let mut s = Setup {
        name: NAMES.into_iter().find(|n| *n == name)?,
        world: SimWorld::new(rules.clone(), seed),
        player: ScriptedPlayer::idle(),
        max_ticks: DAY,
        harass_until: 0,
        horizon: None,
        teach_days: None,
    };
    match name {
        "open_day" => s.max_ticks = 3 * DAY,
        "commanded" => {
            s.player = ScriptedPlayer::commanding(vec![
                (0, "wooden_pickaxe".into()),
                (DAY / 2, "stone_pickaxe".into()),
            ]);
            s.max_ticks = 2 * DAY;
        }
        "harsh" => {
            let harsh = Rules {
                mob_damage: HARSH_DAMAGE,
                ..rules
            };
            s.world = SimWorld::new(harsh, seed);
            s.world.tick = DAY / 2;
            s.harass_until = DAY;
            s.max_ticks = 3 * DAY;
        }
        "long_horizon" => {
            s.max_ticks = 5 * DAY;
            s.horizon = Some((4 * DAY) as f32);
        }
        _ => {}
    }
    Some(s)
}

/// The harsh episode stops pressing below this health, so it frightens and
/// hurts without killing (proposed).
pub const HARSH_FLOOR: f32 = 0.5;

/// Keep `HARSH_MOBS` hostiles within `HARSH_RING` tiles (harsh episode).
/// At or below `HARSH_FLOOR` health they back off, so the episode hurts and
/// frightens without killing.
pub fn harass(w: &mut SimWorld) {
    if w.body.health <= HARSH_FLOOR {
        end_harsh(w);
        return;
    }
    let near = |w: &SimWorld| {
        w.mobs
            .iter()
            .filter(|m| m.kind.hostile() && crate::geom::cheb(m.pos, w.body.pos) <= HARSH_RING)
            .count()
    };
    let spots: Vec<Pos> = ring(w.body.pos, HARSH_RING).collect();
    for p in spots {
        if near(w) >= HARSH_MOBS {
            return;
        }
        let free =
            w.map.in_bounds(p) && w.map.get(p).walkable() && !w.mobs.iter().any(|m| m.pos == p);
        if free {
            w.mobs.push(Mob::new(MobKind::Zombie, p, w.tick));
        }
    }
}

/// The harsh episode is over: every hostile leaves.
pub fn end_harsh(w: &mut SimWorld) {
    w.mobs.retain(|m| !m.kind.hostile());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_builds_and_unknown_does_not() {
        for n in NAMES {
            let s = build(n, Rules::default(), 1).unwrap();
            assert_eq!(s.name, n);
            assert!(s.max_ticks >= DAY);
        }
        assert!(build("nope", Rules::default(), 1).is_none());
    }

    #[test]
    fn scenario_settings() {
        let r = Rules::default;
        let c = build("commanded", r(), 0).unwrap();
        assert_eq!(c.player.commands.len(), 2);
        assert!(c.player.present);
        let h = build("harsh", r(), 0).unwrap();
        assert_eq!((h.world.tick, h.harass_until), (DAY / 2, DAY));
        assert_eq!(h.world.rules.mob_damage, HARSH_DAMAGE);
        let l = build("long_horizon", r(), 0).unwrap();
        assert_eq!(l.horizon, Some(9600.0));
        assert_eq!(build("open_day", r(), 0).unwrap().max_ticks, 3 * DAY);
        assert_eq!(build("first_night", r(), 0).unwrap().max_ticks, DAY);
        assert_eq!((c.max_ticks, h.max_ticks), (2 * DAY, 3 * DAY));
        assert_eq!(l.max_ticks, 5 * DAY);
    }

    #[test]
    fn harass_counts_only_close_hostiles_and_spawns_on_free_ground() {
        let mut s = build("harsh", Rules::default(), 3).unwrap();
        let w = &mut s.world;
        w.mobs.clear();
        let me = Pos { x: 20, y: 0, z: 20 };
        w.body.pos = me;
        for (i, p) in ring(me, HARSH_RING).enumerate() {
            w.map.set(
                p,
                if i < 8 {
                    crate::map::Tile::Stone
                } else {
                    crate::map::Tile::Grass
                },
            );
        }
        for i in 0..HARSH_MOBS as i32 {
            let far = Pos { x: 30 + i, ..me };
            w.mobs.push(Mob::new(MobKind::Zombie, far, w.tick));
        }
        let pig = Pos { x: 21, ..me };
        w.mobs.push(Mob::new(MobKind::Pig, pig, w.tick));
        harass(w);
        let new: Vec<&Mob> = w.mobs[HARSH_MOBS + 1..].iter().collect();
        assert_eq!(new.len(), HARSH_MOBS, "far zombies and a pig do not count");
        for m in &new {
            assert!(
                m.kind.hostile() && w.map.get(m.pos).walkable(),
                "{:?}",
                m.pos
            );
            assert!(crate::geom::cheb(m.pos, me) == HARSH_RING);
        }
        let mut spots: Vec<(i32, i32)> = new.iter().map(|m| (m.pos.x, m.pos.z)).collect();
        spots.dedup();
        assert_eq!(spots.len(), HARSH_MOBS, "one per tile");
    }

    #[test]
    fn harass_keeps_hostiles_close() {
        let mut s = build("harsh", Rules::default(), 3).unwrap();
        harass(&mut s.world);
        let me = s.world.body.pos;
        let close = s
            .world
            .mobs
            .iter()
            .filter(|m| m.kind.hostile() && crate::geom::cheb(m.pos, me) <= HARSH_RING)
            .count();
        assert_eq!(close, HARSH_MOBS);
        let n = s.world.mobs.len();
        harass(&mut s.world);
        assert_eq!(s.world.mobs.len(), n, "already enough");
        s.world.body.health = HARSH_FLOOR;
        harass(&mut s.world);
        assert!(
            s.world.mobs.iter().all(|m| !m.kind.hostile()),
            "they back off at the floor"
        );
    }
}
