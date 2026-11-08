//! The brain loop (spec §4) against a small scripted world, through the
//! public API only. The world has a log to chop, planks to craft, a pig to
//! hunt, new ground to explore, a player who gives a command, a zombie that
//! hurts, lava at the edge and a night. It is small on purpose: every path of
//! the loop runs, and the golden trace pins what the brain did on each tick.

use cognitive_core::activation::CuriosityParams;
use cognitive_core::agent::digest::{float_sums, sums_close, trace_digest};
use cognitive_core::agent::{Brain, BrainConfig, Decision, Goal, Horizon, ThinkTools, TickLog};
use cognitive_core::direction::PlanStep;
use cognitive_core::modes::Mode;
use host_api::{
    AgentId, Candidate, ChatMsg, Inventory, Observation, Outcome, Pos, Seen, SeenKind, Skill,
    Target, TaskType, ThingId,
};

const DAY: u64 = 2400;

fn p(x: i32, z: i32) -> Pos {
    Pos { x, y: 0, z }
}

fn seen(thing: &str, kind: SeenKind, at: Pos, me: Pos) -> Seen {
    let d = ((at.x - me.x).abs().max((at.z - me.z).abs())) as f32;
    Seen {
        thing: ThingId(thing.into()),
        kind,
        pos: at,
        distance: d,
    }
}

/// The scripted world.
struct Toy {
    tick: u64,
    pos: Pos,
    inv: Inventory,
    health: f32,
    food: f32,
    damage: f32,
    explored: i32,
    /// The player has given the command.
    said: bool,
}

impl Toy {
    fn new() -> Self {
        Self {
            tick: 0,
            pos: p(0, 0),
            inv: Inventory::default(),
            health: 1.0,
            food: 0.8,
            damage: 0.0,
            explored: 0,
            said: false,
        }
    }

    fn zombie(&self) -> bool {
        (300..420).contains(&self.tick) || (1500..1560).contains(&self.tick)
    }

    fn obs(&mut self) -> Observation {
        let me = self.pos;
        let mut visible = vec![
            seen("block:oak_log", SeenKind::Block, p(me.x + 1, me.z), me),
            seen(
                "entity:pig",
                SeenKind::Entity { hostile: false },
                p(me.x, me.z + 3),
                me,
            ),
            seen("player:steve", SeenKind::Player, p(me.x - 3, me.z), me),
        ];
        if self.zombie() {
            visible.push(seen(
                "entity:zombie",
                SeenKind::Entity { hostile: true },
                p(me.x + 2, me.z + 2),
                me,
            ));
        }
        if self.tick % 700 < 50 {
            visible.push(seen("block:lava", SeenKind::Block, p(me.x, me.z - 1), me));
        }
        let mut chat = vec![];
        if self.tick >= 50 && !self.said {
            self.said = true;
            chat.push(ChatMsg {
                from: "steve".into(),
                text: "get oak_planks".into(),
            });
        }
        Observation {
            agent: AgentId(0),
            tick: self.tick,
            day_phase: (self.tick % DAY) as f32 / DAY as f32,
            health: self.health,
            food: self.food,
            position: me,
            inventory: self.inv.clone(),
            visible,
            chat,
            damage_taken: self.damage,
            resting: false,
            sheltered: self.tick % DAY > 1300,
        }
    }

