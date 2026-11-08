//! Minecraft candidates (spec §2, §6), checked against hand-worked costs.

mod common;

use common::{at, block, obs, seen};
use host_api::command::CommandQueue;
use host_api::{Candidate, ChatMsg, MAX_CANDIDATES, SeenKind, Skill, Target, TaskType, ThingId};
use minecraft::{McParams, Places, candidates};

const HOSTILE: SeenKind = SeenKind::Entity { hostile: true };
const PASSIVE: SeenKind = SeenKind::Entity { hostile: false };

fn row(c: &Candidate) -> (Skill, Option<Target>, TaskType) {
    (c.skill, c.target.clone(), c.task_type)
}

fn close(c: &Candidate, cost: f32, risk: f32) {
    assert!(
        (c.est_cost - cost).abs() < 1e-5 && (c.est_risk - risk).abs() < 1e-5,
        "{:?}: cost {} risk {}, want {cost} {risk}",
        c.skill,
        c.est_cost,
        c.est_risk
    );
}

fn pos(x: i32, y: i32, z: i32) -> Option<Target> {
    Some(Target::Pos(at(x, y, z)))
}

#[test]
fn a_busy_daytime_scene_by_hand() {
    let me = at(0, 64, 0);
    let mut o = obs(
        me,
        &[("porkchop", 2), ("oak_planks", 4)],
        vec![
            seen("player:alex", SeenKind::Player, me, at(1, 64, 1)),
            block("oak_log", me, at(2, 64, 0)),
            seen("entity:zombie", HOSTILE, me, at(3, 64, 4)),
            seen("entity:pig", PASSIVE, me, at(0, 64, 6)),
        ],
    );
    o.food = 0.5;
    let c = candidates(&o, &Places::default(), &McParams::default());
    let rows: Vec<_> = c.iter().map(row).collect();
    let search = |t: &str| Some(Target::Thing(ThingId(t.into())));
    let item = |i: &str| Some(Target::Item(i.into()));
    assert_eq!(
        rows,
        vec![
            // Away from the zombie: (-3, -4) scaled to 16 blocks.
            (Skill::Flee, pos(-10, 64, -13), TaskType::Travel),
            (Skill::Attack, pos(3, 64, 4), TaskType::Combat),
            (Skill::Eat, item("porkchop"), TaskType::Farming),
            (Skill::Shelter, None, TaskType::Building),
            (Skill::Chop, pos(2, 64, 0), TaskType::Mining),
            (Skill::Farm, search("block:wheat"), TaskType::Farming),
            (Skill::Attack, pos(0, 64, 6), TaskType::Farming),
            (Skill::Craft, item("stick"), TaskType::Crafting),
            (Skill::Craft, item("crafting_table"), TaskType::Crafting),
            (Skill::Explore, pos(0, 64, -24), TaskType::Travel),
            (Skill::Explore, pos(24, 64, 0), TaskType::Travel),
            (Skill::Explore, pos(0, 64, 24), TaskType::Travel),
            (Skill::Explore, pos(-24, 64, 0), TaskType::Travel),
            (
                Skill::FollowPlayer,
                Some(Target::Player("alex".into())),
                TaskType::Helping
            ),
        ]
    );
    assert!(
        c.iter()
            .enumerate()
            .all(|(i, c)| c.id == i as u32 && !c.commanded)
    );
    // Cost = base + blocks / (4 × 24); risk = 0.15 per hostile within 8.
    close(&c[0], 0.1 + 16.0 / 96.0, 0.2);
    close(&c[1], 0.2 + 5.0 / 96.0, 0.6);
    close(&c[2], 0.05, 0.0);
    close(&c[3], 0.25, 0.15);
    close(&c[4], 0.1 + 2.0 / 96.0, 0.15);
    close(&c[5], 0.6, 0.3);
    close(&c[6], 0.15 + 6.0 / 96.0, 0.15);
    close(&c[7], 0.05, 0.0);
    close(&c[9], 0.25, 0.0);
    close(&c[13], 0.05 + 2f32.sqrt() / 96.0, 0.0);
}

#[test]
fn night_shelter_beds_and_lava() {
    let me = at(0, 64, 0);
    let mut o = obs(
        me,
        &[("cobblestone", 8), ("bread", 1)],
        vec![
            block("red_bed", me, at(0, 64, 3)),
            block("lava", me, at(25, 64, 1)),
            seen("entity:skeleton", HOSTILE, me, at(0, 64, 20)),
        ],
    );
    o.day_phase = 0.6;
    let c = candidates(&o, &Places::default(), &McParams::default());
    let rows: Vec<_> = c.iter().map(row).take(3).collect();
    assert_eq!(
        rows,
        vec![
            (Skill::Shelter, None, TaskType::Building),
            (Skill::Rest, pos(0, 64, 3), TaskType::Building),
            (
                Skill::Chop,
                Some(Target::Thing(ThingId("block:oak_log".into()))),
                TaskType::Mining
            ),
        ],
        "full food: no eating; the skeleton is out of threat range"
    );
    close(&c[0], 0.1, 0.3);
    close(&c[1], 0.1 + 3.0 / 96.0, 0.3);
    let east = c.iter().find(|c| c.target == pos(24, 64, 0)).unwrap();
    close(east, 0.25, 0.3 + 0.3);
    let south = c.iter().find(|c| c.target == pos(0, 64, 24)).unwrap();
    close(south, 0.25, 0.3 + 0.15);
    o.sheltered = true;
    let c = candidates(&o, &Places::default(), &McParams::default());
    assert_eq!(row(&c[0]), (Skill::Rest, None, TaskType::Building));
    close(&c[0], 0.1, 0.0);
    assert_ne!(c[1].skill, Skill::Rest, "sheltered: no bed trip offered");
}

