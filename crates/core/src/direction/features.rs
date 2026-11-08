//! Per-candidate features φ(a, S) (spec §3). Knowledge comes from shared
//! memory and every mode reads the same φ (00 §7.8; 01 §2.1). Internal
//! actions fill the same features from memory, so both kinds compete on the
//! same terms (spec §6).

use crate::memory::{ActionKey, InternalAction, MemoryStore, SkillTable};
use crate::modes::{Features, N_FEATURES, feature as f};
use host_api::ontology::region_of;
use host_api::{Candidate, Observation, Skill, Target, TaskType};
use serde::{Deserialize, Serialize};

/// One step toward the active goal, in shared-ontology terms: a skill and the
/// thing it acts on (`block:oak_log`, `item:oak_planks`; empty for skills
/// with no target, such as shelter and rest).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub skill: Skill,
    pub thing: String,
    pub task: TaskType,
}

/// Relevance of goal-adjacent candidates, and of everything when there is no
/// goal or no step (spec §3 f5).
pub const ADJACENT_RELEVANCE: f32 = 0.3;
/// Host-independent cost of an internal action (proposed).
pub const INTERNAL_COST: f32 = 0.05;

/// The parts of S that features read (spec §3).
pub struct FeatureInputs<'a> {
    pub obs: &'a Observation,
    pub mem: &'a MemoryStore,
    pub skills: &'a SkillTable,
    /// Memory ids currently in thought momentum.
    pub tm: &'a [usize],
    pub step: Option<&'a PlanStep>,
    pub has_goal: bool,
    /// Task type of the current context (plan step or last action).
    pub task: TaskType,
    pub now: u64,
}

/// Values only the brain loop knows, for internal-action features (spec §6).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct InternalExtras {
    /// Share of recent episodic items not yet reflected.
    pub unreflected: f32,
    /// Novelty of the best unlinked pair, or 1 when idle with no goal.
    pub imagine_fresh: f32,
    /// Stakes of the next step.
    pub stakes: f32,
    /// Share of prune or merge candidates seen by the subconscious.
    pub weak_share: f32,
    /// The active goal has a valid plan.
    pub plan_valid: bool,
}

/// Memory key of what a candidate acts on: the perceived thing at a target
/// position, the region for movement, the item for crafting and eating.
pub fn target_key(obs: &Observation, c: &Candidate) -> Option<String> {
    match &c.target {
        Some(Target::Pos(p)) => obs
            .visible
            .iter()
            .find(|s| s.pos == *p)
            .map(|s| s.thing.0.clone())
            .or_else(|| {
                matches!(c.skill, Skill::Explore | Skill::MoveTo | Skill::Flee)
                    .then(|| region_of(*p).0)
            }),
        Some(Target::Thing(t)) => Some(t.0.clone()),
        Some(Target::Item(i)) => Some(format!("item:{i}")),
        Some(Target::Player(p)) => Some(format!("player:{p}")),
        None => None,
    }
}

