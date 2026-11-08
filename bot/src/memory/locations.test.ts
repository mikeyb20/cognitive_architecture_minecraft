import assert from "node:assert/strict";
import { test } from "node:test";

import { LocationMemory, MAX_PER_THING } from "./locations.js";

const at = (x: number, y = 0, z = 0) => ({ x, y, z });

test("nearest stored spot, newest wins a tie", () => {
  const m = new LocationMemory();
  assert.equal(m.nearest("block:iron_ore", at(0)), null);
  m.remember("block:iron_ore", at(10));
  m.remember("block:iron_ore", at(-4));
  m.remember("block:iron_ore", at(4));
  assert.deepEqual(m.nearest("block:iron_ore", at(0)), at(4), "tie at 4: newest");
  assert.deepEqual(m.nearest("block:iron_ore", at(9)), at(10));
  m.remember("block:iron_ore", at(-4));
  assert.deepEqual(m.nearest("block:iron_ore", at(0)), at(-4), "seen again is newest");
  assert.equal(m.count("block:iron_ore"), 3, "no duplicates");
  m.forget("block:iron_ore", at(-4));
  assert.deepEqual(m.nearest("block:iron_ore", at(0)), at(4));
  m.forget("block:coal_ore", at(1));
  assert.equal(m.count("block:coal_ore"), 0);
});

test("keeps at most maxPerThing spots, dropping the oldest", () => {
  assert.equal(MAX_PER_THING, 32);
  const m = new LocationMemory(2);
  m.remember("block:stone", at(1));
  m.remember("block:stone", at(2));
  m.remember("block:stone", at(1));
  m.remember("block:stone", at(3));
  assert.equal(m.count("block:stone"), 2);
  assert.deepEqual(m.nearest("block:stone", at(0)), at(1), "1 was refreshed, 2 was dropped");
  assert.deepEqual(m.nearest("block:stone", at(2)), at(3), "2 is gone");
});

test("things lists only non-empty entries, sorted", () => {
  const m = new LocationMemory();
  m.remember("block:stone", at(1));
  m.remember("block:coal_ore", at(1));
  m.remember("block:iron_ore", at(1));
  m.forget("block:iron_ore", at(1));
  assert.deepEqual(m.things(), ["block:coal_ore", "block:stone"]);
  m.forget("block:coal_ore", at(1));
  assert.equal(m.nearest("block:coal_ore", at(0)), null, "emptied, not just unknown");
});
