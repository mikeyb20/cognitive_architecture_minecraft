//! Player commands (spec §22; v0.2 Step 18, R8 and the Step 20 acceptance
//! tests 1–8, played here against a fake world).

use host_api::command::{
    Command, CommandQueue, Parsed, QueueParams, Task, mark_step, normalize, parse, serves,
};
use host_api::planner::in_tech_tree;
use host_api::{
    AgentId, Candidate, ChatMsg, Inventory, Observation, Pos, Seen, SeenKind, Skill, Target,
    TaskType, ThingId,
};

/// Game names a Minecraft host would know (a small slice of minecraft-data).
fn mc(name: &str) -> bool {
    in_tech_tree(name)
        || [
            "iron_ore",
            "diamond_ore",
            "gold_ore",
            "torch",
            "stone_axe",
            "stone_sword",
            "cooked_beef",
        ]
        .contains(&name)
}

fn get(item: &str, count: Option<u32>) -> Parsed {
    Parsed::Command(Command::Get {
        item: item.into(),
        count,
    })
}

#[test]
fn names_players_use_become_game_names() {
    let n = |s: &str| normalize(s, mc);
    assert_eq!(n("oak logs").as_deref(), Some("oak_log"), "plural");
    assert_eq!(
        n("Oak_Log").as_deref(),
        Some("oak_log"),
        "case, underscores"
    );
    assert_eq!(n("iron ore").as_deref(), Some("iron_ore"));
    assert_eq!(n("iron").as_deref(), Some("iron_ore"), "alias");
    assert_eq!(n("wood").as_deref(), Some("oak_log"));
    assert_eq!(n("stone").as_deref(), Some("cobblestone"));
    assert_eq!(n("a stone pickaxe").as_deref(), Some("stone_pickaxe"));
    assert_eq!(n("stone picks").as_deref(), Some("stone_pickaxe"));
    assert_eq!(n("the crafting-table").as_deref(), Some("crafting_table"));
    assert_eq!(n("torches").as_deref(), Some("torch"), "-ches plural");
    assert_eq!(n("sticks").as_deref(), Some("stick"));
    assert_eq!(n("steak").as_deref(), Some("cooked_beef"));
    assert_eq!(n("more iron").as_deref(), Some("iron_ore"));
    assert_eq!(n("unobtainium"), None);
    assert_eq!(n(""), None);
    assert_eq!(
        normalize("sword", |_| false),
        None,
        "aliases are checked too"
    );
}

#[test]
fn chat_lines_read_as_commands() {
    assert_eq!(parse("mine 10 oak logs", mc), get("oak_log", Some(10)));
    assert_eq!(
        parse("Bot, make a stone pickaxe", mc),
        get("stone_pickaxe", Some(1))
    );
    assert_eq!(parse("bot mine 5 iron ore", mc), get("iron_ore", Some(5)));
    assert_eq!(parse("mine more iron", mc), get("iron_ore", None));
    assert_eq!(parse("  chop an oak log ", mc), get("oak_log", Some(1)));
    assert_eq!(parse("craft sticks", mc), get("stick", None));
    for (text, cmd) in [
        ("!status", Command::Status),
        ("!cancel", Command::Cancel),
        ("!stop", Command::Stop),
        ("!follow", Command::Follow),
        ("bot, !stay", Command::Stay),
        ("!COME", Command::Come),
    ] {
        assert_eq!(parse(text, mc), Parsed::Command(cmd), "{text}");
    }
    assert_eq!(parse("hello there", mc), Parsed::Chat);
    assert_eq!(parse("", mc), Parsed::Chat);
    let unknown = |t: &str| match parse(t, mc) {
        Parsed::Unknown(why) => why,
        other => panic!("{t}: {other:?}"),
    };
    assert_eq!(unknown("!dance"), "I don't know the command !dance.");
    assert_eq!(
        unknown("mine 3 unobtainium"),
        "I don't know what \"unobtainium\" is."
    );
    assert_eq!(unknown("mine"), "mine what?");
    assert_eq!(
        unknown("get 0 sticks"),
        "Zero of something is already done."
    );
}

fn obs(inv: &[(&str, u32)], visible: Vec<Seen>) -> Observation {
    Observation {
        agent: AgentId(0),
        tick: 0,
        day_phase: 0.1,
        health: 1.0,
        food: 1.0,
        position: Pos::default(),
        inventory: Inventory {
            items: inv.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
        },
        visible,
        chat: vec![],
        damage_taken: 0.0,
        resting: false,
        sheltered: false,
    }
}

