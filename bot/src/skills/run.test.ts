import assert from "node:assert/strict";
import { test } from "node:test";

import type { Candidate, Outcome, Skill, SkillConfig, Target } from "../host.js";
import type { Body } from "./body.js";
import { FakeBody } from "./fake.test-util.js";
import {
  HOME_BLOCKS,
  type Request,
  gained,
  homeDoor,
  homeInside,
  homeOutside,
  homePlan,
  inHome,
  inRoom,
  run,
  searchNames,
} from "./run.js";

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

test("a home: a sunk 3x3 room with walls, a roof and a door, remembered and finished on return", async () => {
  const b = new FakeBody();
  const shelter = () => go(b, req("shelter", null), new Set(), live());
  const res = (o: Outcome) => [o.success, o.progress, o.blocked];
  const steps = homePlan(P(0, 64, 0));
  assert.equal(steps.length, 70);
  // Fewer blocks than a home needs and no home yet: the quick hole.
  b.inv.set("dirt", HOME_BLOCKS - 1);
  await shelter();
  assert.deepEqual(b.calls[0], ["dig", P(0, 63, 0)]);
  assert.equal(b.home(), null);
  // Enough blocks: start the home here. The floor is dirt; one floor block holds a bed, which stays.
  b.calls = [];
  b.inv.set("dirt", HOME_BLOCKS);
  for (const [dx, dz] of [
    [-1, -1],
    [0, 0],
    [2, 0],
  ]) {
    b.names.set(`${dx},63,${dz}`, "dirt");
  }
  b.names.set("1,63,1", "white_bed");
  b.names.set("2,64,1", "dirt");
  b.names.set("3,63,0", "grass_block");
  const first = await shelter();
  assert.deepEqual(res(first), [false, 68 / 70, false], "no door and no planks for one");
  assert.deepEqual(b.calls[0], ["setHome", P(0, 64, 0)]);
  const digs = b.calls.filter(([m]) => m === "dig").map(([, at]) => at);
  assert.deepEqual(digs, [P(-1, 63, -1), P(0, 63, 0), P(2, 63, 0), P(3, 63, 0)]);
  assert.ok(b.calls.some((c) => c[0] === "goto" && JSON.stringify(c.slice(1)) === JSON.stringify([P(0, 63, 0), 0])));
  const placed = b.calls.filter(([m]) => m === "place").map(([, , at]) => at);
  assert.equal(placed.length, 15 + 14 + 25, "walls (one already solid), roof");
  assert.deepEqual(placed.at(0), P(-2, 63, -2));
  assert.deepEqual(placed.slice(-10), [P(2, 65, 0), ...steps.slice(-11, -2).map((s) => s.at)]);
  // Back from far away with a door: walk home and finish it.
  b.calls = [];
  b.at = P(30, 70, 0);
  b.inv.set("oak_door", 1);
  b.closed = true;
  assert.deepEqual(res(await shelter()), [true, 1, false]);
  assert.deepEqual(b.calls.slice(-2), [
    ["place", "oak_door", P(2, 63, 0)],
    ["goto", P(0, 63, 0), 0],
  ]);
  // The door is in: nothing left to do but stand inside.
  b.names.set("2,63,0", "oak_door").set("2,64,0", "oak_door");
  b.calls = [];
  await shelter();
  assert.deepEqual(
    b.calls.filter(([m]) => m !== "place").map(([m]) => m),
    ["setHome", "dig", "dig", "dig", "enterHome", "enterHome"],
  );
  assert.ok(!b.calls.some(([, item]) => item === "oak_door"), "the door stays");
  b.closed = false;
  assert.deepEqual(res(await shelter()), [false, 1, false], "done but not sheltered");
});

