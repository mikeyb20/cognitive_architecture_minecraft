//! Seeded tile map with biomes (spec §21).

use host_api::Pos;

use crate::geom::{div_ceil, ring};
use rand::Rng;
use rand_chacha::ChaCha8Rng;

/// One tile. Names follow Minecraft so memories transfer (spec §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tile {
    Grass,
    Tree {
        chops_left: u8,
    },
    Stone,
    IronOre,
    Water,
    Lava,
    CaveFloor,
    Wheat,
    /// A built hut: walkable, and hostile mobs cannot reach whoever is in it.
    Shelter,
    /// A wall of planks, as a built hut leaves it (spec §21, step 13).
    Planks,
    /// A door: the agent opens it and walks through; mobs cannot (spec §21,
    /// #58: as in Minecraft, zombies don't open doors).
    Door,
}

impl Tile {
    /// Minecraft-style name, used as the ThingId.
    pub fn name(self) -> &'static str {
        match self {
            Tile::Grass => "grass_block",
            Tile::Tree { .. } => "oak_log",
            Tile::Stone => "stone",
            Tile::IronOre => "iron_ore",
            Tile::Water => "water",
            Tile::Lava => "lava",
            Tile::CaveFloor => "cave_floor",
            Tile::Wheat => "wheat",
            Tile::Shelter => "shelter",
            Tile::Planks => "oak_planks",
            Tile::Door => "oak_door",
        }
    }

    /// Tiles the agent can stand on.
    pub fn walkable(self) -> bool {
        matches!(
            self,
            Tile::Grass | Tile::CaveFloor | Tile::Wheat | Tile::Shelter | Tile::Door
        )
    }

    /// Tiles a mob can stand on: walkable, but not a hut or a door (spec §21).
    pub fn mob_walkable(self) -> bool {
        self.walkable() && !matches!(self, Tile::Shelter | Tile::Door)
    }

    /// Tiles worth listing in an observation (not plain ground).
    pub fn notable(self) -> bool {
        !matches!(self, Tile::Grass | Tile::CaveFloor)
    }
}

/// Biome of a 12×12 cell, chosen per seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Biome {
    Plains,
    Forest,
    Hills,
    Cave,
    Lake,
}

#[derive(Debug, Clone)]
pub struct Map {
    pub size: i32,
    tiles: Vec<Tile>,
    biomes: Vec<Biome>,
}

const CELL: i32 = 12;

impl Map {
    /// Generate a map. Biomes are picked per 12×12 cell; the centre cell is
    /// always plains so every run starts somewhere fair.
    pub fn generate(size: i32, tree_chops: u8, rng: &mut ChaCha8Rng) -> Self {
        let cells = div_ceil(size, CELL);
        let mut biomes = Vec::with_capacity((cells * cells) as usize);
        for cy in 0..cells {
            for cx in 0..cells {
                let b = if cx == cells / 2 && cy == cells / 2 {
                    Biome::Plains
                } else {
                    match rng.random_range(0..100) {
                        0..35 => Biome::Plains,
                        35..65 => Biome::Forest,
                        65..82 => Biome::Hills,
                        82..92 => Biome::Cave,
                        _ => Biome::Lake,
                    }
                };
                biomes.push(b);
            }
        }
        let mut tiles = Vec::with_capacity((size * size) as usize);
        for y in 0..size {
            for x in 0..size {
                let b = biomes[((y / CELL) * cells + x / CELL) as usize];
                let r: u32 = rng.random_range(0..100);
                let t = match b {
                    Biome::Plains => match r {
                        0..4 => Tile::Tree {
                            chops_left: tree_chops,
                        },
                        4..10 => Tile::Wheat,
                        10..12 => Tile::Stone,
                        _ => Tile::Grass,
                    },
                    Biome::Forest => match r {
                        0..30 => Tile::Tree {
                            chops_left: tree_chops,
                        },
                        30..33 => Tile::Stone,
                        _ => Tile::Grass,
                    },
                    // Hills are walkable slopes with rock outcrops: a quarter
                    // blocked, so the open ground stays connected (2-D site
                    // percolation needs > 59% open). At 50% blocked they were
                    // a maze of pockets (sim audit, 14 Nov 2026); in
                    // Minecraft a 1-block step is climbable.
                    Biome::Hills => hill_tile(r),
                    Biome::Cave => match r {
                        0..30 => Tile::Stone,
                        30..38 => Tile::IronOre,
                        38..41 => Tile::Lava,
                        _ => Tile::CaveFloor,
                    },
                    Biome::Lake => match r {
                        0..80 => Tile::Water,
                        _ => Tile::Grass,
                    },
                };
                tiles.push(t);
            }
        }
        Self {
            size,
            tiles,
            biomes,
        }
    }

    /// A map of one tile and one biome (for hand-built scenarios and tests).
    pub fn filled(size: i32, tile: Tile, biome: Biome) -> Self {
        let cells = div_ceil(size, CELL);
        Self {
            size,
            tiles: vec![tile; (size * size) as usize],
            biomes: vec![biome; (cells * cells) as usize],
        }
    }

