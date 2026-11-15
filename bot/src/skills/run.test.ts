import assert from "node:assert/strict";
import { test } from "node:test";

import type { Outcome, Skill, Target } from "../host.js";
import { FakeBody } from "./fake.test-util.js";
import { gained, searchNames } from "./run.js";
import { P, SKILLS, go, live, req } from "./run.test-util.js";

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
  const stuck = await go(b, req("chop", { Thing: "block:oak_log" }), new Set(), live());
  assert.equal(stuck.progress, 0);
  for (const [turn, to] of [
    [0, P(24, 16, 0)],
    [2, P(-24, 16, 0)],
    [3, P(0, 16, -24)],
  ] as const) {
    b.tick = 1200 * turn;
    await go(b, req("chop", { Thing: "block:oak_log" }), new Set(), live());
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
  // One held: place it beside the agent on the -x side (away from a home door), then craft there.
  b.calls = [];
  b.inv.set("crafting_table", 1);
  await go(b, req("craft", { Item: "wooden_pickaxe" }, table), new Set(), live());
  assert.deepEqual(b.calls, [
    ["findBlock", ["crafting_table"], 16],
    ["place", "crafting_table", P(-1, 64, 0)],
    ["craft", "wooden_pickaxe", P(-1, 64, 0)],
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
  b.answers.set("place", false);
  const hard = await go(b, req("shelter", null), new Set(), live());
  assert.deepEqual([hard.success, hard.progress, hard.blocked], [false, 0, false]);
  assert.deepEqual(
    b.calls.slice(0, 2),
    [
      ["dig", P(0, 63, 0)],
      ["place", "dirt", P(1, 64, 0)],
    ],
    "stops digging at the first failure",
  );
});

test("shelter builds walls and a roof where it cannot dig in", async () => {
  const b = new FakeBody();
  b.answers.set("dig", false);
  b.inv.set("cobblestone", 9);
  b.effects.set("place", (item) => {
    const n = (b.inv.get(item as string) ?? 0) - 1;
    if (n > 0) {
      b.inv.set(item as string, n);
    } else {
      b.inv.delete(item as string);
    }
  });
  // Already solid: the feet wall to +x (stone) and the head wall to -z. Grass is open.
  b.names.set("1,64,0", "stone").set("0,65,-1", "dirt").set("-1,64,0", "short_grass").set("0,64,1", "air");
  b.names.set("0,64,-1", "tall_grass").set("1,65,0", "cave_air");
  const ok = await go(b, req("shelter", null), new Set(), live());
  assert.deepEqual([ok.success, ok.progress, ok.blocked], [true, 1, false]);
  const placed = b.calls.filter(([m]) => m === "place").map(([, item, at]) => [item, at]);
  assert.deepEqual(placed, [
    ["cobblestone", P(-1, 64, 0)],
    ["cobblestone", P(0, 64, 1)],
    ["cobblestone", P(0, 64, -1)],
    ["cobblestone", P(1, 65, 0)],
    ["cobblestone", P(-1, 65, 0)],
    ["cobblestone", P(0, 65, 1)],
    ["cobblestone", P(1, 66, 0)],
    ["cobblestone", P(0, 66, 0)],
  ]);
  assert.equal(b.inv.get("cobblestone"), 1);
  // Runs out after two open blocks, mixing the blocks held.
  const c = new FakeBody();
  c.answers.set("dig", false);
  c.inv.set("cobblestone", 1).set("oak_planks", 1);
  c.effects.set("place", (item) => c.inv.delete(item as string));
  const short = await go(c, req("shelter", null), new Set(), live());
  assert.deepEqual([short.success, short.progress], [false, 2 / 10]);
  assert.deepEqual(
    c.calls.filter(([m]) => m === "place").map(([, item]) => item),
    ["cobblestone", "oak_planks"],
  );
  // A failed placement stops the build.
  const d = new FakeBody();
  d.answers.set("dig", false);
  d.inv.set("dirt", 9);
  d.names.set("1,64,0", "stone");
  d.answers.set("place", false);
  const stuck = await go(d, req("shelter", null), new Set(), live());
  assert.deepEqual([stuck.success, stuck.progress], [false, 1 / 10]);
  assert.equal(d.calls.filter(([m]) => m === "place").length, 1);
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

test("farm grows wheat: seeds from grass, a hoe, till and sow, then harvest", async () => {
  const b = new FakeBody();
  const farm = () => go(b, req("farm", { Pos: P(9, 64, 9) }), new Set(), live());
  const res = (o: Outcome) => [o.success, o.progress, o.blocked];
  // No ripe wheat, no seeds, no grass in range: blocked.
  assert.deepEqual(res(await farm()), [false, 0, true]);
  assert.deepEqual(b.calls.slice(0, 2), [
    ["findRipe", "wheat", 64],
    ["findBlock", ["short_grass", "tall_grass"], 64],
  ]);
  // Grass breaks but drops no seed; then it drops one.
  b.blocks.set("short_grass", P(1, 64, 0));
  assert.deepEqual(res(await farm()), [false, 0.25, false]);
  assert.deepEqual(b.calls.at(-1), ["dig", P(1, 64, 0)]);
  b.effects.set("dig", () => b.inv.set("wheat_seeds", 1));
  assert.deepEqual(res(await farm()), [false, 0.5, false]);
  b.inv.delete("wheat_seeds");
  b.answers.set("dig", false);
  assert.deepEqual(res(await farm()), [false, 0.25, false], "a seed dropped, but the dig failed");
  b.inv.set("wheat_seeds", 1);
  b.answers.delete("dig");
  b.effects.delete("dig");
  // Seeds but no hoe and too little wood: blocked; with wood, craft one at a table.
  b.blocks.set("crafting_table", P(2, 64, 0));
  b.inv.set("oak_planks", 1).set("stick", 2);
  assert.deepEqual(res(await farm()), [false, 0, true]);
  assert.ok(!b.calls.some(([m]) => m === "craft"), "too little wood: no craft tried");
  b.inv.set("oak_planks", 2);
  b.calls = [];
  assert.deepEqual(res(await farm()), [false, 0.5, false]);
  assert.deepEqual(b.calls.at(-1), ["craft", "wooden_hoe", P(2, 64, 0)]);
  b.answers.set("craft", false);
  assert.deepEqual(res(await farm()), [false, 0, false]);
  b.answers.delete("craft");
  // Hoe and seeds, no soil: blocked. Soil: till, then sow; success once wheat stands there.
  b.inv.set("wooden_hoe", 1);
  assert.deepEqual(res(await farm()), [false, 0, true]);
  b.found.set("soil", P(3, 63, 3));
  b.calls = [];
  assert.deepEqual(res(await farm()), [false, 0, false], "sown but no wheat seen");
  assert.deepEqual(b.calls.slice(-2), [
    ["use", "wooden_hoe", P(3, 63, 3)],
    ["use", "wheat_seeds", P(3, 63, 3)],
  ]);
  b.names.set("3,64,3", "wheat");
  assert.deepEqual(res(await farm()), [true, 1, false]);
  b.answers.set("use", false);
  b.calls = [];
  assert.deepEqual(res(await farm()), [false, 0, false]);
  assert.equal(b.calls.filter(([m]) => m === "use").length, 1, "no sowing after a failed till");
  // Ripe wheat in range comes first and is harvested; the target position is ignored.
  b.found.set("wheat", P(4, 64, 4));
  b.calls = [];
  assert.deepEqual(res(await farm()), [true, 1, false]);
  assert.deepEqual(b.calls, [["findRipe", "wheat", 64], ["dig", P(4, 64, 4)]]);
});