    fn cands(&self, obs: &Observation) -> Vec<Candidate> {
        let me = self.pos;
        let mut v = vec![
            (
                Skill::Chop,
                Some(Target::Pos(p(me.x + 1, me.z))),
                TaskType::Mining,
                0.0,
            ),
            (
                Skill::Attack,
                Some(Target::Pos(p(me.x, me.z + 3))),
                TaskType::Farming,
                0.1,
            ),
            (
                Skill::Explore,
                Some(Target::Pos(p(8 * (self.explored + 1), 8))),
                TaskType::Travel,
                0.0,
            ),
            (
                Skill::Chat,
                Some(Target::Player("steve".into())),
                TaskType::Travel,
                0.0,
            ),
            (Skill::Rest, None, TaskType::Building, 0.0),
        ];
        if obs.inventory.count("oak_log") > 0 {
            v.push((
                Skill::Craft,
                Some(Target::Item("oak_planks".into())),
                TaskType::Crafting,
                0.0,
            ));
        }
        if obs.inventory.count("porkchop") > 0 {
            v.push((
                Skill::Eat,
                Some(Target::Item("porkchop".into())),
                TaskType::Farming,
                0.0,
            ));
        }
        // Fleeing is always on offer; only danger pushes it (threat drive).
        v.push((Skill::Flee, None, TaskType::Combat, 0.0));
        if self.zombie() {
            // Two equally risky ways to fight it (the hazard check compares
            // risks).
            for _ in 0..2 {
                v.push((
                    Skill::Attack,
                    Some(Target::Thing(ThingId("entity:zombie".into()))),
                    TaskType::Combat,
                    0.5,
                ));
            }
        }
        v.into_iter()
            .enumerate()
            .map(|(i, (skill, target, task_type, est_risk))| Candidate {
                id: i as u32,
                skill,
                target,
                task_type,
                est_cost: 0.1,
                est_risk,
                commanded: skill == Skill::Chop && (50..400).contains(&self.tick),
            })
            .collect()
    }

    fn gain(&mut self, item: &str, n: u32) -> Inventory {
        match self.inv.items.iter_mut().find(|(i, _)| i == item) {
            Some((_, k)) => *k += n,
            None => self.inv.items.push((item.into(), n)),
        }
        Inventory {
            items: vec![(item.into(), n)],
        }
    }

    fn take(&mut self, item: &str) {
        if let Some((_, k)) = self.inv.items.iter_mut().find(|(i, _)| i == item) {
            *k = k.saturating_sub(1);
        }
        self.inv.items.retain(|(_, k)| *k > 0);
    }

    /// Run a candidate. Chopping fails every third try; the zombie hurts
    /// anything but fleeing.
    fn execute(&mut self, c: &Candidate) -> Outcome {
        let mut out = Outcome {
            success: true,
            progress: 1.0,
            duration_ticks: 5,
            ..Outcome::default()
        };
        match (c.skill, &c.target) {
            (Skill::Chop, _) if self.tick % 3 == 0 => {
                out.success = false;
                out.progress = 0.4;
            }
            (Skill::Chop, _) => {
                out.items_gained = self.gain("oak_log", 1);
                out.duration_ticks = 20;
            }
            (Skill::Craft, _) => {
                self.take("oak_log");
                out.items_gained = self.gain("oak_planks", 4);
            }
            (Skill::Attack, Some(Target::Pos(_))) => out.items_gained = self.gain("porkchop", 1),
            (Skill::Attack, _) => out.blocked = true,
            (Skill::Eat, _) => {
                self.take("porkchop");
                self.food = 1.0;
            }
            (Skill::Explore, _) if self.explored % 5 == 4 => {
                // Every fifth trip runs into a wall.
                out.success = false;
                out.blocked = true;
                self.explored += 1;
            }
            (Skill::Explore, Some(Target::Pos(t))) => {
                self.explored += 1;
                self.pos = *t;
                out.new_things = vec![ThingId(format!("block:stone@{}", self.explored))];
                out.duration_ticks = 40;
            }
            _ => {}
        }
        if self.zombie() && c.skill != Skill::Flee {
            out.damage = 0.05;
            self.health = (self.health - 0.05).max(0.1);
        }
        out
    }

    fn advance(&mut self, ticks: u64) {
        // A fall at tick 1000 leaves the body low, so the reflex fires.
        if self.tick < 1000 && self.tick + ticks >= 1000 {
            self.health = 0.2;
        }
        // Lava at tick 2000 kills.
        if self.tick < 2000 && self.tick + ticks >= 2000 {
            self.health = 0.0;
            self.tick += ticks;
            return;
        }
        self.tick += ticks;
        self.food = (self.food - ticks as f32 / 3000.0).max(0.0);
        self.health = (self.health + ticks as f32 / 4000.0).min(1.0);
    }
}