#[test]
fn remembered_ore_is_offered_and_commanded() {
    // Test 4: with nothing in view, the stored iron is the target.
    let me = at(0, 64, 0);
    let mut places = Places::default();
    places.remember("block:iron_ore", at(40, 12, 0));
    let o = obs(
        me,
        &[("stone_pickaxe", 1)],
        vec![block("stone", me, at(1, 63, 0))],
    );
    places.see(&o);
    let mut c = candidates(&o, &places, &McParams::default());
    let mine: Vec<_> = c
        .iter()
        .filter(|c| c.skill == Skill::Mine)
        .map(row)
        .collect();
    assert_eq!(
        mine,
        vec![
            (Skill::Mine, pos(1, 63, 0), TaskType::Mining),
            (Skill::Mine, pos(40, 12, 0), TaskType::Mining),
        ]
    );
    let iron = c.iter().find(|c| c.target == pos(40, 12, 0)).unwrap();
    close(iron, 0.1 + 4304f32.sqrt() / 96.0, 0.0);
    let mut q = CommandQueue::default();
    let msg = ChatMsg {
        from: "alex".into(),
        text: "mine more iron".into(),
    };
    q.hear(&msg, &o, |n| n == "iron_ore");
    q.mark(&o, &places, &mut c);
    let marked: Vec<_> = c.iter().filter(|c| c.commanded).map(row).collect();
    assert_eq!(
        marked,
        vec![(Skill::Mine, pos(40, 12, 0), TaskType::Mining)]
    );
}

#[test]
fn never_more_than_k_and_players_are_cut_first() {
    let me = at(0, 64, 0);
    let mut visible = vec![];
    for i in 0..20 {
        visible.push(seen(
            &format!("player:p{i}"),
            SeenKind::Player,
            me,
            at(i, 64, 2),
        ));
    }
    let o = obs(me, &[], visible);
    let c = candidates(&o, &Places::default(), &McParams::default());
    assert_eq!(c.len(), MAX_CANDIDATES);
    let followers = c.iter().filter(|c| c.skill == Skill::FollowPlayer).count();
    assert_eq!(
        followers,
        20 - 7,
        "shelter, 2 searches, 4 explores, then players"
    );
    assert_eq!(c[19].id, 19);
}

#[test]
fn a_hostile_on_top_of_the_agent_flees_east() {
    let me = at(5, 64, 5);
    let o = obs(
        me,
        &[],
        vec![seen("entity:zombie", HOSTILE, me, at(5, 64, 5))],
    );
    let c = candidates(&o, &Places::default(), &McParams::default());
    assert_eq!(row(&c[0]), (Skill::Flee, pos(21, 64, 5), TaskType::Travel));
    assert_eq!(c[1].target, pos(5, 64, 5));
}

mod props {
    use super::*;
    use proptest::prelude::*;

    fn kind(k: u8) -> (SeenKind, &'static str) {
        match k % 6 {
            0 => (HOSTILE, "entity:zombie"),
            1 => (PASSIVE, "entity:cow"),
            2 => (SeenKind::Player, "player:p"),
            3 => (SeenKind::Block, "block:iron_ore"),
            4 => (SeenKind::Block, "block:red_bed"),
            _ => (SeenKind::Block, "block:lava"),
        }
    }

    proptest! {
        #[test]
        fn candidates_stay_in_range(
            things in prop::collection::vec((0u8..6, -40i32..40, -40i32..40), 0..40),
            phase in 0.0f32..1.0,
            food in 0.0f32..1.0,
            sheltered: bool,
            picks in 0u8..4,
        ) {
            let me = at(0, 64, 0);
            let visible = things
                .iter()
                .map(|(k, x, z)| {
                    let (kind, name) = kind(*k);
                    seen(name, kind, me, at(*x, 64, *z))
                })
                .collect();
            let pick = ["dirt", "wooden_pickaxe", "stone_pickaxe", "iron_pickaxe"][picks as usize];
            let mut o = obs(me, &[(pick, 1), ("bread", 1)], visible);
            o.day_phase = phase;
            o.food = food;
            o.sheltered = sheltered;
            let c = candidates(&o, &Places::default(), &McParams::default());
            prop_assert!(!c.is_empty() && c.len() <= MAX_CANDIDATES);
            for (i, c) in c.iter().enumerate() {
                prop_assert_eq!(c.id, i as u32);
                prop_assert!((0.0..=1.0).contains(&c.est_cost));
                prop_assert!((0.0..=1.0).contains(&c.est_risk));
                prop_assert!(!c.commanded);
            }
        }
    }
}
