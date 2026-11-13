//! The shared teacher for the arms without a brain (plan §5.0, #60; 01
//! §4.11). When the player asks for a shelter, the teacher shows the same
//! recipe the brain is shown (`brain::shelter_recipe`), and marks the
//! candidates that do its next step `commanded`, the way the host marks a
//! player command's next step (spec §16). Arm A follows commanded steps and
//! arm B reads them in its prompt. Unlike the brain's, this recipe never
//! fades: the baselines have no ρ. The lesson ends when the shelter test
//! passes, as the brain's goal does (#58).

use cognitive_core::control::teacher::{Lesson, step_matches};
use cognitive_core::direction::target_key;
use host_api::{Candidate, Observation, Skill};
use sim::world::ShelterTest;

use crate::brain::{SHELTER, shelter_recipe};
use crate::teachability::teaching;

/// The teacher beside a baseline arm (plan §5.0).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Teacher {
    /// Off is the teacher-off rerun (plan §5.0).
    pub show: bool,
    /// The tick it stops showing new lessons (plan §5.0 teachability).
    pub until: Option<u64>,
    lesson: Option<Lesson>,
    /// The last chosen action, to move the lesson on when it succeeds.
    chosen: Option<(Skill, Option<String>)>,
}

impl Teacher {
    pub fn new(show: bool, until: Option<u64>) -> Self {
        Self {
            show,
            until,
            ..Self::default()
        }
    }

    /// Hear chat: "get shelter" opens a lesson from what the agent holds,
    /// unless one is open (01 §4.11: shown once per goal) or the teacher
    /// has stopped.
    pub fn hear(&mut self, obs: &Observation) {
        let asked = obs
            .chat
            .iter()
            .any(|m| m.text.strip_prefix("get ").map(str::trim) == Some(SHELTER));
        let open = self.show && teaching(self.until, obs.tick);
        if open && asked && self.lesson.is_none() {
            self.lesson = Some(Lesson::new(shelter_recipe(&obs.inventory)));
        }
    }

    /// A passing shelter test ends the lesson (#58).
    pub fn see(&mut self, test: ShelterTest) {
        if test.passes() {
            self.lesson = None;
        }
    }

    /// The candidates with the next step marked `commanded`.
    pub fn mark(&self, obs: &Observation, cands: &[Candidate]) -> Vec<Candidate> {
        let step = self.lesson.as_ref().and_then(Lesson::step);
        cands
            .iter()
            .map(|c| {
                let taught =
                    step.is_some_and(|s| step_matches(s, c.skill, target_key(obs, c).as_deref()));
                Candidate {
                    commanded: c.commanded || taught,
                    ..c.clone()
                }
            })
            .collect()
    }

    /// The arm chose `c`.
    pub fn chose(&mut self, obs: &Observation, c: &Candidate) {
        self.chosen = Some((c.skill, target_key(obs, c)));
    }

