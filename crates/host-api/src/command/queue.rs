//! The command queue (spec §22, kept from v0.2 Step 18): one task runs, up
//! to `max_queued` wait behind it, and each finishes, is cancelled or is
//! given up after `stall_limit` decisions without progress. Every change is
//! answered with a chat line, which the host says to the players.

use std::collections::{BTreeMap, VecDeque};

use super::{Command, Parsed, Whereabouts, mark_step, parse};
use crate::craft::drop_of;
use crate::planner::steps_left_for;
use crate::{Candidate, ChatMsg, Observation, SeenKind, Skill, Target};

/// Queue limits (v0.2 Step 18 and Step 20).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueueParams {
    /// Tasks that may wait behind the current one (v0.2 Step 18: 5).
    pub max_queued: usize,
    /// Decisions without progress before a task is given up (v0.2 Step 20
    /// test 5 asks for a hard limit; the value is proposed until the pilot).
    pub stall_limit: u32,
    /// How close "come" gets to the player, in blocks (v0.2 Step 20 test 8).
    pub come_distance: f32,
}

impl Default for QueueParams {
    fn default() -> Self {
        Self {
            max_queued: 5,
            stall_limit: 30,
            come_distance: 3.0,
        }
    }
}

/// One thing the agent was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    /// Gain `count` of `item`. Asking for a block counts what it drops
    /// ("mine iron ore" counts `raw_iron`).
    Get { item: String, count: u32 },
    /// Stay near `player` until told to stay; any new task replaces it.
    Follow { player: String },
    /// Walk to `player` once.
    Come { player: String },
}