impl FeatureInputs<'_> {
    /// f5: 1 for the current plan step, 0.3 goal-adjacent (same task type as
    /// the step, proposed), 0 otherwise; 0.3 for all with no goal or step.
    fn relevance(&self, c: &Candidate, key: Option<&str>) -> f32 {
        match self.step {
            Some(s) if self.has_goal => {
                let thing_ok = s.thing.is_empty() || key == Some(s.thing.as_str());
                if c.skill == s.skill && thing_ok {
                    1.0
                } else if c.task_type == s.task {
                    ADJACENT_RELEVANCE
                } else {
                    0.0
                }
            }
            _ => ADJACENT_RELEVANCE,
        }
    }

    fn in_or_linked_to_tm(&self, id: usize) -> bool {
        self.tm.contains(&id) || self.tm.iter().any(|&t| self.mem.linked(id, t))
    }

    /// φ for an external candidate (spec §3).
    pub fn external(&self, c: &Candidate) -> Features {
        let mut phi = [0.0; N_FEATURES];
        let key = target_key(self.obs, c);
        let a = ActionKey::from(c.skill);
        phi[f::BIAS] = 1.0;
        phi[f::UNCERTAINTY] = self.skills.uncertainty(a, c.task_type);
        phi[f::COMPETENCE] = self.skills.competence(a, c.task_type);
        phi[f::RELEVANCE] = self.relevance(c, key.as_deref());
        phi[f::RISK] = c.est_risk;
        phi[f::RECENT_FAIL] = self.skills.recent_fail(a, c.task_type, self.now);
        phi[f::COST] = c.est_cost;
        let Some(key) = key else {
            return phi;
        };
        let mut things = vec![key.clone()];
        if c.skill == Skill::Craft {
            things.push(key.replacen("item:", "recipe:", 1));
        }
        let unknown = things
            .iter()
            .filter(|k| !self.mem.get(k).is_some_and(|id| self.mem.is_alive(id)))
            .count();
        phi[f::UNKNOWN] = unknown as f32 / things.len() as f32;
        phi[f::SAVING] = self.mem.saving(&route_key(c.skill, &key), c.est_cost);
        match self.mem.get(&key).filter(|&id| self.mem.is_alive(id)) {
            Some(id) => {
                let h = self.mem.habituation(id, self.now);
                phi[f::FRESH] = (1.0 - h) * self.mem.novelty(&key);
                let e = self.mem.entry(id);
                phi[f::RISK] = c.est_risk.max(e.danger);
                phi[f::VALENCE_TAG] = e.item.v_enc;
                phi[f::TM_LINK] = f32::from(u8::from(self.in_or_linked_to_tm(id)));
            }
            None => phi[f::FRESH] = 1.0,
        }
        phi
    }

    /// φ for an internal action, filled from memory (spec §6 table).
    pub fn internal(&self, a: InternalAction, x: &InternalExtras) -> Features {
        let mut phi = [0.0; N_FEATURES];
        let k = ActionKey::from(a);
        phi[f::BIAS] = 1.0;
        phi[f::INTERNAL] = 1.0;
        phi[f::UNCERTAINTY] = self.skills.uncertainty(k, self.task);
        phi[f::COMPETENCE] = self.skills.competence(k, self.task);
        phi[f::RECENT_FAIL] = self.skills.recent_fail(k, self.task, self.now);
        phi[f::COST] = INTERNAL_COST;
        match a {
            InternalAction::Reflect => phi[f::UNKNOWN] = x.unreflected,
            InternalAction::Imagine => phi[f::FRESH] = x.imagine_fresh,
            InternalAction::Rehearse => phi[f::RELEVANCE] = x.stakes,
            InternalAction::Consolidate => phi[f::SAVING] = x.weak_share,
            InternalAction::Plan => {
                phi[f::SAVING] = f32::from(u8::from(self.has_goal && !x.plan_valid));
            }
        }
        phi
    }
}