test("a home's door is crafted when needed; a failed step stops the build; a far home is replaced", async () => {
  const b = new FakeBody();
  const shelter = () => go(b, req("shelter", null), new Set(), live());
  b.inv.set("dirt", HOME_BLOCKS).set("oak_planks", 6);
  b.blocks.set("crafting_table", P(5, 64, 0));
  await shelter();
  assert.deepEqual(b.calls.slice(-3), [
    ["craft", "oak_door", P(5, 64, 0)],
    ["place", "oak_door", P(2, 63, 0)],
    ["goto", P(0, 63, 0), 0],
  ]);
  b.answers.set("craft", false);
  b.closed = true;
  b.calls = [];
  const noDoor = await shelter();
  assert.deepEqual([noDoor.success, noDoor.progress], [false, 68 / 70]);
  b.answers.delete("craft");
  b.inv.set("oak_planks", 5);
  b.calls = [];
  await shelter();
  assert.ok(!b.calls.some(([m]) => m === "craft"), "too few planks: no craft tried");
  assert.ok(!b.calls.some(([m, item]) => m === "place" && item === "oak_door"));
  // No firm floor two below (leaves, water, air): no home, the quick hole.
  for (const [cell, name] of [
    ["1,62,1", "oak_leaves"],
    ["-1,62,0", "water"],
    ["0,62,-1", "air"],
    ["0,62,0", "lava"],
    ["-1,62,-1", "air"],
    ["1,62,1", "air"],
    ["-1,62,1", "air"],
    ["1,62,-1", "air"],
  ]) {
    const f = new FakeBody();
    f.inv.set("dirt", HOME_BLOCKS);
    f.names.set(cell as string, name as string);
    await go(f, req("shelter", null), new Set(), live());
    assert.equal(f.home(), null, `${name} at ${cell}`);
    assert.deepEqual(f.calls[0], ["dig", P(0, 63, 0)]);
  }
  const firm = new FakeBody();
  firm.inv.set("dirt", HOME_BLOCKS);
  firm.names.set("1,62,1", "stone").set("-1,62,-1", "grass_block").set("2,62,0", "air").set("0,62,-2", "air");
  await go(firm, req("shelter", null), new Set(), live());
  assert.deepEqual(firm.home(), P(0, 64, 0));
  // A flower in a wall is dug out, then built over; if it cannot be dug, the build stops there.
  const flower = new FakeBody();
  flower.inv.set("dirt", HOME_BLOCKS);
  flower.names.set("-2,63,-2", "poppy").set("-2,63,-1", "bush");
  flower.answers.set("place", false);
  await go(flower, req("shelter", null), new Set(), live());
  assert.deepEqual(flower.calls.slice(-2), [
    ["dig", P(-2, 63, -2)],
    ["place", "dirt", P(-2, 63, -2)],
  ]);
  flower.answers.delete("place");
  flower.answers.set("dig", false);
  flower.calls = [];
  const stuckFlower = await go(flower, req("shelter", null), new Set(), live());
  assert.equal(stuckFlower.progress, 13 / 70);
  assert.ok(!flower.calls.some(([m]) => m === "place"));
  // Out of blocks part way: stop at the first wall that cannot be placed.
  const c = new FakeBody();
  c.inv.set("dirt", HOME_BLOCKS);
  c.effects.set("place", () => c.inv.delete("dirt"));
  const short = await go(c, req("shelter", null), new Set(), live());
  assert.deepEqual([short.success, short.progress], [false, 14 / 70]);
  // Digging and walking fail too.
  const d = new FakeBody();
  d.inv.set("dirt", HOME_BLOCKS);
  d.names.set("-1,63,-1", "dirt");
  d.answers.set("dig", false);
  d.answers.set("place", false);
  assert.equal((await go(d, req("shelter", null), new Set(), live())).progress, 0);
  assert.equal(d.home(), null, "a new home whose floor cannot be dug is given up");
  assert.deepEqual(d.calls.slice(-3), [
    ["setHome", null],
    ["dig", P(0, 63, 0)],
    ["place", "dirt", P(1, 64, 0)],
  ]);
  d.remembered = P(0, 64, 0);
  d.calls = [];
  await go(d, req("shelter", null), new Set(), live());
  assert.deepEqual(d.home(), P(0, 64, 0), "a known home is kept");
  assert.equal(d.calls.filter(([m]) => m === "dig").length, 1);
  d.answers.delete("dig");
  d.answers.delete("place");
  d.answers.set("goto", false);
  assert.equal((await go(d, req("shelter", null), new Set(), live())).progress, 12 / 70);
  // A home farther than the search radius is left; a new one starts here.
  const e = new FakeBody();
  e.inv.set("dirt", HOME_BLOCKS);
  e.remembered = P(10, 64, -10);
  e.at = P(11 + SKILLS.searchRadius, 64, -10);
  await go(e, req("shelter", null), new Set(), live());
  assert.deepEqual(e.calls[0], ["setHome", P(11 + SKILLS.searchRadius, 64, -10)]);
  e.inv.set("dirt", 1);
  for (const at of [P(10 + SKILLS.searchRadius, 64, -10), P(10, 64, SKILLS.searchRadius - 10)]) {
    e.calls = [];
    e.at = at;
    e.remembered = P(10, 64, -10);
    await go(e, req("shelter", null), new Set(), live());
    assert.deepEqual(e.calls[0], ["setHome", P(10, 64, -10)], "within the radius: go home");
  }
  e.calls = [];
  e.at = P(10, 64, SKILLS.searchRadius - 9);
  await go(e, req("shelter", null), new Set(), live());
  assert.deepEqual(e.calls[0], ["dig", P(10, 63, SKILLS.searchRadius - 9)], "just past it on z, too few blocks: a hole");
});

test("a home's door and extent, for the glue that keeps the pathfinder out of its walls", () => {
  const o = P(10, 64, -5);
  assert.deepEqual(homeDoor(o), P(12, 63, -5));
  assert.deepEqual([homeInside(o), homeOutside(o)], [P(11, 63, -5), P(13, 63, -5)]);
  for (const p of [P(9, 63, -6), P(11, 63, -4), P(10, 63, -5)]) {
    assert.ok(inRoom(o, p), JSON.stringify(p));
  }
  for (const p of [P(12, 63, -5), P(8, 63, -5), P(10, 63, -7), P(10, 63, -3), P(10, 64, -5), P(10, 62, -5)]) {
    assert.ok(!inRoom(o, p), JSON.stringify(p));
  }
  assert.deepEqual(homePlan(o).slice(-2), [
    { kind: "door", at: homeDoor(o) },
    { kind: "stand", at: P(10, 63, -5) },
  ]);
  for (const p of [P(8, 62, -7), P(12, 65, -3), P(10, 64, -5)]) {
    assert.ok(inHome(o, p), JSON.stringify(p));
  }
  for (const p of [P(7, 64, -5), P(13, 64, -5), P(10, 64, -8), P(10, 64, -2), P(10, 61, -5), P(10, 66, -5)]) {
    assert.ok(!inHome(o, p), JSON.stringify(p));
  }
});