    /// Index of the 12×12 cell holding `p` (clamped onto the map).
    fn cell(&self, p: Pos) -> usize {
        let cells = div_ceil(self.size, CELL);
        let x = p.x.clamp(0, self.size - 1) / CELL;
        let z = p.z.clamp(0, self.size - 1) / CELL;
        (z * cells + x) as usize
    }

    /// Set the biome of the 12×12 cell holding `p`.
    pub fn set_biome(&mut self, p: Pos, b: Biome) {
        let i = self.cell(p);
        self.biomes[i] = b;
    }

    pub fn in_bounds(&self, p: Pos) -> bool {
        p.x >= 0 && p.z >= 0 && p.x < self.size && p.z < self.size
    }

    /// Tile at a position; out of bounds reads as water (the map edge).
    pub fn get(&self, p: Pos) -> Tile {
        if self.in_bounds(p) {
            self.tiles[(p.z * self.size + p.x) as usize]
        } else {
            Tile::Water
        }
    }

    pub fn set(&mut self, p: Pos, t: Tile) {
        if self.in_bounds(p) {
            let i = (p.z * self.size + p.x) as usize;
            self.tiles[i] = t;
        }
    }

    /// What a cleared tile becomes: cave floor in caves, grass elsewhere.
    pub fn ground(&self, p: Pos) -> Tile {
        if self.biome(p) == Biome::Cave {
            Tile::CaveFloor
        } else {
            Tile::Grass
        }
    }

    pub fn biome(&self, p: Pos) -> Biome {
        self.biomes[self.cell(p)]
    }

    /// A walkable tile near the centre (the start position).
    pub fn start_pos(&self) -> Pos {
        let c = Pos {
            x: self.size / 2,
            y: 0,
            z: self.size / 2,
        };
        (0..self.size)
            .flat_map(|r| ring(c, r))
            .find(|p| self.get(*p).walkable())
            .unwrap_or(c)
    }
}