/// Plans like the sim's planner: chop, then craft. Proposes planks.
struct ToyTools;

impl ThinkTools for ToyTools {
    fn plan(&mut self, goal: &str, obs: &Observation) -> Option<PlanStep> {
        if obs.inventory.count(goal) > 0 {
            return None;
        }
        Some(if obs.inventory.count("oak_log") > 0 {
            PlanStep {
                skill: Skill::Craft,
                thing: format!("item:{goal}"),
                task: TaskType::Crafting,
            }
        } else {
            PlanStep {
                skill: Skill::Chop,
                thing: "block:oak_log".into(),
                task: TaskType::Mining,
            }
        })
    }
    fn progress(&mut self, goal: &str, obs: &Observation) -> f32 {
        if obs.inventory.count(goal) > 0 {
            1.0
        } else if obs.inventory.count("oak_log") > 0 {
            0.5
        } else {
            0.0
        }
    }
    fn propose_goal(&mut self, obs: &Observation) -> Option<String> {
        (obs.inventory.count("stick") == 0).then(|| "stick".into())
    }
}

#[derive(Debug, Default, PartialEq)]
struct Counts {
    acts: u32,
    idle: u32,
    thoughts: u32,
    reflexes: u32,
}

/// Drive the brain for `ticks` world ticks, as the runner does.
fn run(mut brain: Brain, ticks: u64, rows: &mut Vec<TickLog>) -> (Brain, Counts) {
    let mut w = Toy::new();
    let mut tools = ToyTools;
    let mut n = Counts::default();
    while w.tick < ticks {
        let obs = w.obs();
        let cands = w.cands(&obs);
        w.damage = 0.0;
        if obs.health <= 0.0 {
            // Died: the brain sees one step at zero health, then respawns.
            let _ = brain.step(&obs, &cands, &mut tools);
            rows.push(brain.last_log().clone());
            w.health = 1.0;
            w.advance(1);
            continue;
        }
        if obs.health < 0.3 {
            n.reflexes += 1;
            brain.reflex_fired(obs.tick);
            w.health = 0.6;
            w.advance(1);
            continue;
        }
        match brain.step(&obs, &cands, &mut tools) {
            Decision::Act(i, it) => {
                assert!(i < cands.len());
                assert!((0.0..=1.0).contains(&it.effort) && (0.0..=1.0).contains(&it.focus));
                n.acts += 1;
                let out = w.execute(&cands[i]);
                w.damage = out.damage;
                w.advance(u64::from(out.duration_ticks));
                // Now and then the host loses an outcome; the brain then
                // treats the action as blocked.
                if n.acts % 37 != 0 {
                    brain.outcome(out);
                }
            }
            Decision::Thought(_) => {
                n.thoughts += 1;
                w.advance(1);
            }
            Decision::Idle => {
                n.idle += 1;
                w.advance(1);
            }
        }
        rows.push(brain.last_log().clone());
    }
    (brain, n)
}

/// A brain that acts most ticks: high curiosity, a near-linear g, a horizon
/// for urgency, and an older start so stages and ρ move.
fn busy() -> BrainConfig {
    BrainConfig {
        curiosity: CuriosityParams {
            base: 0.8,
            rate: 0.05,
        },
        alpha: 1.2,
        tau_scale: 0.8,
        lens_lr: [1.0, 1.5, 1.0, 0.5],
        horizon: Some(Horizon { ticks: 6000.0 }),
        rho_pre: 0.2,
        age_per_experience: 0.5,
        ..BrainConfig::default()
    }
}

/// Low curiosity under a standing command: mostly commanded choices start.
fn low_commanded(seed: u64) -> Brain {
    let mut b = Brain::new(
        BrainConfig {
            curiosity: CuriosityParams {
                base: 0.1,
                rate: 0.1,
            },
            ..BrainConfig::default()
        },
        seed,
    );
    b.set_goal(Some(Goal {
        item: "stick".into(),
        commanded: true,
    }));
    b
}

