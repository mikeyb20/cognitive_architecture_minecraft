//! Pins the sim's default rules (spec §21) so any change is deliberate and
//! shows up in review. Values are proposals until the sim pilot.

use sim::Rules;

#[test]
fn default_rules_are_pinned() {
    let r = Rules::default();
    assert_eq!(r.map_size, 96);
    assert_eq!(r.day_ticks, 2400);
    // Spec §21 proposed 1/600; 1/2400 is the step 3 proposal (flagged in the PR).
    assert_eq!(r.food_decay, 1.0 / 2400.0);
    assert_eq!(r.starve_damage, 1.0 / 300.0);
    assert_eq!((r.regen, r.regen_food_min), (1.0 / 600.0, 0.8));
    assert_eq!(r.view_radius, 8);
    assert_eq!((r.walk_ticks, r.sprint_ticks, r.max_move), (4, 2, 8));
    assert_eq!((r.night_spawn, r.cave_spawn), (0.02, 0.005));
    assert_eq!((r.spawn_min, r.spawn_max, r.max_hostiles), (6, 12, 6));
    assert_eq!((r.mob_move_ticks, r.chase_radius), (6, 16));
    assert_eq!((r.mob_damage, r.mob_hit_ticks), (0.1, 20));
    assert_eq!((r.meat_food, r.bread_food), (0.3, 0.5));
    assert_eq!((r.shelter_ticks_blocks, r.shelter_ticks_dig), (20, 6));
    assert_eq!(
        (r.base_success, r.tree_chops, r.animal_density),
        (0.95, 3, 1.0)
    );
    assert_eq!((r.min_effort, r.sprint_effort), (0.05, 0.6));
    assert_eq!(
        (
            r.chop_ticks,
            r.farm_ticks,
            r.craft_ticks,
            r.eat_ticks,
            r.swing_ticks
        ),
        (30, 20, 10, 30, 10)
    );
    assert_eq!(r.mine_ticks, [40, 30, 20]);
    assert_eq!((r.swing_damage, r.max_swings), (0.34, 4));
    assert_eq!(
        (r.rest_ticks, r.threat_penalty, r.blocked_ticks),
        (300, 0.75, 5)
    );
}