impl Task {
    fn describe(&self) -> String {
        match self {
            Task::Get { item, count } => format!("{count} {item}"),
            Task::Follow { player } => format!("following {player}"),
            Task::Come { player } => format!("coming to {player}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Active {
    task: Task,
    /// Count of the item held when the task started.
    start: u32,
    /// Lowest distance-to-done seen so far (steps left, or blocks away).
    best: Option<u32>,
    stall: u32,
}

/// Player commands waiting and running (v0.2 Step 18).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CommandQueue {
    params: QueueParams,
    current: Option<Active>,
    waiting: VecDeque<Task>,
    last_count: BTreeMap<String, u32>,
}

fn player_distance(obs: &Observation, player: &str) -> Option<f32> {
    let id = format!("player:{player}");
    obs.visible
        .iter()
        .find(|s| s.kind == SeenKind::Player && s.thing.0 == id)
        .map(|s| s.distance)
}

impl CommandQueue {
    pub fn new(params: QueueParams) -> Self {
        Self {
            params,
            ..Self::default()
        }
    }

    /// The running task.
    pub fn current(&self) -> Option<&Task> {
        self.current.as_ref().map(|a| &a.task)
    }

    /// Tasks waiting, in order.
    pub fn waiting(&self) -> impl Iterator<Item = &Task> {
        self.waiting.iter()
    }

    /// Read one chat line and act on it. Returns the reply, if any.
    pub fn hear(
        &mut self,
        msg: &ChatMsg,
        obs: &Observation,
        known: impl Fn(&str) -> bool,
    ) -> Option<String> {
        let cmd = match parse(&msg.text, known) {
            Parsed::Chat => return None,
            Parsed::Unknown(why) => return Some(why),
            Parsed::Command(c) => c,
        };
        let player = msg.from.clone();
        Some(match cmd {
            Command::Get { item, count } => {
                let item = drop_of(&item).to_string();
                let count = count
                    .or_else(|| self.last_count.get(&item).copied())
                    .unwrap_or(1);
                self.last_count.insert(item.clone(), count);
                let target = obs.inventory.count(&item) + count;
                if steps_left_for(&obs.inventory, &item, target).is_none() {
                    return Some(format!("I don't know how to get {item} yet."));
                }
                self.submit(Task::Get { item, count }, obs)
            }
            Command::Follow => self.submit(Task::Follow { player }, obs),
            Command::Come => self.submit(Task::Come { player }, obs),
            Command::Stay => match self.current() {
                Some(Task::Follow { .. }) => self.finish("Staying.", obs),
                _ => "I'm not following anyone.".into(),
            },
            Command::Status => self.status(obs),
            Command::Cancel => match self.current() {
                Some(t) => self.finish(&format!("Cancelled {}.", t.describe()), obs),
                None => "Nothing to cancel.".into(),
            },
            Command::Stop => {
                self.current = None;
                self.waiting.clear();
                "Stopped. Queue cleared.".into()
            }
        })
    }

    fn submit(&mut self, task: Task, obs: &Observation) -> String {
        match &self.current {
            None => self.start(task, obs),
            Some(Active {
                task: Task::Follow { .. },
                ..
            }) => format!("Stopped following. {}", self.start(task, obs)),
            Some(a) if self.waiting.len() < self.params.max_queued => {
                let reply = format!(
                    "Queued {} as #{}, after {}.",
                    task.describe(),
                    self.waiting.len() + 2,
                    a.task.describe()
                );
                self.waiting.push_back(task);
                reply
            }
            Some(_) => format!("My queue is full ({}).", self.params.max_queued),
        }
    }

    fn start(&mut self, task: Task, obs: &Observation) -> String {
        let start = match &task {
            Task::Get { item, .. } => obs.inventory.count(item),
            _ => 0,
        };
        let reply = format!("On it: {}.", task.describe());
        self.current = Some(Active {
            task,
            start,
            best: None,
            stall: 0,
        });
        reply
    }

    /// End the current task with `said`, then start the next one.
    fn finish(&mut self, said: &str, obs: &Observation) -> String {
        self.current = None;
        match self.waiting.pop_front() {
            Some(next) => format!("{said} {}", self.start(next, obs)),
            None => format!("{said} Ready for commands."),
        }
    }

    /// Distance to done (lower is better): steps left for an item, blocks
    /// for "come"; `None` when unknown. Following is never done.
    fn remaining(&self, a: &Active, obs: &Observation) -> Option<u32> {
        match &a.task {
            Task::Get { item, count } => steps_left_for(&obs.inventory, item, a.start + count),
            Task::Come { player } => {
                let d = player_distance(obs, player)?;
                Some(if d <= self.params.come_distance {
                    0
                } else {
                    d.ceil() as u32
                })
            }
            Task::Follow { .. } => None,
        }
    }

    /// Call once per decision: finishes a task that is done, gives up one
    /// that made no progress for `stall_limit` decisions, and starts the
    /// next. Returns what to say.
    pub fn update(&mut self, obs: &Observation) -> Option<String> {
        let mut a = self.current.clone()?;
        if matches!(a.task, Task::Follow { .. }) {
            return None;
        }
        let left = self.remaining(&a, obs);
        if left == Some(0) {
            return Some(self.finish(&format!("Done: {}.", a.task.describe()), obs));
        }
        if left.is_some() && (a.best.is_none() || left < a.best) {
            a.best = left;
            a.stall = 0;
        } else {
            a.stall += 1;
        }
        if a.stall >= self.params.stall_limit {
            let why = format!("I couldn't finish {}; giving up.", a.task.describe());
            return Some(self.finish(&why, obs));
        }
        self.current = Some(a);
        None
    }

    /// The `!status` reply.
    pub fn status(&self, obs: &Observation) -> String {
        let now = match &self.current {
            None => "Idle.".to_string(),
            Some(a) => match &a.task {
                Task::Get { item, count } => {
                    let got = obs.inventory.count(item).saturating_sub(a.start);
                    format!("Working on {item}: {got}/{count}.")
                }
                t => format!("Now {}.", t.describe()),
            },
        };
        if self.waiting.is_empty() {
            return format!("{now} Queue: empty.");
        }
        let list: Vec<String> = self.waiting.iter().map(Task::describe).collect();
        format!("{now} Queue: {}.", list.join(", "))
    }

    /// Mark the candidates that serve the current task (spec §16, #41).
    /// `known` says where things are: the observation itself, or a host's
    /// memory of places as well.
    pub fn mark(&self, obs: &Observation, known: &impl Whereabouts, cands: &mut [Candidate]) {
        let Some(a) = &self.current else { return };
        match &a.task {
            Task::Get { item, count } => {
                mark_step(&obs.inventory, known, cands, item, a.start + count);
            }
            Task::Follow { player } | Task::Come { player } => {
                let to_player = Some(Target::Player(player.clone()));
                for c in cands.iter_mut() {
                    if matches!(c.skill, Skill::FollowPlayer | Skill::MoveTo)
                        && c.target == to_player
                    {
                        c.commanded = true;
                    }
                }
            }
        }
    }
}