fn in_range(v: f32, lo: f32, hi: f32) -> bool {
    v.is_finite() && (lo..=hi).contains(&v)
}

#[test]
fn every_row_stays_in_range() {
    for cfg in [BrainConfig::default(), busy()] {
        let mut rows = vec![];
        run(Brain::new(cfg, 7), 3 * DAY, &mut rows);
        for r in &rows {
            assert!(
                in_range(r.c, 0.0, 1.0) && in_range(r.c_target, 0.0, 1.0),
                "{r:?}"
            );
            assert!(in_range(r.g, 0.0, 1.0) && in_range(r.d, 0.0, 1.0));
            assert!(in_range(r.v, -1.0, 1.0) && in_range(r.pi, 0.0, 1.0));
            assert!(in_range(r.fatigue, 0.0, 1.0) && in_range(r.threat, 0.0, 1.0));
            assert!(in_range(r.conflict, 0.0, 1.0 + 1e-6) && in_range(r.u, 0.0, 1.0));
            assert!(in_range(r.w.iter().sum(), 0.999, 1.001), "weights sum to 1");
            assert!(r.w.iter().all(|w| in_range(*w, 0.0, 1.0)));
            assert!(r.top5.len() <= 5 && r.top5.len() <= r.n_choices);
            assert!(r.top5.windows(2).all(|p| p[0].score >= p[1].score));
            assert!(r.success.iter().all(|s| in_range(*s, -1.0, 1.0)));
            assert!(
                r.richness.is_finite() && r.richness >= 0.0,
                "{}",
                r.richness
            );
            assert!(in_range(r.rho_eff, 0.0, 1.0), "{}", r.rho_eff);
        }
    }
}

#[test]
fn the_loop_uses_every_part() {
    let mut rows = vec![];
    let (b, n) = run(Brain::new(busy(), 3), 3 * DAY, &mut rows);
    run(low_commanded(3), DAY, &mut rows);
    assert!(n.acts > 100 && n.thoughts > 0, "{n:?}");
    let has = |f: &dyn Fn(&TickLog) -> bool| rows.iter().any(f);
    assert!(has(&|r| r.goal_commanded), "the command became the goal");
    assert!(
        has(&|r| r.goal.as_deref() == Some("stick")),
        "own goal after"
    );
    assert!(has(&|r| r.chosen == "Chat:steve"));
    assert!(has(
        &|r| r.chosen == "Explore:16,8" || r.chosen.starts_with("Explore:")
    ));
    assert!(has(&|r| r.d_goal > 0.0), "progress toward the goal");
    assert!(has(&|r| r.threat > 0.0 && r.nearest_hostile.is_some()));
    assert!(has(&|r| r.social > 0.0));
    assert!(has(&|r| r.health == 0.0), "the brain saw its death");
    assert!(has(&|r| r.success[1] == -1.0), "a blocked trip");
    assert!(has(&|r| r.creative || r.new_items > 0));
    assert!(has(&|r| r.sigma_tiers.is_some()));
    assert!(has(&|r| r.stage > 1), "development moves with experience");
    assert!(b.memory().len() > 20);
    assert!(b.lifespan().is_some());
    assert!(b.lens(Mode::Performing).theta != b.lens(Mode::Learning).theta);
}

#[test]
fn a_command_wins_over_an_own_goal() {
    let mut b = Brain::new(busy(), 1);
    b.set_goal(Some(Goal {
        item: "stick".into(),
        commanded: false,
    }));
    let mut rows = vec![];
    let (b, _) = run(b, 200, &mut rows);
    let g = rows.iter().find(|r| r.tick >= 50).map(|r| r.goal.clone());
    assert_eq!(g, Some(Some("oak_planks".into())));
    assert!(b.goal().is_some());
}

