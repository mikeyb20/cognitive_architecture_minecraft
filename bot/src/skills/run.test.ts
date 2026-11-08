import assert from "node:assert/strict";
import { test } from "node:test";

import type { Candidate, Outcome, Skill, SkillConfig, Target } from "../host.js";
import type { Body } from "./body.js";
import { FakeBody } from "./fake.test-util.js";
import { type Request, gained, run, searchNames } from "./run.js";

const P = (x: number, y: number, z: number) => ({ x, y, z });
const live = () => new AbortController().signal;

/** The runner's defaults (host-api `SkillConfig::default`). */
const SKILLS: SkillConfig = {
  searchRadius: 64,
  oreDepth: 16,
  followRange: 3,
  followMs: 5000,
  restMs: 5000,
  searchStep: 24,
};

function go(b: Body, r: Request, held: Set<string>, s: AbortSignal, cfg = SKILLS): Promise<Outcome> {
  return run(b, r, held, s, cfg);
}

function req(skill: Skill, target: Target | null, recipe: Request["recipe"] = null): Request {
  const action: Candidate = {
    id: 0,
    skill,
    target,
    task_type: "mining",
    est_cost: 0.1,
    est_risk: 0,
    commanded: false,
  };
  return { action, intensity: { effort: 0.5, focus: 1, external_drive: false }, recipe };
}

test("an outcome reports gains, damage, time and first-time items", async () => {
  const b = new FakeBody();
  b.inv.set("oak_log", 2);
  b.tick = 100;
  b.effects.set("dig", () => {
    b.inv.set("oak_log", 3);
    b.inv.set("apple", 1);
    b.hp = 17;
    b.tick += 40;
  });
  const held = new Set(["oak_log"]);
  const o = await go(b, req("chop", { Pos: P(1, 64, 0) }), held, live());
  assert.deepEqual(o, {
    success: true,
    progress: 1,
    items_gained: {
      items: [
        ["apple", 1],
        ["oak_log", 1],
      ],
    },
    damage: 0.15,
    new_things: ["item:apple"],
    blocked: false,
    duration_ticks: 40,
  });
  assert.deepEqual([...held].sort(), ["apple", "oak_log"]);
  const again = await go(b, req("chop", { Pos: P(1, 64, 0) }), held, live());
  assert.deepEqual(again.new_things, [], "apple is not new twice");
  assert.equal(again.damage, 0, "healing is not negative damage");
  b.answers.set("dig", false);
  const failed = await go(b, req("mine", { Pos: P(1, 63, 0) }), held, live());
  assert.deepEqual([failed.success, failed.progress, failed.blocked], [false, 0, false]);
});

test("an aborted action is never a success", async () => {
  const b = new FakeBody();
  const c = new AbortController();
  b.effects.set("goto", () => c.abort());
  const o = await go(b, req("explore", { Pos: P(24, 64, 0) }), new Set(), c.signal);
  assert.equal(o.success, false);
  assert.equal(o.progress, 1, "it did get there, but it was interrupted");
});

test("movement skills", async () => {
  const b = new FakeBody();
  await go(b, req("explore", { Pos: P(24, 64, 0) }), new Set(), live());
  await go(b, req("flee", { Pos: P(-16, 64, 0) }), new Set(), live());
  await go(b, req("move_to", { Pos: P(3, 64, 3) }), new Set(), live());
  await go(b, req("move_to", { Player: "alex" }), new Set(), live());
  await go(b, req("follow_player", { Player: "alex" }), new Set(), live());
  assert.deepEqual(b.calls, [
    ["goto", P(24, 64, 0), 2],
    ["goto", P(-16, 64, 0), 2],
    ["goto", P(3, 64, 3), 1],
    ["follow", "alex", 3, 5000],
    ["follow", "alex", 3, 5000],
  ]);
});

test("missing targets and skills the bot does not offer are blocked", async () => {
  const b = new FakeBody();
  const cases: Array<[Skill, Target | null]> = [
    ["explore", null],
    ["flee", { Item: "x" }],
    ["move_to", null],
    ["mine", null],
    ["attack", { Player: "alex" }],
    ["eat", null],
    ["craft", null],
    ["follow_player", { Pos: P(0, 0, 0) }],
    ["place", { Pos: P(0, 0, 0) }],
    ["chat", null],
  ];
  for (const [skill, target] of cases) {
    const o = await go(b, req(skill, target), new Set(), live());
    assert.deepEqual([o.success, o.blocked], [false, true], skill);
  }
  assert.deepEqual(b.calls, [], "nothing was attempted");
});

