// Skill names match the wire format (spec §2).

use host_api::Skill;

#[test]
fn skill_names_are_the_serde_names_and_all_is_complete() {
    for s in Skill::ALL {
        assert_eq!(serde_json::to_value(s).unwrap(), s.name());
    }
    let mut names: Vec<_> = Skill::ALL.iter().map(|s| s.name()).collect();
    names.dedup();
    assert_eq!(names.len(), 14);
    assert_eq!(Skill::FollowPlayer.name(), "follow_player");
}