/// Key for the cheapest-route index: the same result reached by the same
/// skill (spec §3 f6).
pub fn route_key(skill: Skill, target: &str) -> String {
    format!("{skill:?}:{target}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PriorityInputs, Source, Tier};
    use host_api::{AgentId, Inventory, Pos, Seen, SeenKind, ThingId};

    fn p(x: i32, z: i32) -> Pos {
        Pos { x, y: 0, z }
    }

    fn obs() -> Observation {
        Observation {
            agent: AgentId(0),
            tick: 0,
            day_phase: 0.0,
            health: 1.0,
            food: 1.0,
            position: p(0, 0),
            inventory: Inventory::default(),
            visible: vec![Seen {
                thing: ThingId("block:oak_log".into()),
                kind: SeenKind::Block,
                pos: p(1, 1),
                distance: 1.4,
            }],
            chat: vec![],
            damage_taken: 0.0,
            resting: false,
            sheltered: false,
        }
    }

    fn cand(skill: Skill, target: Option<Target>, task: TaskType) -> Candidate {
        Candidate {
            id: 0,
            skill,
            target,
            task_type: task,
            est_cost: 0.2,
            est_risk: 0.1,
            commanded: false,
        }
    }

    fn chop() -> Candidate {
        cand(Skill::Chop, Some(Target::Pos(p(1, 1))), TaskType::Mining)
    }

    #[test]
    fn target_keys() {
        let o = obs();
        assert_eq!(target_key(&o, &chop()).as_deref(), Some("block:oak_log"));
        let walk = cand(
            Skill::Explore,
            Some(Target::Pos(p(9, 17))),
            TaskType::Travel,
        );
        assert_eq!(target_key(&o, &walk).as_deref(), Some("region:1,2"));
        let mine_air = cand(Skill::Mine, Some(Target::Pos(p(9, 9))), TaskType::Mining);
        assert_eq!(target_key(&o, &mine_air), None, "nothing seen there");
        let craft = cand(
            Skill::Craft,
            Some(Target::Item("stick".into())),
            TaskType::Crafting,
        );
        assert_eq!(target_key(&o, &craft).as_deref(), Some("item:stick"));
        let thing = cand(
            Skill::Attack,
            Some(Target::Thing(ThingId("entity:pig".into()))),
            TaskType::Combat,
        );
        assert_eq!(target_key(&o, &thing).as_deref(), Some("entity:pig"));
        let pl = cand(
            Skill::FollowPlayer,
            Some(Target::Player("steve".into())),
            TaskType::Helping,
        );
        assert_eq!(target_key(&o, &pl).as_deref(), Some("player:steve"));
        assert_eq!(
            target_key(&o, &cand(Skill::Rest, None, TaskType::Building)),
            None
        );
    }

    #[test]
    fn unknown_target_is_fresh_and_unknown() {
        let (o, mem, sk) = (obs(), MemoryStore::default(), SkillTable::default());
        let fi = FeatureInputs {
            obs: &o,
            mem: &mem,
            skills: &sk,
            tm: &[],
            step: None,
            has_goal: false,
            task: TaskType::Mining,
            now: 0,
        };
        let phi = fi.external(&chop());
        assert_eq!(phi[f::BIAS], 1.0);
        assert_eq!(phi[f::FRESH], 1.0);
        assert_eq!(phi[f::UNKNOWN], 1.0);
        assert_eq!(phi[f::UNCERTAINTY], 1.0);
        assert_eq!(phi[f::COMPETENCE], 0.0);
        assert_eq!(phi[f::RELEVANCE], ADJACENT_RELEVANCE, "no goal");
        assert_eq!((phi[f::RISK], phi[f::COST]), (0.1, 0.2));
        assert_eq!(phi[f::INTERNAL], 0.0);
        let rest = fi.external(&cand(Skill::Rest, None, TaskType::Building));
        assert_eq!((rest[f::FRESH], rest[f::UNKNOWN]), (0.0, 0.0), "no target");
    }

    #[test]
    fn known_target_reads_memory() {
        let o = obs();
        let mut mem = MemoryStore::default();
        let x = PriorityInputs {
            v_enc: -0.5,
            ..PriorityInputs::default()
        };
        let (log, _) = mem.encode("block:oak_log", Tier::Semantic, x, 0, Source::Perception);
        let (other, _) = mem.encode("block:stone", Tier::Semantic, x, 0, Source::Perception);
        mem.access(log, 0);
        mem.expose(log, 0, false);
        mem.entry_mut(log).danger = 0.7;
        mem.record_route(&route_key(Skill::Chop, "block:oak_log"), 0.4);
        let mut sk = SkillTable::default();
        sk.record(Skill::Chop.into(), TaskType::Mining, false, 0);
        let tm = [other];
        let mut fi = FeatureInputs {
            obs: &o,
            mem: &mem,
            skills: &sk,
            tm: &tm,
            step: None,
            has_goal: false,
            task: TaskType::Mining,
            now: 0,
        };
        let phi = fi.external(&chop());
        assert!((phi[f::FRESH] - 0.9 * 0.5).abs() < 1e-6);
        assert_eq!(phi[f::UNKNOWN], 0.0);
        assert_eq!(phi[f::RISK], 0.7, "remembered danger beats the estimate");
        assert_eq!(phi[f::VALENCE_TAG], -0.5);
        assert_eq!(phi[f::SAVING], 0.5);
        assert_eq!(phi[f::TM_LINK], 0.0);
        assert!((phi[f::COMPETENCE] - (2.0 / 3.0 - 1.0)).abs() < 1e-6);
        assert_eq!(phi[f::RECENT_FAIL], 0.2);
        let tm_self = [log];
        fi.tm = &tm_self;
        assert_eq!(fi.external(&chop())[f::TM_LINK], 1.0, "in TM");
        let mut mem2 = mem.clone();
        mem2.link(log, other, 0.1);
        fi.mem = &mem2;
        fi.tm = &tm;
        assert_eq!(fi.external(&chop())[f::TM_LINK], 1.0, "linked to TM");
    }

    #[test]
    fn crafting_counts_the_recipe_as_unknown_too() {
        let o = obs();
        let mut mem = MemoryStore::default();
        mem.encode(
            "item:stick",
            Tier::Semantic,
            PriorityInputs::default(),
            0,
            Source::Perception,
        );
        let sk = SkillTable::default();
        let fi = FeatureInputs {
            obs: &o,
            mem: &mem,
            skills: &sk,
            tm: &[],
            step: None,
            has_goal: false,
            task: TaskType::Crafting,
            now: 0,
        };
        let craft = cand(
            Skill::Craft,
            Some(Target::Item("stick".into())),
            TaskType::Crafting,
        );
        assert_eq!(fi.external(&craft)[f::UNKNOWN], 0.5);
    }

    #[test]
    fn relevance_follows_the_plan_step() {
        let (o, mem, sk) = (obs(), MemoryStore::default(), SkillTable::default());
        let step = PlanStep {
            skill: Skill::Chop,
            thing: "block:oak_log".into(),
            task: TaskType::Mining,
        };
        let mut fi = FeatureInputs {
            obs: &o,
            mem: &mem,
            skills: &sk,
            tm: &[],
            step: Some(&step),
            has_goal: true,
            task: TaskType::Mining,
            now: 0,
        };
        assert_eq!(fi.external(&chop())[f::RELEVANCE], 1.0);
        let mine = cand(Skill::Mine, Some(Target::Pos(p(1, 1))), TaskType::Mining);
        assert_eq!(fi.external(&mine)[f::RELEVANCE], ADJACENT_RELEVANCE);
        let walk = cand(Skill::Explore, Some(Target::Pos(p(5, 5))), TaskType::Travel);
        assert_eq!(fi.external(&walk)[f::RELEVANCE], 0.0);
        fi.has_goal = false;
        assert_eq!(fi.external(&walk)[f::RELEVANCE], ADJACENT_RELEVANCE);
    }

    #[test]
    fn internal_actions_fill_their_own_feature() {
        let (o, mem, sk) = (obs(), MemoryStore::default(), SkillTable::default());
        let mut fi = FeatureInputs {
            obs: &o,
            mem: &mem,
            skills: &sk,
            tm: &[],
            step: None,
            has_goal: true,
            task: TaskType::Mining,
            now: 0,
        };
        let x = InternalExtras {
            unreflected: 0.6,
            imagine_fresh: 0.7,
            stakes: 0.8,
            weak_share: 0.9,
            plan_valid: false,
        };
        use InternalAction::*;
        let get = |fi: &FeatureInputs, a, i| fi.internal(a, &x)[i];
        assert_eq!(get(&fi, Reflect, f::UNKNOWN), 0.6);
        assert_eq!(get(&fi, Imagine, f::FRESH), 0.7);
        assert_eq!(get(&fi, Rehearse, f::RELEVANCE), 0.8);
        assert_eq!(get(&fi, Consolidate, f::SAVING), 0.9);
        assert_eq!(get(&fi, Plan, f::SAVING), 1.0);
        let phi = fi.internal(Reflect, &x);
        assert_eq!(
            (phi[f::BIAS], phi[f::INTERNAL], phi[f::COST]),
            (1.0, 1.0, INTERNAL_COST)
        );
        assert_eq!((phi[f::UNCERTAINTY], phi[f::SAVING]), (1.0, 0.0));
        fi.has_goal = false;
        assert_eq!(get(&fi, Plan, f::SAVING), 0.0, "no goal: nothing to plan");
        let valid = InternalExtras {
            plan_valid: true,
            ..x
        };
        fi.has_goal = true;
        assert_eq!(fi.internal(Plan, &valid)[f::SAVING], 0.0);
    }
}