test("attack, eat, sleep and rest", async () => {
  const b = new FakeBody();
  await go(b, req("attack", { Pos: P(2, 64, 2) }), new Set(), live());
  await go(b, req("eat", { Item: "bread" }), new Set(), live());
  await go(b, req("rest", { Pos: P(0, 64, 3) }), new Set(), live());
  const rest = await go(b, req("rest", null), new Set(), live());
  assert.ok(rest.success);
  assert.deepEqual(b.calls, [
    ["attack", P(2, 64, 2)],
    ["eat", "bread"],
    ["sleep", P(0, 64, 3)],
    ["wait", 5000],
  ]);
  const c = new AbortController();
  b.effects.set("wait", () => c.abort());
  const cut = await go(b, req("rest", null), new Set(), c.signal);
  assert.deepEqual([cut.success, cut.progress], [false, 0], "woken early");
});

test("searches: in range, after digging down for ore, else one step on", async () => {
  assert.deepEqual(searchNames("block:iron_ore"), ["iron_ore", "deepslate_iron_ore"]);
  assert.deepEqual(searchNames("block:oak_log"), ["oak_log"]);
  for (const ore of ["coal_ore", "gold_ore", "diamond_ore"]) {
    assert.deepEqual(searchNames(`block:${ore}`), [ore, `deepslate_${ore}`]);
  }
  const b = new FakeBody();
  b.blocks.set("oak_log", P(5, 64, 5));
  const log = await go(b, req("chop", { Thing: "block:oak_log" }), new Set(), live());
  assert.ok(log.success);
  assert.deepEqual(b.calls, [
    ["findBlock", ["oak_log"], 64],
    ["dig", P(5, 64, 5)],
  ]);
  // Iron in range at the surface: no digging down.
  b.calls = [];
  b.blocks.set("iron_ore", P(9, 60, 0));
  await go(b, req("mine", { Thing: "block:iron_ore" }), new Set(), live());
  assert.deepEqual(b.calls.at(-1), ["dig", P(9, 60, 0)]);
  assert.ok(!b.calls.some(([m]) => m === "gotoY"));
  b.blocks.delete("iron_ore");
  // Iron: none in range at the surface, so dig down to 16 and look again.
  b.calls = [];
  b.effects.set("gotoY", () => {
    b.at = P(0, 16, 0);
    b.blocks.set("iron_ore", P(3, 14, 0));
  });
  const iron = await go(b, req("mine", { Thing: "block:iron_ore" }), new Set(), live());
  assert.ok(iron.success);
  assert.deepEqual(b.calls, [
    ["findBlock", ["iron_ore", "deepslate_iron_ore"], 64],
    ["gotoY", 16],
    ["findBlock", ["iron_ore", "deepslate_iron_ore"], 64],
    ["dig", P(3, 14, 0)],
  ]);
  // Already deep and nothing in range: walk one step; the heading turns
  // every 1200 ticks.
  b.calls = [];
  b.blocks.clear();
  b.tick = 1200 * 5;
  const step = await go(b, req("mine", { Thing: "block:iron_ore" }), new Set(), live());
  assert.deepEqual([step.success, step.progress, step.blocked], [false, 0.25, false]);
  assert.deepEqual(b.calls.slice(-1), [["goto", P(0, 16, 24), 2]], "turn 1: +z");
  b.answers.set("goto", false);
  const stuck = await go(b, req("farm", { Thing: "block:wheat" }), new Set(), live());
  assert.equal(stuck.progress, 0);
  for (const [turn, to] of [
    [0, P(24, 16, 0)],
    [2, P(-24, 16, 0)],
    [3, P(0, 16, -24)],
  ] as const) {
    b.tick = 1200 * turn;
    await go(b, req("farm", { Thing: "block:wheat" }), new Set(), live());
    assert.deepEqual(b.calls.at(-1), ["goto", to, 2], `turn ${turn}`);
  }
  // At the surface a non-ore search does not dig down.
  b.at = P(0, 70, 0);
  b.calls = [];
  await go(b, req("chop", { Thing: "block:oak_log" }), new Set(), live());
  assert.ok(!b.calls.some(([m]) => m === "gotoY"));
});

