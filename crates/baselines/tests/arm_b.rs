// Arms B and B+N against a scripted LLM (spec §23, §24).

use std::collections::VecDeque;

use baselines::arm_b::Counts;
use baselines::{ArmB, ArmBConfig, Policy};
use host_api::{
    AgentId, Candidate, ChatMsg, Inventory, Observation, Pos, Seen, SeenKind, Skill, Target,
    TaskType, ThingId,
};
use llm::{LlmError, LlmStats, Prompt, Provider, Reply, Tier};

/// Answers from a list and keeps every prompt it was sent.
#[derive(Default)]
struct Script {
    replies: VecDeque<Result<Reply, LlmError>>,
    sent: Vec<Prompt>,
}

impl Provider for Script {
    fn id(&self) -> String {
        "script/m".into()
    }
    fn complete(&mut self, p: &Prompt) -> Result<Reply, LlmError> {
        self.sent.push(p.clone());
        self.replies
            .pop_front()
            .unwrap_or(Err(LlmError::Transport("no more".into())))
    }
}

fn says(text: &str, cached: bool) -> Result<Reply, LlmError> {
    Ok(Reply {
        text: text.into(),
        input_tokens: 400,
        output_tokens: 20,
        cost_usd: 0.25,
        cached,
    })
}

fn arm(cfg: ArmBConfig, replies: Vec<Result<Reply, LlmError>>) -> ArmB<Script> {
    ArmB::new(
        cfg,
        Script {
            replies: replies.into(),
            sent: vec![],
        },
    )
}

fn p(x: i32, y: i32, z: i32) -> Pos {
    Pos { x, y, z }
}

