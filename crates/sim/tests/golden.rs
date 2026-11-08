//! Golden worlds: exact counts for two seeds. Any change to world generation
//! (biome odds, tile odds, cell layout, pig placement, start) shows up here,
//! so it has to be deliberate. If you change generation on purpose, rerun
//! with `-- --nocapture` and update the numbers in the same commit.

use std::collections::BTreeMap;

use host_api::{Pos, World};
use sim::map::Tile;
use sim::{Rules, SimWorld};

fn summary(seed: u64) -> (BTreeMap<String, usize>, BTreeMap<String, usize>, usize, Pos) {
    let w = SimWorld::new(Rules::default(), seed);
    let mut tiles = BTreeMap::new();
    let mut biomes = BTreeMap::new();
    for z in 0..96 {
        for x in 0..96 {
            let p = Pos { x, y: 0, z };
            let name = match w.map.get(p) {
                Tile::Tree { chops_left: 3 } => "oak_log",
                Tile::Tree { .. } => "used_tree",
                t => t.name(),
            };
            *tiles.entry(name.to_string()).or_default() += 1;
            if x % 12 == 0 && z % 12 == 0 {
                *biomes.entry(format!("{:?}", w.map.biome(p))).or_default() += 1;
            }
        }
    }
    let pigs = w.mobs.len();
    let start = w.body.pos;
    (tiles, biomes, pigs, start)
}

fn check(
    seed: u64,
    tiles: &[(&str, usize)],
    biomes: &[(&str, usize)],
    pigs: usize,
    start: (i32, i32),
) {
    let (t, b, n, s) = summary(seed);
    println!("seed {seed}: tiles {t:?} biomes {b:?} pigs {n} start {s:?}");
    let want_t: BTreeMap<String, usize> = tiles.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let want_b: BTreeMap<String, usize> = biomes.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    assert_eq!(t, want_t, "tiles");
    assert_eq!(b, want_b, "biomes");
    assert_eq!(n, pigs, "pigs");
    assert_eq!((s.x, s.z), start, "start");
}

#[test]
fn golden_seed_0() {
    check(
        0,
        &[
            ("cave_floor", 864),
            ("grass_block", 4802),
            ("iron_ore", 167),
            ("lava", 102),
            ("oak_log", 1109),
            ("stone", 1500),
            ("water", 571),
            ("wheat", 101),
        ],
        &[
            ("Cave", 10),
            ("Forest", 24),
            ("Hills", 15),
            ("Lake", 5),
            ("Plains", 10),
        ],
        19,
        (48, 48),
    );
}

#[test]
fn golden_seed_1() {
    check(
        1,
        &[
            ("cave_floor", 499),
            ("grass_block", 5887),
            ("iron_ore", 94),
            ("lava", 50),
            ("oak_log", 756),
            ("stone", 856),
            ("water", 805),
            ("wheat", 269),
        ],
        &[
            ("Cave", 6),
            ("Forest", 14),
            ("Hills", 7),
            ("Lake", 7),
            ("Plains", 30),
        ],
        36,
        (48, 48),
    );
}

#[test]
fn reset_rebuilds_the_same_world() {
    let mut w = SimWorld::new(Rules::default(), 5);
    let before = (w.map.clone(), w.mobs.clone(), w.body.clone());
    w.reset(
        5,
        &host_api::Scenario {
            name: "first_night".into(),
        },
    );
    let after = (w.map.clone(), w.mobs.clone(), w.body.clone());
    assert_eq!(format!("{before:?}"), format!("{after:?}"));
}