test("crafting uses or places the station the recipe needs", async () => {
  const b = new FakeBody();
  const none = { station: null, inputs: [] as Array<[string, number]> };
  await go(b, req("craft", { Item: "oak_planks" }, none), new Set(), live());
  await go(b, req("craft", { Item: "stick" }), new Set(), live());
  assert.deepEqual(b.calls, [
    ["craft", "oak_planks", null],
    ["craft", "stick", null],
  ]);
  const table = { station: "crafting_table", inputs: [["oak_planks", 3]] as Array<[string, number]> };
  b.calls = [];
  b.blocks.set("crafting_table", P(4, 64, 4));
  await go(b, req("craft", { Item: "wooden_pickaxe" }, table), new Set(), live());
  assert.deepEqual(b.calls, [
    ["findBlock", ["crafting_table"], 16],
    ["craft", "wooden_pickaxe", P(4, 64, 4)],
  ]);
  // No table near and none held: blocked.
  b.calls = [];
  b.blocks.clear();
  const no = await go(b, req("craft", { Item: "wooden_pickaxe" }, table), new Set(), live());
  assert.ok(no.blocked);
  // One near and one held: use the near one.
  b.calls = [];
  b.inv.set("crafting_table", 1);
  b.blocks.set("crafting_table", P(4, 64, 4));
  await go(b, req("craft", { Item: "wooden_pickaxe" }, table), new Set(), live());
  assert.deepEqual(b.calls.at(-1), ["craft", "wooden_pickaxe", P(4, 64, 4)]);
  assert.ok(!b.calls.some(([m]) => m === "place"));
  b.blocks.clear();
  // One held: place it beside the agent, then craft there.
  b.calls = [];
  b.inv.set("crafting_table", 1);
  await go(b, req("craft", { Item: "wooden_pickaxe" }, table), new Set(), live());
  assert.deepEqual(b.calls, [
    ["findBlock", ["crafting_table"], 16],
    ["place", "crafting_table", P(1, 64, 0)],
    ["craft", "wooden_pickaxe", P(1, 64, 0)],
  ]);
  b.answers.set("place", false);
  const fail = await go(b, req("craft", { Item: "wooden_pickaxe" }, table), new Set(), live());
  assert.ok(fail.blocked, "placing failed");
  b.answers.delete("place");
  // Smelting goes to a furnace with the recipe's input.
  const smelt = { station: "furnace", inputs: [["raw_iron", 1]] as Array<[string, number]> };
  b.calls = [];
  b.blocks.set("furnace", P(2, 64, 0));
  const ingot = await go(b, req("craft", { Item: "iron_ingot" }, smelt), new Set(), live());
  assert.ok(ingot.success);
  assert.deepEqual(b.calls.at(-1), ["smelt", "raw_iron", P(2, 64, 0)]);
  const empty = { station: "furnace", inputs: [] as Array<[string, number]> };
  const bad = await go(b, req("craft", { Item: "iron_ingot" }, empty), new Set(), live());
  assert.ok(bad.blocked, "a furnace recipe without an input");
});

test("shelter digs in three blocks and closes the hole", async () => {
  const b = new FakeBody();
  const none = await go(b, req("shelter", null), new Set(), live());
  assert.ok(none.blocked, "no building block held");
  b.inv.set("dirt", 3);
  const ok = await go(b, req("shelter", null), new Set(), live());
  assert.ok(ok.success);
  assert.deepEqual(b.calls, [
    ["dig", P(0, 63, 0)],
    ["dig", P(0, 62, 0)],
    ["dig", P(0, 61, 0)],
    ["place", "dirt", P(0, 63, 0)],
  ]);
  for (const block of ["cobblestone", "oak_planks", "cobbled_deepslate"]) {
    const c = new FakeBody();
    c.inv.set(block, 1);
    await go(c, req("shelter", null), new Set(), live());
    assert.deepEqual(c.calls.at(-1), ["place", block, P(0, 63, 0)], block);
  }
  b.calls = [];
  b.answers.set("dig", false);
  const hard = await go(b, req("shelter", null), new Set(), live());
  assert.ok(!hard.success && !hard.blocked);
  assert.deepEqual(b.calls, [["dig", P(0, 63, 0)]], "stops at the first failure");
});

test("gained lists only increases, sorted", () => {
  const before = new Map([
    ["dirt", 5],
    ["stick", 2],
    ["torch", 1],
  ]);
  const after = new Map([
    ["torch", 1],
    ["stick", 4],
    ["dirt", 3],
    ["bread", 1],
  ]);
  assert.deepEqual(gained(before, after), [
    ["bread", 1],
    ["stick", 2],
  ]);
});
