//! Place memory (v0.2 R4.9, Step 20 test 4).

mod common;

use common::{at, block, obs};
use host_api::command::Whereabouts;
use minecraft::{PlaceParams, Places};

#[test]
fn remembers_what_it_saw_and_finds_the_nearest() {
    let mut m = Places::default();
    let me = at(0, 64, 0);
    m.see(&obs(
        me,
        &[],
        vec![
            block("iron_ore", me, at(10, 60, 0)),
            block("deepslate_iron_ore", me, at(-6, 64, 0)),
            block("stone", me, at(1, 64, 0)),
        ],
    ));
    assert_eq!(m.count("block:iron_ore"), 1);
    assert_eq!(m.count("block:deepslate_iron_ore"), 1);
    assert_eq!(
        m.nearest("block:iron_ore", me),
        Some(at(-6, 64, 0)),
        "deepslate counts"
    );
    assert_eq!(
        m.nearest("block:iron_ore", at(9, 60, 0)),
        Some(at(10, 60, 0))
    );
    assert_eq!(m.nearest("block:coal_ore", me), None);
    assert!(m.is_at(at(10, 60, 0), "block:iron_ore"));
    assert!(m.is_at(at(-6, 64, 0), "block:iron_ore"));
    assert!(!m.is_at(at(1, 64, 0), "block:iron_ore"));
    m.forget("block:iron_ore", at(10, 60, 0));
    assert!(!m.is_at(at(10, 60, 0), "block:iron_ore"));
    m.forget("block:gold_ore", at(0, 0, 0));
    assert_eq!(m.count("block:gold_ore"), 0);
}

#[test]
fn ties_go_to_the_newer_spot_and_old_spots_drop_first() {
    let mut m = Places::new(PlaceParams {
        max_per_thing: 2,
        ..PlaceParams::default()
    });
    m.remember("block:stone", at(4, 0, 0));
    m.remember("block:stone", at(-4, 0, 0));
    assert_eq!(m.nearest("block:stone", at(0, 0, 0)), Some(at(-4, 0, 0)));
    m.remember("block:stone", at(4, 0, 0));
    assert_eq!(
        m.nearest("block:stone", at(0, 0, 0)),
        Some(at(4, 0, 0)),
        "refreshed"
    );
    assert_eq!(m.count("block:stone"), 2, "no duplicates");
    m.remember("block:stone", at(9, 0, 0));
    assert_eq!(m.count("block:stone"), 2);
    assert!(!m.is_at(at(-4, 0, 0), "block:stone"), "the oldest went");
    assert!(m.is_at(at(4, 0, 0), "block:stone") && m.is_at(at(9, 0, 0), "block:stone"));
}

#[test]
fn nearby_spots_that_are_gone_are_forgotten() {
    let mut m = Places::default();
    let me = at(0, 64, 0);
    m.remember("block:iron_ore", at(2, 64, 0));
    m.remember("block:iron_ore", at(30, 64, 0));
    m.remember("block:oak_log", at(0, 64, 3));
    // Nothing listed at all: no evidence, nothing forgotten.
    m.see(&obs(me, &[], vec![]));
    assert_eq!(m.count("block:iron_ore"), 2);
    // Blocks listed out to 3: the ore at 2 should have been listed and was
    // not, so it is gone; the log at exactly 3 and the far ore stay.
    m.see(&obs(me, &[], vec![block("stone", me, at(-3, 64, 0))]));
    assert!(!m.is_at(at(2, 64, 0), "block:iron_ore"));
    assert!(m.is_at(at(30, 64, 0), "block:iron_ore"));
    assert!(m.is_at(at(0, 64, 3), "block:oak_log"));
    // The checked radius never passes confirm_radius, even when the bot
    // lists blocks farther out.
    m.remember("block:iron_ore", at(5, 64, 0));
    m.see(&obs(me, &[], vec![block("stone", me, at(0, 64, 9))]));
    assert!(
        m.is_at(at(5, 64, 0), "block:iron_ore"),
        "5 is past the 4-block check"
    );
    assert!(!m.is_at(at(0, 64, 3), "block:oak_log"), "3 is inside it");
    assert!(
        m.is_at(at(0, 64, 9), "block:stone"),
        "what is seen is stored"
    );
}