#[test]
fn same_seed_same_trace_and_the_trace_is_pinned() {
    let mut a = vec![];
    let (_, na) = run(Brain::new(busy(), 11), 2 * DAY, &mut a);
    let mut b = vec![];
    let (_, nb) = run(Brain::new(busy(), 11), 2 * DAY, &mut b);
    assert_eq!(trace_digest(&a), trace_digest(&b));
    assert_eq!(na, nb);
    let mut d = vec![];
    run(Brain::new(BrainConfig::default(), 11), DAY, &mut d);
    let mut e = vec![];
    run(low_commanded(11), DAY, &mut e);
    assert!(e.iter().any(|r| r.chosen == "Plan"), "the command plans");
    assert!(e.iter().any(|r| r.plan_step.is_some()));
    assert!(e.iter().any(|r| r.chosen == "idle"), "nothing else starts");
    // Pinned. Change only on a deliberate behaviour change, and say why in
    // the PR (CLAUDE.md "Testing").
    assert_eq!(
        (na.acts, na.idle, na.thoughts, na.reflexes),
        GOLDEN_COUNTS,
        "{na:?}"
    );
    assert_eq!(
        (trace_digest(&a), trace_digest(&d), trace_digest(&e)),
        GOLDEN_HASHES,
        "{:?}",
        (trace_digest(&a), trace_digest(&d), trace_digest(&e))
    );
    for (rows, want) in [
        (&a, GOLDEN_SUMS[0]),
        (&d, GOLDEN_SUMS[1]),
        (&e, GOLDEN_SUMS[2]),
    ] {
        let got = float_sums(rows);
        assert!(sums_close(got, want, 1e-3), "{got:?} vs {want:?}");
    }
}

/// Float sums per golden run (busy, default, low-commanded).
const GOLDEN_SUMS: [[f64; 5]; 3] = [
    [242.4022, 0.0, 158.0622, 237.5047, 294.2703],
    [210.3362, 0.0, 51.7783, 94.9521, 984.6509],
    [98.5611, 0.0, 8.8014, 38.3409, 844.8468],
];

const GOLDEN_COUNTS: (u32, u32, u32, u32) = (336, 0, 20, 1);
const GOLDEN_HASHES: (u64, u64, u64) = (
    2_672_765_271_370_490_839,
    13_805_104_005_382_746_148,
    4_154_220_247_819_864_329,
);

#[test]
fn intensity_is_used() {
    // Effort and focus come from C and the chosen mode (spec §16); a busy
    // brain uses more than the minimum at least once.
    let mut rows = vec![];
    run(Brain::new(busy(), 5), DAY, &mut rows);
    assert!(rows.iter().any(|r| r.effort > 0.0 && r.focus > 0.0));
}

#[test]
fn accessors_report_the_state() {
    let mut rows = vec![];
    let (b, _) = run(Brain::new(busy(), 3), DAY, &mut rows);
    let last = rows.last().unwrap();
    assert_eq!(
        (b.c(), b.d(), b.v(), b.pi()),
        (last.c, last.d, last.v, last.pi)
    );
    assert!(b.pi() > 0.0 && b.pi() < 1.0);
    let o = b.oracle();
    assert_eq!(o.floor, BrainConfig::default().oracle_floor);
    assert_ne!(o.theta, Brain::new(busy(), 3).oracle().theta, "Θ learned");
    assert_eq!(b.lifespan(), LIFESPAN, "hidden lifespan for seed 3");
    assert_eq!(Brain::new(BrainConfig::default(), 3).lifespan(), None);
}

/// Seed 3's hidden lifespan for a 6,000-tick horizon (spec §15). Pinned.
const LIFESPAN: Option<u64> = Some(5094);

#[test]
fn force_state_sets_c_v_and_d() {
    let mut b = Brain::new(BrainConfig::default(), 0);
    b.force_state(Some(0.7), None);
    assert!((b.c() - 0.7).abs() < 1e-6, "{}", b.c());
    assert_eq!((b.v(), b.d()), (0.0, 0.0));
    b.force_state(None, Some((-0.5, 0.6)));
    assert!((b.c() - 0.7).abs() < 1e-6);
    assert_eq!((b.v(), b.d()), (-0.5, 0.6));
}
