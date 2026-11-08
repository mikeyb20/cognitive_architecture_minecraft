//! Grid geometry shared by the map, mobs and the world. Every neighbourhood
//! walk goes through these helpers, so their order and reach are tested once.

use host_api::Pos;

/// Ceiling division for positive sizes: cells of side `b` needed to cover `a`.
pub fn div_ceil(a: i32, b: i32) -> i32 {
    (a + b - 1) / b
}

/// `p` moved by (dx, dz).
pub fn offset(p: Pos, dx: i32, dz: i32) -> Pos {
    Pos {
        x: p.x + dx,
        y: p.y,
        z: p.z + dz,
    }
}

/// Chebyshev distance: king-move steps between two tiles.
pub fn cheb(a: Pos, b: Pos) -> i32 {
    (a.x - b.x).abs().max((a.z - b.z).abs())
}

/// Straight-line distance between two tiles.
pub fn dist(a: Pos, b: Pos) -> f32 {
    let dx = (a.x - b.x) as f32;
    let dz = (a.z - b.z) as f32;
    (dx * dx + dz * dz).sqrt()
}

/// Every tile within Chebyshev distance `r` of `c`, row by row from the
/// north-west corner (−x, −z first).
pub fn square(c: Pos, r: i32) -> impl Iterator<Item = Pos> {
    (-r..=r).flat_map(move |dz| (-r..=r).map(move |dx| offset(c, dx, dz)))
}

/// Tiles at exactly Chebyshev distance `r` from `c`, in [`square`] order.
pub fn ring(c: Pos, r: i32) -> impl Iterator<Item = Pos> {
    square(c, r).filter(move |q| cheb(*q, c) == r)
}

/// The four orthogonal neighbours: east, west, south, north.
pub fn neighbours4(p: Pos) -> [Pos; 4] {
    [
        offset(p, 1, 0),
        offset(p, -1, 0),
        offset(p, 0, 1),
        offset(p, 0, -1),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i32, z: i32) -> Pos {
        Pos { x, y: 0, z }
    }

    #[test]
    fn ceiling_division() {
        assert_eq!(div_ceil(48, 12), 4);
        assert_eq!(div_ceil(49, 12), 5);
        assert_eq!(div_ceil(97, 8), 13);
        assert_eq!(div_ceil(1, 12), 1);
        assert_eq!(div_ceil(13, 6), 3);
    }

    #[test]
    fn offsets_and_distances() {
        assert_eq!(
            offset(Pos { x: 2, y: 7, z: 3 }, -5, 4),
            Pos { x: -3, y: 7, z: 7 }
        );
        assert_eq!(cheb(p(0, 0), p(3, -5)), 5);
        assert_eq!(cheb(p(-4, 1), p(2, 2)), 6);
        assert_eq!(cheb(p(2, 2), p(2, 2)), 0);
        assert_eq!(dist(p(0, 0), p(3, 4)), 5.0);
        assert_eq!(dist(p(1, 1), p(-2, -3)), 5.0);
    }

    #[test]
    fn square_is_row_major_from_the_north_west() {
        let s: Vec<Pos> = square(p(10, 20), 1).collect();
        assert_eq!(
            s,
            [
                p(9, 19),
                p(10, 19),
                p(11, 19),
                p(9, 20),
                p(10, 20),
                p(11, 20),
                p(9, 21),
                p(10, 21),
                p(11, 21)
            ]
        );
        assert_eq!(square(p(0, 0), 3).count(), 49);
        assert_eq!(square(p(0, 0), 0).collect::<Vec<_>>(), [p(0, 0)]);
    }

    #[test]
    fn ring_is_the_edge_only() {
        assert_eq!(ring(p(0, 0), 0).collect::<Vec<_>>(), [p(0, 0)]);
        let r: Vec<Pos> = ring(p(5, 5), 2).collect();
        assert_eq!(r.len(), 16);
        assert!(r.iter().all(|q| cheb(*q, p(5, 5)) == 2));
        assert_eq!(r[0], p(3, 3));
    }

    #[test]
    fn neighbours_are_orthogonal() {
        assert_eq!(neighbours4(p(1, 1)), [p(2, 1), p(0, 1), p(1, 2), p(1, 0)]);
    }
}
