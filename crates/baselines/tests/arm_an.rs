//! Arm A+N: the tree plus B+N's count bonus (spec §24; 07 M2).

use baselines::{ArmA, ArmAN, CountBy, Policy};
use host_api::{
    AgentId, Candidate, Inventory, Observation, Pos, Seen, SeenKind, Skill, Target, TaskType,
    ThingId,
};

fn p(x: i32, z: i32) -> Pos {
    Pos { x, y: 0, z }
}

/// Daytime, healthy, fed, a tree at (1, 1): the tree's work pick is Chop.
fn obs() -> Observation {
    Observation {
        agent: AgentId(0),
        tick: 0,
        day_phase: 0.1,
        health: 1.0,
        food: 1.0,
        position: p(0, 0),
        inventory: Inventory::default(),
        visible: vec![Seen {
            thing: ThingId("block:oak_log".into()),
            kind: SeenKind::Block,
            pos: p(1, 1),
            distance: 1.0,
        }],
        chat: vec![],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

fn c(skill: Skill, task: TaskType, x: i32, z: i32, cost: f32) -> Candidate {
    Candidate {
        id: 0,
        skill,
        target: Some(Target::Pos(p(x, z))),
        task_type: task,
        est_cost: cost,
        est_risk: 0.0,
        commanded: false,
    }
}

/// Chop the tree (the goal), or explore one of two new places.
fn three() -> Vec<Candidate> {
    vec![
        c(Skill::Chop, TaskType::Mining, 1, 1, 0.15),
        c(Skill::Explore, TaskType::Travel, 8, 0, 0.25),
        c(Skill::Explore, TaskType::Travel, 0, 8, 0.25),
    ]
}

fn picks(beta: f32, n: usize) -> Vec<usize> {
    let mut arm = ArmAN::new(beta);
    (0..n)
        .map(|_| arm.choose(&obs(), &three()).unwrap())
        .collect()
}

#[test]
fn beta_two_leaves_the_goal_once_its_bonus_is_spent() {
    // Chop scores 1 + 2/√(n+1) − 0.075; a new place 2 − 0.125 = 1.875.
    // Chop wins while n ≤ 3 (at n = 3: 1.925); at n = 4 (1.819) the first
    // new place wins, then the second (its 1.875 beats the first's 1.289),
    // then Chop again (1.819 > 1.289).
    assert_eq!(picks(2.0, 7), [0, 0, 0, 0, 1, 2, 0]);
}

#[test]
fn beta_zero_is_the_tree_whenever_the_tree_has_work() {
    assert_eq!(picks(0.0, 20), vec![0; 20]);
    let mut tree = ArmA::default();
    let mut an = ArmAN::new(0.0);
    for _ in 0..10 {
        assert_eq!(an.choose(&obs(), &three()), tree.choose(&obs(), &three()));
    }
}

#[test]
fn a_larger_beta_leaves_the_goal_sooner() {
    let first_other = |beta| picks(beta, 50).iter().position(|&i| i != 0);
    let at: Vec<Option<usize>> = [0.5, 1.0, 2.0, 4.0].map(first_other).to_vec();
    assert_eq!(at[0], None, "β = 0.5 never beats the goal's +1 here");
    assert_eq!(
        at[1], None,
        "nor β = 1: the goal's bonus never goes below 0"
    );
    assert_eq!(at[2], Some(4));
    assert_eq!(at[3], Some(1), "β = 4: 1 + 4/√2 − 0.075 < 4 − 0.125");
}

#[test]
fn danger_comes_first_and_is_counted() {
    let mut o = obs();
    o.visible.push(Seen {
        thing: ThingId("entity:zombie".into()),
        kind: SeenKind::Entity { hostile: true },
        pos: p(2, 0),
        distance: 2.0,
    });
    let mut cands = three();
    cands.push(c(Skill::Attack, TaskType::Combat, 2, 0, 0.3));
    let mut arm = ArmAN::new(10.0);
    assert_eq!(arm.choose(&o, &cands), Some(3), "fight, as arm A would");
    assert_eq!(arm.counts().count(&cands[3]), 1);
    assert_eq!(arm.counts().count(&cands[0]), 0);
    assert_eq!(arm.beta, 10.0);
    assert_eq!(arm.choose(&o, &[]), None);
}

#[test]
fn by_region_counts_decisions_where_the_agent_stands() {
    // All three decisions so far were made in region (0, 0), where the
    // tree is: Chop's bonus there falls as 2/√(n+1) as before, so it wins
    // while n ≤ 3. The new places' regions were never stood in, so both
    // keep 1.875 and the first stays ahead on ties until the agent moves.
    let mut arm = ArmAN::by_region(2.0);
    assert_eq!(arm.count_by, CountBy::Region);
    let got: Vec<usize> = (0..7)
        .map(|_| arm.choose(&obs(), &three()).unwrap())
        .collect();
    assert_eq!(got, [0, 0, 0, 0, 1, 1, 1]);
    assert_eq!(arm.counts().count_key("region:0,0"), 7);
    // Standing in region (1, 0) instead spends that region's bonus, not
    // the tree's: Chop leads to region (0, 0), never stood in, so it keeps
    // 1 + 2 − 0.075 and always wins.
    let mut o = obs();
    o.position = p(8, 0);
    let mut moved = ArmAN::by_region(2.0);
    let picks: Vec<usize> = (0..7)
        .map(|_| moved.choose(&o, &three()).unwrap())
        .collect();
    assert_eq!(picks, vec![0; 7]);
    assert_eq!(moved.counts().count_key("region:1,0"), 7);
    assert_eq!(moved.counts().count_key("region:0,0"), 0);
}