fn seen(thing: &str, kind: SeenKind, x: i32, distance: f32) -> Seen {
    Seen {
        thing: ThingId(thing.into()),
        kind,
        pos: Pos { x, y: 0, z: 0 },
        distance,
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

fn say(q: &mut CommandQueue, text: &str, o: &Observation) -> String {
    let msg = ChatMsg {
        from: "alex".into(),
        text: text.into(),
    };
    q.hear(&msg, o, mc).unwrap_or_default()
}

#[test]
fn gathering_counts_from_what_was_held() {
    // Test 1: "mine 10 oak logs" with 2 already held finishes at 12.
    let mut q = CommandQueue::default();
    let o = obs(&[("oak_log", 2)], vec![]);
    assert_eq!(say(&mut q, "mine 10 oak logs", &o), "On it: 10 oak_log.");
    let ten_logs = Task::Get {
        item: "oak_log".into(),
        count: 10,
    };
    assert_eq!(q.current(), Some(&ten_logs));
    assert_eq!(
        q.status(&obs(&[("oak_log", 6)], vec![])),
        "Working on oak_log: 4/10. Queue: empty."
    );
    assert_eq!(q.update(&obs(&[("oak_log", 11)], vec![])), None);
    assert_eq!(
        q.update(&obs(&[("oak_log", 12)], vec![])).as_deref(),
        Some("Done: 10 oak_log. Ready for commands.")
    );
    assert_eq!(q.current(), None);
    assert_eq!(q.status(&o), "Idle. Queue: empty.");
}

#[test]
fn more_repeats_the_last_count_and_unroutable_items_are_refused() {
    // Test 4 asks for the same item again; test 5 asks for one with no route.
    let mut q = CommandQueue::default();
    let o = obs(&[("stone_pickaxe", 1)], vec![]);
    say(&mut q, "mine 5 iron ore", &o);
    say(&mut q, "!cancel", &o);
    assert_eq!(say(&mut q, "mine more iron", &o), "On it: 5 raw_iron.");
    say(&mut q, "!stop", &o);
    assert_eq!(
        say(&mut q, "get sticks", &o),
        "On it: 1 stick.",
        "default count is 1"
    );
    assert_eq!(
        say(&mut q, "mine 10 diamond ore", &o),
        "I don't know how to get diamond yet."
    );
    assert_eq!(
        say(&mut q, "!dance", &o),
        "I don't know the command !dance."
    );
    assert_eq!(say(&mut q, "nice weather", &o), "");
}

#[test]
fn second_command_queues_and_runs_after_the_first() {
    // Test 6: iron starts, oak queues with a confirmation, both finish in order.
    let mut q = CommandQueue::new(QueueParams {
        max_queued: 2,
        ..QueueParams::default()
    });
    let o = obs(&[("stone_pickaxe", 1)], vec![]);
    say(&mut q, "mine 20 iron ore", &o);
    assert_eq!(
        say(&mut q, "mine 30 oak logs", &o),
        "Queued 30 oak_log as #2, after 20 raw_iron."
    );
    assert_eq!(
        say(&mut q, "get 2 sticks", &o),
        "Queued 2 stick as #3, after 20 raw_iron."
    );
    assert_eq!(say(&mut q, "get a furnace", &o), "My queue is full (2).");
    assert_eq!(
        q.status(&o),
        "Working on raw_iron: 0/20. Queue: 30 oak_log, 2 stick."
    );
    let iron = obs(
        &[("stone_pickaxe", 1), ("raw_iron", 20), ("oak_log", 4)],
        vec![],
    );
    assert_eq!(
        q.update(&iron).as_deref(),
        Some("Done: 20 raw_iron. On it: 30 oak_log.")
    );
    assert_eq!(
        q.status(&iron),
        "Working on oak_log: 0/30. Queue: 2 stick.",
        "counts from 4"
    );
    assert_eq!(q.waiting().count(), 1);
}

#[test]
fn cancel_and_stop() {
    // Test 7: "!cancel" stops the current task and starts the next.
    let mut q = CommandQueue::default();
    let o = obs(&[], vec![]);
    assert_eq!(say(&mut q, "!cancel", &o), "Nothing to cancel.");
    say(&mut q, "mine 3 logs", &o);
    say(&mut q, "get 2 sticks", &o);
    assert_eq!(
        say(&mut q, "!cancel", &o),
        "Cancelled 3 oak_log. On it: 2 stick."
    );
    assert_eq!(
        say(&mut q, "!cancel", &o),
        "Cancelled 2 stick. Ready for commands."
    );
    say(&mut q, "mine 3 logs", &o);
    say(&mut q, "get 2 sticks", &o);
    assert_eq!(say(&mut q, "!stop", &o), "Stopped. Queue cleared.");
    assert_eq!((q.current(), q.waiting().count()), (None, 0));
}

#[test]
fn follow_stay_and_come() {
    // Test 8: follow until "!stay"; "!come" walks over once.
    let mut q = CommandQueue::default();
    // Another player standing close must not count as alex.
    let far = obs(
        &[],
        vec![
            seen("player:sam", SeenKind::Player, 1, 1.0),
            seen("player:alex", SeenKind::Player, 9, 9.0),
        ],
    );
    assert_eq!(say(&mut q, "!stay", &far), "I'm not following anyone.");
    assert_eq!(say(&mut q, "!follow", &far), "On it: following alex.");
    for _ in 0..100 {
        assert_eq!(q.update(&far), None, "following never ends or stalls");
    }
    assert_eq!(q.status(&far), "Now following alex. Queue: empty.");
    assert_eq!(say(&mut q, "!stay", &far), "Staying. Ready for commands.");
    say(&mut q, "!follow", &far);
    assert_eq!(
        say(&mut q, "get 2 sticks", &far),
        "Stopped following. On it: 2 stick.",
        "a new task replaces following"
    );
    say(&mut q, "!stop", &far);
    assert_eq!(say(&mut q, "!come", &far), "On it: coming to alex.");
    assert_eq!(q.update(&far), None);
    let near = obs(&[], vec![seen("player:alex", SeenKind::Player, 3, 3.0)]);
    assert_eq!(
        q.update(&near).as_deref(),
        Some("Done: coming to alex. Ready for commands.")
    );
}

#[test]
fn tasks_without_progress_are_given_up() {
    // Test 5: a hard limit instead of searching forever.
    let mut q = CommandQueue::new(QueueParams {
        stall_limit: 3,
        ..QueueParams::default()
    });
    let o = obs(&[], vec![]);
    say(&mut q, "mine 2 logs", &o);
    assert_eq!(q.update(&o), None, "first look sets the best");
    assert_eq!(q.update(&o), None, "stall 1");
    let one = obs(&[("oak_log", 1)], vec![]);
    assert_eq!(q.update(&one), None, "progress resets the stall");
    assert_eq!(q.update(&one), None);
    assert_eq!(q.update(&one), None);
    assert_eq!(
        q.update(&one).as_deref(),
        Some("I couldn't finish 2 oak_log; giving up. Ready for commands.")
    );
    // A player out of sight counts as no progress.
    say(&mut q, "!come", &o);
    for _ in 0..2 {
        assert_eq!(q.update(&o), None);
    }
    assert_eq!(
        q.update(&o).as_deref(),
        Some("I couldn't finish coming to alex; giving up. Ready for commands.")
    );
    assert_eq!(q.update(&o), None, "idle queue says nothing");
}

#[test]
fn marks_the_step_that_serves_the_task() {
    let log = seen("block:oak_log", SeenKind::Block, 4, 4.0);
    let alex = seen("player:alex", SeenKind::Player, 9, 9.0);
    let o = obs(&[], vec![log, alex]);
    let at = |x| Target::Pos(Pos { x, y: 0, z: 0 });
    let fresh = || {
        vec![
            cand(Skill::Chop, at(4)),
            cand(Skill::Chop, at(7)),
            cand(Skill::Mine, at(4)),
            cand(Skill::Craft, Target::Item("stick".into())),
            cand(Skill::FollowPlayer, Target::Player("alex".into())),
            cand(Skill::MoveTo, Target::Player("alex".into())),
            cand(Skill::FollowPlayer, Target::Player("sam".into())),
            cand(
                Skill::Explore,
                Target::Thing(ThingId("block:oak_log".into())),
            ),
        ]
    };
    let marks = |q: &CommandQueue, o: &Observation| {
        let mut c = fresh();
        q.mark(o, &mut c);
        c.iter().map(|c| c.commanded).collect::<Vec<_>>()
    };
    let mut q = CommandQueue::default();
    assert!(marks(&q, &o).iter().all(|m| !m), "nothing open");
    say(&mut q, "mine 2 logs", &o);
    let f = false;
    assert_eq!(
        marks(&q, &o),
        [true, f, f, f, f, f, f, f],
        "the seen log only"
    );
    say(&mut q, "!stop", &o);
    let planks = obs(&[("oak_planks", 2)], vec![]);
    say(&mut q, "get a stick", &planks);
    assert_eq!(marks(&q, &planks), [f, f, f, true, f, f, f, f]);
    say(&mut q, "!follow", &o);
    assert_eq!(
        marks(&q, &planks),
        [f, f, f, true, f, f, f, f],
        "follow waits behind sticks"
    );
    say(&mut q, "!cancel", &o);
    assert_eq!(
        marks(&q, &o),
        [f, f, f, f, true, true, f, f],
        "toward alex only"
    );
    // Direct helpers.
    let mut c = fresh();
    mark_step(&obs(&[("oak_log", 2)], vec![]), &mut c, "oak_log", 2);
    assert!(c.iter().all(|c| !c.commanded), "held already");
    assert!(!serves(
        &o,
        &cand(Skill::Chop, Target::Player("alex".into())),
        "block:oak_log"
    ));
}