fn obs() -> Observation {
    let seen = |t: &str, kind, pos, d| Seen {
        thing: ThingId(t.into()),
        kind,
        pos,
        distance: d,
    };
    Observation {
        agent: AgentId(0),
        tick: 100,
        day_phase: 0.25,
        health: 0.9,
        food: 0.555,
        position: p(0, 64, 0),
        inventory: Inventory {
            items: vec![("stick".into(), 2), ("oak_log".into(), 5)],
        },
        visible: vec![
            seen("block:stone", SeenKind::Block, p(9, 64, 0), 9.0),
            seen("block:oak_log", SeenKind::Block, p(3, 64, 4), 5.0),
        ],
        chat: vec![ChatMsg {
            from: "alex".into(),
            text: "hi".into(),
        }],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

fn cand(id: u32, skill: Skill, target: Option<Target>, cost: f32, risk: f32) -> Candidate {
    Candidate {
        id,
        skill,
        target,
        task_type: TaskType::Mining,
        est_cost: cost,
        est_risk: risk,
        commanded: false,
    }
}

fn cands() -> Vec<Candidate> {
    vec![
        cand(0, Skill::Chop, Some(Target::Pos(p(3, 64, 4))), 0.15, 0.0),
        cand(
            1,
            Skill::Explore,
            Some(Target::Pos(p(0, 64, -24))),
            0.25,
            0.1,
        ),
        Candidate {
            commanded: true,
            ..cand(
                2,
                Skill::Craft,
                Some(Target::Item("oak_planks".into())),
                0.05,
                0.0,
            )
        },
        cand(
            3,
            Skill::Mine,
            Some(Target::Thing(ThingId("block:iron_ore".into()))),
            0.6,
            0.3,
        ),
        cand(
            4,
            Skill::FollowPlayer,
            Some(Target::Player("alex".into())),
            0.1,
            0.0,
        ),
        cand(5, Skill::Rest, None, 0.1, 0.0),
    ]
}

#[test]
fn the_prompt_by_hand() {
    let a = arm(ArmBConfig::default(), vec![]);
    let pr = a.prompt(&obs(), &cands());
    assert_eq!(pr.max_tokens, 1000);
    assert_eq!(
        pr.system,
        "You control one player in a Minecraft world. Stay alive and work toward: iron_pickaxe. \
         Each turn you get your status and a numbered list of options. \
         Reply with the number of one option and nothing else."
    );
    assert_eq!(
        pr.user,
        "health 90%, food 56%, day, at (0, 64, 0)\n\
         inventory: oak_log x5, stick x2\n\
         see block:oak_log 5 away\n\
         see block:stone 9 away\n\
         alex says: hi\n\
         Options:\n\
         0: chop block:oak_log at (3, 64, 4) [Mining] cost 0.15 risk 0.00\n\
         1: explore to (0, 64, -24) [Mining] cost 0.25 risk 0.10\n\
         2: craft oak_planks [Mining] cost 0.05 risk 0.00 (asked by a player)\n\
         3: mine (search for block:iron_ore) [Mining] cost 0.60 risk 0.30\n\
         4: follow_player player alex [Mining] cost 0.10 risk 0.00\n\
         5: rest [Mining] cost 0.10 risk 0.00"
    );
}

#[test]
fn night_shelter_and_an_empty_inventory_read_plainly() {
    let mut o = obs();
    o.day_phase = 0.75;
    o.sheltered = true;
    o.inventory.items.clear();
    o.visible.clear();
    o.chat.clear();
    let pr = arm(ArmBConfig::default(), vec![]).prompt(&o, &cands()[5..]);
    assert_eq!(
        pr.user,
        "health 90%, food 56%, night, sheltered, at (0, 64, 0)\ninventory: empty\nOptions:\n\
         5: rest [Mining] cost 0.10 risk 0.00"
    );
}

#[test]
fn the_reply_picks_by_id_and_costs_are_logged() {
    let mut a = arm(
        ArmBConfig::default(),
        vec![
            says("I pick 3.", false),
            says("4", true),
            says("99 then 1", false),
        ],
    );
    let (o, c) = (obs(), cands());
    assert_eq!(a.choose(&o, &c), Some(3));
    assert_eq!(a.choose(&o, &c), Some(4));
    assert_eq!(a.choose(&o, &c), Some(1), "first offered id");
    assert_eq!(
        a.stats,
        LlmStats {
            calls: 3,
            cached: 1,
            fallbacks: 0,
            unparsed: 0,
            input_tokens: 800,
            output_tokens: 40,
            cost_usd: 0.5,
        }
    );
}

#[test]
fn ids_are_matched_not_positions() {
    let mut c = cands();
    c.reverse();
    let mut a = arm(ArmBConfig::default(), vec![says("0", false)]);
    assert_eq!(a.choose(&obs(), &c), Some(5));
}

#[test]
fn no_id_an_error_or_no_budget_falls_back_to_cheapest() {
    let cfg = ArmBConfig {
        tier: Tier::Strong,
        llm: llm::LlmParams {
            strong_calls_per_hour: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut a = arm(cfg, vec![says("none fit", false), Err(LlmError::Refused)]);
    let (o, c) = (obs(), cands());
    assert_eq!(a.choose(&o, &c), Some(2), "craft: cost 0.05, risk 0");
    assert_eq!(a.choose(&o, &c), Some(2));
    assert_eq!(a.choose(&o, &c), Some(2), "cap reached: not asked");
    assert_eq!(a.stats.calls, 1);
    assert_eq!((a.stats.unparsed, a.stats.fallbacks), (1, 3));
    let mut later = o.clone();
    later.tick += 72_000; // one hour at 20 ticks/s
    assert!(a.choose(&later, &c).is_some());
    assert_eq!(a.stats.fallbacks, 4, "the queue is empty, so an error");
    assert_eq!(a.choose(&o, &[]), None);
}

#[test]
fn b_plus_n_ranks_by_bonus_and_counts_visits() {
    let cfg = ArmBConfig {
        novelty: Some(1.0),
        ..Default::default()
    };
    let mut a = arm(
        cfg,
        vec![says("0", false), says("0", false), says("0", false)],
    );
    let (o, c) = (obs(), cands());
    let first = a.prompt(&o, &c);
    assert!(first.system.contains("Options are ranked"));
    let order: Vec<&str> = first
        .user
        .lines()
        .skip_while(|l| *l != "Options:")
        .skip(1)
        .map(|l| &l[..1])
        .collect();
    // Score = 1 − (cost + risk)/2: craft 0.975, rest and follow 0.95, chop 0.925 …
    assert_eq!(order, ["2", "4", "5", "0", "1", "3"]);
    assert!(first.user.contains(
        "2: craft oak_planks [Mining] cost 0.05 risk 0.00 novelty 1.00 (asked by a player)"
    ));
    for _ in 0..3 {
        assert_eq!(a.choose(&o, &c), Some(0));
    }
    assert_eq!(a.counts().count(&c[0]), 3);
    // Chopping that log again: bonus 1/√4 = 0.5.
    assert!((a.counts().bonus(&c[0], 1.0) - 0.5).abs() < 1e-6);
    let again = a.prompt(&o, &c);
    assert!(again.user.ends_with(
        "0: chop block:oak_log at (3, 64, 4) [Mining] cost 0.15 risk 0.00 novelty 0.50"
    ));
}

#[test]
fn plain_b_keeps_no_counts() {
    let mut a = arm(ArmBConfig::default(), vec![says("0", false)]);
    a.choose(&obs(), &cands());
    assert_eq!(a.counts().count(&cands()[0]), 0);
}

#[test]
fn targets_share_counts_only_when_they_match() {
    let c = cands();
    let mut n = Counts::default();
    for x in &c {
        n.visit(x);
    }
    let same_pos = cand(9, Skill::Mine, Some(Target::Pos(p(3, 64, 4))), 0.0, 0.0);
    assert_eq!(n.count(&same_pos), 1, "same place, any skill");
    let other = cand(9, Skill::Shelter, None, 0.0, 0.0);
    assert_eq!(n.count(&other), 0);
    assert_eq!(n.count(&c[5]), 1);
    let item = cand(
        9,
        Skill::Eat,
        Some(Target::Item("oak_planks".into())),
        0.0,
        0.0,
    );
    assert_eq!(n.count(&item), 1);
    let thing = cand(
        9,
        Skill::Mine,
        Some(Target::Thing(ThingId("block:iron_ore".into()))),
        0.0,
        0.0,
    );
    assert_eq!(n.count(&thing), 1);
    let who = cand(
        9,
        Skill::MoveTo,
        Some(Target::Player("alex".into())),
        0.0,
        0.0,
    );
    assert_eq!(n.count(&who), 1);
}

#[test]
fn risk_counts_against_a_candidate_in_fallback_and_ranking() {
    let c = vec![
        cand(0, Skill::Explore, None, 0.1, 0.5),
        cand(1, Skill::Rest, None, 0.3, 0.0),
    ];
    let mut a = arm(ArmBConfig::default(), vec![says("no", false)]);
    assert_eq!(a.choose(&obs(), &c), Some(1), "0.3 beats 0.6");
    let n = arm(
        ArmBConfig {
            novelty: Some(1.0),
            ..Default::default()
        },
        vec![],
    );
    let user = n.prompt(&obs(), &c).user;
    let rest = user.find("1: rest").unwrap();
    assert!(rest < user.find("0: explore").unwrap(), "rest ranks first");
}

#[test]
fn the_hourly_cap_runs_on_world_time() {
    let cfg = ArmBConfig {
        llm: llm::LlmParams {
            cheap_calls_per_hour: 1,
            seconds_per_tick: 0.5,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut a = arm(
        cfg,
        vec![says("0", false), says("0", false), says("0", false)],
    );
    let (mut o, c) = (obs(), cands());
    a.choose(&o, &c);
    o.tick += 3600; // half an hour at 0.5 s a tick
    a.choose(&o, &c);
    assert_eq!(a.stats.calls, 1, "still inside the hour");
    o.tick += 3600;
    a.choose(&o, &c);
    assert_eq!(a.stats.calls, 2);
}

/// Arm B with an input cap of `tokens`.
fn capped(tokens: u32) -> ArmB<Script> {
    let cfg = ArmBConfig {
        llm: llm::LlmParams {
            max_input_tokens: tokens,
            ..Default::default()
        },
        ..Default::default()
    };
    arm(cfg, vec![])
}

#[test]
fn an_over_long_prompt_drops_far_things_then_small_stacks() {
    let (o, c) = (obs(), cands()[5..].to_vec());
    let sys = capped(1000).system().chars().count();
    let full = capped(1000).prompt(&o, &c).user;
    let fit = |chars: usize| ((sys + chars - 1) / 4) as u32;
    // One character short: the farthest thing goes.
    let short = capped(fit(full.chars().count())).prompt(&o, &c).user;
    assert_eq!(short, full.replace("\nsee block:stone 9 away", ""));
    // Short of even the nearest: things go, then the smaller stack.
    let unseen = full
        .replace("\nsee block:stone 9 away", "")
        .replace("\nsee block:oak_log 5 away", "");
    let tight = capped(fit(unseen.chars().count())).prompt(&o, &c).user;
    assert_eq!(tight, unseen.replace("oak_log x5, stick x2", "oak_log x5"));
    // Nothing left to drop: the candidates still go out.
    let bare = capped(0).prompt(&o, &c).user;
    assert_eq!(bare, unseen.replace("oak_log x5, stick x2", "empty"));
}

mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Every candidate is always listed, and when the candidates fit,
        /// the whole prompt stays under the input cap.
        #[test]
        fn prompts_fit_and_keep_every_candidate(items in 0usize..60, seen in 0usize..60, n in 1usize..21) {
            let mut o = obs();
            o.inventory.items = (0..items).map(|i| (format!("item_number_{i}"), i as u32 + 1)).collect();
            o.visible = (0..seen)
                .map(|i| Seen {
                    thing: ThingId(format!("block:thing_{i}")),
                    kind: SeenKind::Block,
                    pos: p(i as i32, 64, 0),
                    distance: i as f32,
                })
                .collect();
            let c: Vec<Candidate> = (0..n)
                .map(|i| cand(i as u32, Skill::Explore, Some(Target::Pos(p(0, 64, i as i32))), 0.2, 0.1))
                .collect();
            let pr = arm(ArmBConfig::default(), vec![]).prompt(&o, &c);
            for x in &c {
                let head = format!("\n{}: explore", x.id);
                prop_assert!(pr.user.contains(&head));
            }
            prop_assert!(pr.estimated_tokens() <= 1000, "{} tokens", pr.estimated_tokens());
        }
    }
}

#[test]
fn b_plus_n_beta_is_pinned() {
    assert_eq!(
        baselines::arm_b::DEFAULT_BETA,
        1.0,
        "proposed until the pilot"
    );
}

#[test]
fn only_llm_arms_report_llm_use() {
    use baselines::ArmA;
    assert!(ArmA::default().llm_stats().is_none());
    let mut a = arm(ArmBConfig::default(), vec![says("0", false)]);
    a.choose(&obs(), &cands());
    assert_eq!(a.llm_stats(), Some(&a.stats));
    assert_eq!(a.llm_stats().map(|s| s.calls), Some(1));
}
