//! Minecraft as a `Host` (spec §2, §22): observations and outcomes come over
//! the link; candidates, place memory and player commands are worked out
//! here in Rust, so every arm sees the same choices as in the sim.

use std::collections::BTreeSet;

use host_api::command::{CommandQueue, QueueParams};
use host_api::link::{BotConfig, LinkReply, LinkRequest, RecipeHint};
use host_api::{Candidate, Host, Intensity, Inventory, Observation, Outcome, Skill, Target};
use minecraft::{McParams, Places, candidates};

use super::link::{Link, LinkError};

pub struct McHost {
    link: Link,
    names: BTreeSet<String>,
    places: Places,
    queue: CommandQueue,
    params: McParams,
    last: Observation,
    /// The first link error; the run stops once it is set.
    pub error: Option<LinkError>,
}

/// A failed action that took no time, for when the link is down.
fn not_done() -> Outcome {
    Outcome {
        success: false,
        progress: 0.0,
        items_gained: Inventory::default(),
        damage: 0.0,
        new_things: vec![],
        blocked: true,
        duration_ticks: 0,
    }
}

impl McHost {
    /// Say hello with `config` and read the first observation.
    pub fn connect(mut link: Link, config: BotConfig, params: McParams) -> Result<Self, LinkError> {
        let names = match link.call(&LinkRequest::Hello { config })? {
            LinkReply::Ready { names } => names.into_iter().collect(),
            other => return Err(LinkError::Unexpected(format!("{other:?}"))),
        };
        let last = observe(&mut link)?;
        Ok(Self {
            link,
            names,
            places: Places::default(),
            queue: CommandQueue::new(QueueParams::default()),
            params,
            last,
            error: None,
        })
    }

    /// Player commands in progress and waiting.
    pub fn queue(&self) -> &CommandQueue {
        &self.queue
    }

    pub fn places(&self) -> &Places {
        &self.places
    }

    fn say(&mut self, text: String) {
        let r = self.link.call(&LinkRequest::Say { text });
        if let Err(e) = r {
            self.error.get_or_insert(e);
        }
    }
}

fn observe(link: &mut Link) -> Result<Observation, LinkError> {
    match link.call(&LinkRequest::Perceive)? {
        LinkReply::Observed { obs } => Ok(obs),
        other => Err(LinkError::Unexpected(format!("{other:?}"))),
    }
}

impl Host for McHost {
    /// The bot's view now; the last one again if the link fails.
    fn perceive(&mut self) -> Observation {
        match observe(&mut self.link) {
            Ok(obs) => self.last = obs,
            Err(e) => {
                self.error.get_or_insert(e);
            }
        }
        self.last.clone()
    }

    /// Remember what is in view, answer chat commands, then offer the
    /// candidates with command steps marked (spec §6, #40).
    fn candidate_actions(&mut self, obs: &Observation) -> Vec<Candidate> {
        self.places.see(obs);
        let mut replies = Vec::new();
        for msg in &obs.chat {
            let names = &self.names;
            replies.extend(self.queue.hear(msg, obs, |n| names.contains(n)));
        }
        replies.extend(self.queue.update(obs));
        for r in replies {
            self.say(r);
        }
        let mut c = candidates(obs, &self.places, &self.params);
        self.queue.mark(obs, &self.places, &mut c);
        c
    }

    /// Run one action on the bot. Crafts carry the recipe, so the bot knows
    /// which station to find or place.
    fn execute(&mut self, action: &Candidate, intensity: Intensity) -> Outcome {
        let recipe = match (&action.skill, &action.target) {
            (Skill::Craft, Some(Target::Item(item))) => RecipeHint::for_item(item),
            _ => None,
        };
        let req = LinkRequest::Execute {
            action: action.clone(),
            intensity,
            recipe,
        };
        match self.link.call(&req) {
            Ok(LinkReply::Done { outcome }) => outcome,
            Ok(other) => {
                self.error
                    .get_or_insert(LinkError::Unexpected(format!("{other:?}")));
                not_done()
            }
            Err(e) => {
                self.error.get_or_insert(e);
                not_done()
            }
        }
    }
}
