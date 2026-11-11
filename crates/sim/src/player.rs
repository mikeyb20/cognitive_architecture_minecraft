//! Scripted player (spec §21): idle, or issues commands from the scenario
//! list. A command arrives as chat ("get <item>"), and while it is open the
//! candidates that serve its next step are marked `commanded`, so they get
//! the external-drive floor (spec §16, #41). The player's presence feeds the
//! social signal (spec §5).

use host_api::command::mark_step;
use host_api::ontology::{DUSK, SHELTER_GOAL};
use host_api::{Candidate, ChatMsg, Observation, Pos, Seen, SeenKind, ThingId};

/// The player's name in chat and perception.
pub const PLAYER: &str = "steve";
/// How far from the agent the player stands, in tiles.
pub const PLAYER_DISTANCE: i32 = 3;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptedPlayer {
    /// (tick, item) commands in order.
    pub commands: Vec<(u64, String)>,
    /// The player stands next to the agent (social signal).
    pub present: bool,
    /// At each dusk, ask for a shelter unless the agent is in one that
    /// passes the shelter test (plan §5.0 shared floor; spec §27 step 16).
    pub dusk_call: bool,
    called_day: Option<u64>,
    next: usize,
    active: Option<String>,
}

impl ScriptedPlayer {
    pub fn idle() -> Self {
        Self::default()
    }

    pub fn commanding(commands: Vec<(u64, String)>) -> Self {
        Self {
            commands,
            present: true,
            ..Self::default()
        }
    }

    /// The dusk call (plan §5.0 shared floor): once per day, from dusk on,
    /// "get shelter" in chat while the agent is not in a shelter that
    /// passes the test. `day_ticks` is the length of a day.
    pub fn call_at_dusk(&mut self, obs: &mut Observation, day_ticks: u64, sheltered: bool) {
        let day = obs.tick / day_ticks;
        if !self.dusk_call || sheltered || obs.day_phase < DUSK || self.called_day == Some(day) {
            return;
        }
        self.called_day = Some(day);
        obs.chat.push(ChatMsg {
            from: PLAYER.into(),
            text: format!("get {SHELTER_GOAL}"),
        });
    }

