//! The mob lessons' tests in Minecraft (01 §4.11, #59; Michael, 15 Nov
//! 2026: "zombie: fight; skeleton: hide", fight only at health ≥
//! `Rules::fight_health`). The sim reads them off its world
//! (`SimWorld::mob_test`); here they are read off the bot's observation, so
//! the same player call and teacher (`mob_lessons`) run on the bot.
//!
//! Two differences from the sim, both from what the bot can see: a
//! skeleton within `McParams::shoot_range` counts as able to shoot, since
//! the observation has no line of sight (as the candidates' threat check);
//! and only `entity:zombie` is a zombie, because the fight recipe's step
//! is "Attack entity:zombie" (a husk or drowned is still a threat to flee).

use host_api::ontology::RANGED_HOSTILES;
use host_api::{Observation, Seen, SeenKind};
use minecraft::McParams;
use sim::Rules;
use sim::world::MobTest;

const ZOMBIE: &str = "entity:zombie";

fn hostile(s: &Seen) -> bool {
    s.kind == (SeenKind::Entity { hostile: true })
}

/// The mob lessons' tests where the bot stands (module doc): nothing
/// while it is sheltered, as in the sim.
pub(crate) fn mob_test(obs: &Observation, p: &McParams) -> MobTest {
    if obs.sheltered {
        return MobTest::default();
    }
    let near =
        |s: &Seen, name: &str, range: f32| hostile(s) && s.thing.0 == name && s.distance <= range;
    MobTest {
        zombie_near: obs.visible.iter().any(|s| near(s, ZOMBIE, p.threat_range)),
        shot_at: obs
            .visible
            .iter()
            .any(|s| RANGED_HOSTILES.iter().any(|r| near(s, r, p.shoot_range))),
        weak: obs.health < Rules::default().fight_health,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_api::{AgentId, Inventory, Pos, ThingId};

    fn seen(name: &str, hostile: bool, distance: f32) -> Seen {
        Seen {
            thing: ThingId(name.into()),
            kind: SeenKind::Entity { hostile },
            pos: Pos { x: 0, y: 64, z: 0 },
            distance,
        }
    }

    fn view(health: f32, visible: Vec<Seen>) -> Observation {
        Observation {
            agent: AgentId(0),
            tick: 0,
            day_phase: 0.6,
            health,
            food: 1.0,
            position: Pos { x: 0, y: 64, z: 0 },
            inventory: Inventory::default(),
            visible,
            chat: vec![],
            damage_taken: 0.0,
            resting: false,
            sheltered: false,
        }
    }

    fn t(zombie_near: bool, shot_at: bool, weak: bool) -> MobTest {
        MobTest {
            zombie_near,
            shot_at,
            weak,
        }
    }

    #[test]
    fn zombies_count_within_threat_range_and_skeletons_within_shot() {
        let p = McParams::default();
        let at = |v| mob_test(&view(1.0, v), &p);
        assert_eq!(at(vec![seen(ZOMBIE, true, 8.0)]), t(true, false, false));
        assert_eq!(at(vec![seen(ZOMBIE, true, 8.5)]), t(false, false, false));
        for r in RANGED_HOSTILES {
            assert_eq!(at(vec![seen(r, true, 15.0)]), t(false, true, false));
            assert_eq!(at(vec![seen(r, true, 15.5)]), t(false, false, false));
        }
        let both = vec![seen("entity:skeleton", true, 12.0), seen(ZOMBIE, true, 3.0)];
        assert_eq!(at(both), t(true, true, false));
    }

    #[test]
    fn other_mobs_and_passive_namesakes_do_not_count() {
        let p = McParams::default();
        let at = |v| mob_test(&view(1.0, v), &p);
        let husk = vec![
            seen("entity:husk", true, 2.0),
            seen("entity:spider", true, 2.0),
        ];
        assert_eq!(at(husk), MobTest::default());
        let tame = vec![
            seen(ZOMBIE, false, 2.0),
            seen("entity:skeleton", false, 2.0),
        ];
        assert_eq!(at(tame), MobTest::default());
    }

    #[test]
    fn weak_is_below_the_sims_fight_health_and_shelter_clears_all() {
        let p = McParams::default();
        let gate = Rules::default().fight_health;
        assert_eq!(gate, 0.5);
        let z = || vec![seen(ZOMBIE, true, 2.0), seen("entity:skeleton", true, 2.0)];
        assert_eq!(mob_test(&view(0.49, z()), &p), t(true, true, true));
        assert_eq!(mob_test(&view(0.5, z()), &p), t(true, true, false));
        let mut inside = view(0.1, z());
        inside.sheltered = true;
        assert_eq!(mob_test(&inside, &p), MobTest::default());
    }
}