/// A hills tile from a 0..100 roll (spec §21; sim audit 14 Nov 2026).
fn hill_tile(r: u32) -> Tile {
    match r {
        0..20 => Tile::Stone,
        20..23 => Tile::IronOre,
        23..25 => Tile::Lava,
        _ => Tile::Grass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn hills_are_a_quarter_rock_and_stay_open() {
        // 20% stone, 3% ore, 2% lava: about 25% blocked, well under the
        // 41% at which 4-way open ground stops percolating.
        let (mut blocked, mut all) = (0u32, 0u32);
        for seed in 0..8 {
            let m = Map::generate(96, 3, &mut ChaCha8Rng::seed_from_u64(seed));
            for z in 0..96 {
                for x in 0..96 {
                    let q = Pos { x, y: 0, z };
                    if m.biome(q) == Biome::Hills {
                        all += 1;
                        blocked += u32::from(!m.get(q).walkable());
                    }
                }
            }
        }
        let share = f64::from(blocked) / f64::from(all);
        assert!(
            all > 5000 && (0.22..0.28).contains(&share),
            "{share} of {all}"
        );
        let only = hill_tile;
        assert_eq!(only(19), Tile::Stone);
        assert_eq!(only(20), Tile::IronOre);
        assert_eq!(only(22), Tile::IronOre);
        assert_eq!(only(23), Tile::Lava);
        assert_eq!(only(24), Tile::Lava);
        assert_eq!(only(25), Tile::Grass);
    }

    #[test]
    fn same_seed_same_map() {
        let a = Map::generate(48, 3, &mut ChaCha8Rng::seed_from_u64(5));
        let b = Map::generate(48, 3, &mut ChaCha8Rng::seed_from_u64(5));
        assert_eq!(a.tiles, b.tiles);
        let c = Map::generate(48, 3, &mut ChaCha8Rng::seed_from_u64(6));
        assert_ne!(a.tiles, c.tiles);
    }

    #[test]
    fn centre_is_plains_and_start_is_walkable() {
        for seed in 0..20 {
            let m = Map::generate(96, 3, &mut ChaCha8Rng::seed_from_u64(seed));
            let s = m.start_pos();
            assert!(m.get(s).walkable());
            assert_eq!(m.biome(Pos { x: 48, y: 0, z: 48 }), Biome::Plains);
        }
    }

    #[test]
    fn out_of_bounds_is_water_and_set_ignores_it() {
        let mut m = Map::generate(24, 3, &mut ChaCha8Rng::seed_from_u64(1));
        let out = Pos { x: -1, y: 0, z: 3 };
        assert_eq!(m.get(out), Tile::Water);
        m.set(out, Tile::Lava);
        assert_eq!(m.get(out), Tile::Water);
        assert!(!m.in_bounds(Pos { x: 24, y: 0, z: 0 }));
        assert!(!m.in_bounds(Pos { x: 0, y: 0, z: 24 }));
        let p = Pos { x: 2, y: 0, z: 2 };
        m.set(p, Tile::Lava);
        assert_eq!(m.get(p), Tile::Lava);
    }

    #[test]
    fn tile_properties() {
        for t in [
            Tile::Grass,
            Tile::CaveFloor,
            Tile::Wheat,
            Tile::Shelter,
            Tile::Door,
        ] {
            assert!(t.walkable(), "{t:?}");
            let mobs = !matches!(t, Tile::Shelter | Tile::Door);
            assert_eq!(t.mob_walkable(), mobs, "{t:?}");
        }
        assert!(Tile::Door.notable() && !Tile::Stone.mob_walkable());
        assert!(Tile::Shelter.notable() && Tile::Wheat.notable());
        assert_eq!(Tile::Shelter.name(), "shelter");
        for t in [
            Tile::Stone,
            Tile::Water,
            Tile::Lava,
            Tile::IronOre,
            Tile::Planks,
            Tile::Tree { chops_left: 1 },
        ] {
            assert!(!t.walkable(), "{t:?}");
            assert!(t.notable());
        }
        assert!(!Tile::Grass.notable() && !Tile::CaveFloor.notable());
        assert_eq!(Tile::Tree { chops_left: 2 }.name(), "oak_log");
        let names: Vec<_> = [
            Tile::Grass,
            Tile::Stone,
            Tile::IronOre,
            Tile::Water,
            Tile::Lava,
            Tile::CaveFloor,
            Tile::Wheat,
            Tile::Planks,
            Tile::Door,
        ]
        .iter()
        .map(|t| t.name())
        .collect();
        assert_eq!(
            names,
            [
                "grass_block",
                "stone",
                "iron_ore",
                "water",
                "lava",
                "cave_floor",
                "wheat",
                "oak_planks",
                "oak_door"
            ]
        );
    }

    #[test]
    fn cells_are_12_by_12_and_x_is_not_z() {
        let mut m = Map::filled(50, Tile::Grass, Biome::Plains);
        m.set_biome(Pos { x: 30, y: 0, z: 5 }, Biome::Cave);
        let b = |m: &Map, x, z| m.biome(Pos { x, y: 0, z });
        assert_eq!(b(&m, 24, 0), Biome::Cave);
        assert_eq!(b(&m, 35, 11), Biome::Cave);
        assert_eq!(b(&m, 36, 11), Biome::Plains);
        assert_eq!(b(&m, 30, 12), Biome::Plains);
        assert_eq!(b(&m, 5, 30), Biome::Plains, "x and z are not swapped");
        m.set_biome(Pos { x: 49, y: 0, z: 49 }, Biome::Lake);
        assert_eq!(b(&m, 48, 48), Biome::Lake, "a partial last cell exists");
        assert_eq!(
            b(&m, 60, 60),
            Biome::Lake,
            "off-map reads clamp to the edge cell"
        );
        assert_eq!(b(&m, 47, 47), Biome::Plains);
    }

    #[test]
    fn off_map_reads_clamp_on_an_exact_multiple_of_12() {
        let mut m = Map::filled(48, Tile::Grass, Biome::Plains);
        m.set_biome(Pos { x: 47, y: 0, z: 0 }, Biome::Cave);
        assert_eq!(m.biome(Pos { x: 100, y: 0, z: 0 }), Biome::Cave);
        assert_eq!(m.biome(Pos { x: 0, y: 0, z: 100 }), Biome::Plains);
    }

    #[test]
    fn start_is_the_nearest_walkable_tile_to_the_centre() {
        let mut m = Map::filled(24, Tile::Stone, Biome::Hills);
        assert_eq!(
            m.start_pos(),
            Pos { x: 12, y: 0, z: 12 },
            "nothing walkable: centre"
        );
        m.set(Pos { x: 15, y: 0, z: 14 }, Tile::Grass);
        m.set(Pos { x: 10, y: 0, z: 9 }, Tile::Grass);
        assert_eq!(
            m.start_pos(),
            Pos { x: 10, y: 0, z: 9 },
            "ring 3, north-west first"
        );
        m.set(Pos { x: 13, y: 0, z: 10 }, Tile::Wheat);
        assert_eq!(
            m.start_pos(),
            Pos { x: 13, y: 0, z: 10 },
            "ring 2 beats ring 3"
        );
    }

    #[test]
    fn all_biomes_appear_across_seeds() {
        let mut seen = std::collections::HashSet::new();
        for seed in 0..10 {
            let m = Map::generate(96, 3, &mut ChaCha8Rng::seed_from_u64(seed));
            for z in (0..96).step_by(12) {
                for x in (0..96).step_by(12) {
                    seen.insert(format!("{:?}", m.biome(Pos { x, y: 0, z })));
                }
            }
        }
        assert_eq!(seen.len(), 5);
    }
}
