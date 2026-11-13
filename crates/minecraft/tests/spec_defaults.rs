//! The Minecraft host's values, pinned so a change is deliberate. All are
//! proposed until the pilot (spec §22).

use minecraft::{McParams, PlaceParams};

#[test]
fn minecraft_host_defaults() {
    let p = McParams::default();
    assert_eq!(
        (
            p.threat_range,
            p.flee_distance,
            p.explore_distance,
            p.shelter_blocks
        ),
        (8.0, 16, 24, 8)
    );
    let m = PlaceParams::default();
    assert_eq!((m.max_per_thing, m.confirm_radius), (32, 4.0));
}