    /// The chosen action's result: a success that was the next step moves
    /// the lesson on (01 §4.11).
    pub fn outcome(&mut self, success: bool) {
        if let (Some(l), Some((skill, key))) = (self.lesson.as_mut(), self.chosen.take()) {
            l.record(skill, key.as_deref(), success);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use host_api::{AgentId, ChatMsg, Inventory, Pos, Seen, SeenKind, Target, TaskType, ThingId};

    pub(crate) fn obs(chat: &[&str], inv: &[(&str, u32)]) -> Observation {
        Observation {
            agent: AgentId(0),
            tick: 0,
            day_phase: 0.5,
            health: 1.0,
            food: 1.0,
            position: Pos { x: 5, y: 0, z: 5 },
            inventory: Inventory {
                items: inv.iter().map(|(i, n)| ((*i).to_string(), *n)).collect(),
            },
            visible: vec![Seen {
                thing: ThingId("block:oak_log".into()),
                kind: SeenKind::Block,
                pos: Pos { x: 6, y: 0, z: 5 },
                distance: 1.0,
            }],
            chat: chat
                .iter()
                .map(|t| ChatMsg {
                    from: "steve".into(),
                    text: (*t).to_string(),
                })
                .collect(),
            damage_taken: 0.0,
            resting: false,
            sheltered: false,
        }
    }

    fn cand(skill: Skill, target: Option<Target>) -> Candidate {
        Candidate {
            id: 0,
            skill,
            target,
            task_type: TaskType::Mining,
            est_cost: 0.1,
            est_risk: 0.0,
            commanded: false,
        }
    }

    /// Chop the seen log, craft planks, build, explore.
    pub(crate) fn cands() -> Vec<Candidate> {
        vec![
            cand(Skill::Chop, Some(Target::Pos(Pos { x: 6, y: 0, z: 5 }))),
            cand(Skill::Craft, Some(Target::Item("oak_planks".into()))),
            cand(Skill::Shelter, None),
            cand(Skill::Explore, Some(Target::Pos(Pos { x: 9, y: 0, z: 9 }))),
        ]
    }

    fn marked(t: &Teacher, o: &Observation) -> Vec<bool> {
        t.mark(o, &cands()).iter().map(|c| c.commanded).collect()
    }

    fn passing(yes: bool) -> ShelterTest {
        ShelterTest {
            enclosed: yes,
            exit: true,
        }
    }

    #[test]
    fn a_shelter_call_is_taught_step_by_step_until_the_test_passes() {
        let mut t = Teacher::new(true, None);
        let quiet = obs(&[], &[]);
        t.hear(&quiet);
        assert_eq!(marked(&t, &quiet), [false; 4], "nothing asked");
        let asked = obs(&["get oak_planks", "get  shelter "], &[]);
        t.hear(&asked);
        assert_eq!(marked(&t, &asked), [true, false, false, false], "chop");
        // A failed chop, or another action, does not move it on.
        t.chose(&asked, &cands()[0]);
        t.outcome(false);
        t.chose(&asked, &cands()[3]);
        t.outcome(true);
        assert_eq!(marked(&t, &asked), [true, false, false, false]);
        t.chose(&asked, &cands()[0]);
        t.outcome(true);
        // Hearing the call again keeps the open lesson.
        t.hear(&asked);
        assert_eq!(marked(&t, &asked), [false, true, false, false], "craft");
        t.chose(&asked, &cands()[1]);
        t.outcome(true);
        assert_eq!(marked(&t, &asked), [false, false, true, false], "build");
        t.see(passing(false));
        assert_eq!(marked(&t, &asked), [false, false, true, false]);
        t.see(passing(true));
        assert_eq!(marked(&t, &asked), [false; 4], "done by test");
        // The next call opens a new lesson from what is held now.
        let stone = obs(&["get shelter"], &[("cobblestone", 4)]);
        t.hear(&stone);
        assert_eq!(marked(&t, &stone), [false, false, true, false]);
    }

    #[test]
    fn a_stopped_teacher_opens_no_new_lesson() {
        let mut t = Teacher::new(true, Some(5));
        let mut late = obs(&["get shelter"], &[]);
        late.tick = 5;
        t.hear(&late);
        assert_eq!(marked(&t, &late), [false; 4]);
        late.tick = 4;
        t.hear(&late);
        assert_eq!(marked(&t, &late), [true, false, false, false]);
    }

    #[test]
    fn with_the_teacher_off_nothing_is_marked_but_commands_stay() {
        let mut t = Teacher::new(false, None);
        let o = obs(&["get shelter"], &[]);
        t.hear(&o);
        let mut cs = cands();
        cs[2].commanded = true;
        let out: Vec<bool> = t.mark(&o, &cs).iter().map(|c| c.commanded).collect();
        assert_eq!(out, [false, false, true, false]);
        // An outcome with no lesson changes nothing.
        t.outcome(true);
        assert_eq!(t, Teacher::new(false, None));
    }
}
