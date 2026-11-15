import assert from "node:assert/strict";
import { test } from "node:test";

import type { Outcome } from "../host.js";
import { FakeBody } from "./fake.test-util.js";
import { HOME_BLOCKS, homeDoor, homeInside, homeOutside, homePlan, inHome, inRoom } from "./run.js";
import { P, SKILLS, go, live, req } from "./run.test-util.js";

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