    /// The open command, if any.
    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// Add chat and the player to one observation and mark commanded
    /// candidates. Call after `perceive` and `candidate_actions`.
    pub fn apply(&mut self, obs: &mut Observation, cands: &mut [Candidate]) {
        if self
            .active
            .as_ref()
            .is_some_and(|g| obs.inventory.count(g) > 0)
        {
            self.active = None;
        }
        while let Some((t, item)) = self.commands.get(self.next) {
            if *t > obs.tick {
                break;
            }
            obs.chat.push(ChatMsg {
                from: PLAYER.into(),
                text: format!("get {item}"),
            });
            self.active = Some(item.clone());
            self.next += 1;
        }
        if self.present {
            let p = obs.position;
            obs.visible.push(Seen {
                thing: ThingId(format!("player:{PLAYER}")),
                kind: SeenKind::Player,
                pos: Pos {
                    x: p.x + PLAYER_DISTANCE,
                    ..p
                },
                distance: PLAYER_DISTANCE as f32,
            });
        }
        if let Some(goal) = &self.active {
            mark_step(&obs.inventory, obs, cands, goal, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use host_api::{AgentId, Inventory, Skill, Target, TaskType};

    fn obs(tick: u64) -> Observation {
        Observation {
            agent: AgentId(0),
            tick,
            day_phase: 0.1,
            health: 1.0,
            food: 1.0,
            position: Pos { x: 5, y: 0, z: 5 },
            inventory: Inventory::default(),
            visible: vec![Seen {
                thing: ThingId("block:oak_log".into()),
                kind: SeenKind::Block,
                pos: Pos { x: 6, y: 0, z: 5 },
                distance: 1.0,
            }],
            chat: vec![],
            damage_taken: 0.0,
            resting: false,
            sheltered: false,
        }
    }

    fn cand(skill: Skill, target: Target) -> Candidate {
        Candidate {
            id: 0,
            skill,
            target: Some(target),
            task_type: TaskType::Mining,
            est_cost: 0.1,
            est_risk: 0.0,
            commanded: false,
        }
    }

    fn cands() -> Vec<Candidate> {
        vec![
            cand(Skill::Chop, Target::Pos(Pos { x: 6, y: 0, z: 5 })),
            cand(Skill::Chop, Target::Pos(Pos { x: 9, y: 0, z: 9 })),
            cand(Skill::Craft, Target::Item("oak_planks".into())),
            cand(Skill::Explore, Target::Pos(Pos { x: 6, y: 0, z: 5 })),
        ]
    }

    #[test]
    fn idle_player_changes_nothing() {
        let mut p = ScriptedPlayer::idle();
        let mut o = obs(0);
        let mut c = cands();
        p.apply(&mut o, &mut c);
        assert_eq!((o.chat.len(), o.visible.len()), (0, 1));
        assert!(c.iter().all(|c| !c.commanded));
        assert_eq!(p.active(), None);
    }

    #[test]
    fn command_arrives_on_time_and_marks_its_step() {
        let mut p = ScriptedPlayer::commanding(vec![(10, "oak_planks".into())]);
        let mut o = obs(9);
        let mut c = cands();
        p.apply(&mut o, &mut c);
        assert!(o.chat.is_empty() && c.iter().all(|c| !c.commanded));
        assert_eq!(o.visible.len(), 2, "the player stands nearby");
        assert_eq!(o.visible[1].kind, SeenKind::Player);
        let beside = Pos {
            x: 5 + PLAYER_DISTANCE,
            y: 0,
            z: 5,
        };
        assert_eq!((o.visible[1].pos, o.visible[1].distance), (beside, 3.0));
        let mut o = obs(10);
        let mut c = cands();
        p.apply(&mut o, &mut c);
        assert_eq!(o.chat[0].text, "get oak_planks");
        assert_eq!(o.chat[0].from, PLAYER);
        assert_eq!(p.active(), Some("oak_planks"));
        let marked: Vec<bool> = c.iter().map(|c| c.commanded).collect();
        assert_eq!(marked, vec![true, false, false, false], "chop the seen log");
        // With a log in hand the step is crafting.
        let mut o = obs(11);
        o.inventory.items.push(("oak_log".into(), 1));
        let mut c = cands();
        p.apply(&mut o, &mut c);
        assert!(o.chat.is_empty(), "said once");
        let marked: Vec<bool> = c.iter().map(|c| c.commanded).collect();
        assert_eq!(marked, vec![false, false, true, false]);
        // Done once the item is held.
        let mut o = obs(12);
        o.inventory.items.push(("oak_planks".into(), 4));
        let mut c = cands();
        p.apply(&mut o, &mut c);
        assert_eq!(p.active(), None);
        assert!(c.iter().all(|c| !c.commanded));
    }

    #[test]
    fn unknown_goal_marks_nothing() {
        let mut p = ScriptedPlayer::commanding(vec![(0, "diamond".into())]);
        let mut o = obs(0);
        let mut c = cands();
        p.apply(&mut o, &mut c);
        assert_eq!(p.active(), Some("diamond"));
        assert!(c.iter().all(|c| !c.commanded));
    }

    #[test]
    fn the_dusk_call_asks_once_a_day_from_dusk_unless_sheltered() {
        // Days of 100 ticks; `tick` sets the day, `phase` the time of day.
        let calls = |p: &mut ScriptedPlayer, phase, tick, sheltered| {
            let mut o = Observation {
                day_phase: phase,
                ..obs(tick)
            };
            p.call_at_dusk(&mut o, 100, sheltered);
            o.chat
        };
        let mut off = ScriptedPlayer::idle();
        assert!(calls(&mut off, 0.5, 50, false).is_empty(), "off by default");
        let mut p = ScriptedPlayer {
            dusk_call: true,
            ..ScriptedPlayer::idle()
        };
        assert!(calls(&mut p, 0.41, 41, false).is_empty(), "before dusk");
        assert!(calls(&mut p, DUSK, 42, true).is_empty(), "already in one");
        let c = calls(&mut p, DUSK, 42, false);
        assert_eq!(c.len(), 1);
        assert_eq!(
            (c[0].from.as_str(), c[0].text.as_str()),
            (PLAYER, "get shelter")
        );
        assert!(calls(&mut p, 0.9, 90, false).is_empty(), "once that day");
        assert_eq!(
            calls(&mut p, 0.5, 150, false).len(),
            1,
            "and again the next"
        );
    }
}
