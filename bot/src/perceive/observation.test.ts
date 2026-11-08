import assert from "node:assert/strict";
import { test } from "node:test";

import { type Snapshot, blockPos, toObservation } from "./observation.js";

function snap(over: Partial<Snapshot> = {}): Snapshot {
  return {
    age: 1234,
    timeOfDay: 6000,
    health: 15,
    food: 18,
    position: { x: 0.5, y: 64, z: -0.5 },
    inventory: [],
    blocks: [],
    entities: [],
    chat: [],
    damage: 0,
    sleeping: false,
    sheltered: false,
    ...over,
  };
}

const limits = { maxVisibleBlocks: 2 };

test("scales body bars and the clock to 0..1", () => {
  const o = toObservation(snap({ damage: 3 }), limits, 4);
  assert.equal(o.agent, 4);
  assert.equal(o.tick, 1234);
  assert.equal(o.day_phase, 0.25);
  assert.equal(o.health, 0.75);
  assert.equal(o.food, 0.9);
  assert.equal(o.damage_taken, 0.15);
  assert.deepEqual(o.position, { x: 0, y: 64, z: -1 }, "floored to the block");
  assert.equal(toObservation(snap({ timeOfDay: 12000 }), limits).day_phase, 0.5, "sunset is night");
  assert.equal(toObservation(snap({ timeOfDay: 30000 }), limits).day_phase, 0.25, "wraps");
  assert.equal(toObservation(snap({ timeOfDay: -6000 }), limits).day_phase, 0.75);
  const odd = toObservation(snap({ health: 25, food: -1, damage: -2 }), limits);
  assert.deepEqual([odd.health, odd.food, odd.damage_taken], [1, 0, 0], "clamped");
});

test("merges stacks and sorts the inventory by name", () => {
  const o = toObservation(
    snap({
      inventory: [
        { name: "oak_log", count: 3 },
        { name: "dirt", count: 64 },
        { name: "oak_log", count: 2 },
      ],
    }),
    limits,
  );
  assert.deepEqual(o.inventory.items, [
    ["dirt", 64],
    ["oak_log", 5],
  ]);
});

test("everything seen, nearest first, with the sim's thing ids", () => {
  const o = toObservation(
    snap({
      position: { x: 0, y: 0, z: 0 },
      blocks: [
        { name: "stone", pos: { x: 5, y: 0, z: 0 } },
        { name: "oak_log", pos: { x: 1, y: 0, z: 0 } },
        { name: "iron_ore", pos: { x: 3, y: 0, z: 0 } },
      ],
      entities: [
        { name: "zombie", kind: "hostile", pos: { x: 2.5, y: 0, z: 0 } },
        { name: "pig", kind: "passive", pos: { x: 0, y: 0, z: 9 } },
        { name: "alex", kind: "player", pos: { x: 0, y: 4, z: 0 } },
      ],
      chat: [{ from: "alex", text: "hi" }],
      sleeping: true,
      sheltered: true,
    }),
    limits,
  );
  assert.deepEqual(
    o.visible.map((s) => [s.thing, s.distance]),
    [
      ["block:oak_log", 1],
      ["entity:zombie", 2.5],
      ["block:iron_ore", 3],
      ["player:alex", 4],
      ["entity:pig", 9],
    ],
    "stone is past the block limit; entities are never cut",
  );
  assert.deepEqual(o.visible[1]?.kind, { Entity: { hostile: true } });
  assert.deepEqual(o.visible[1]?.pos, { x: 2, y: 0, z: 0 });
  assert.deepEqual(o.visible[3]?.kind, "Player");
  assert.deepEqual(o.visible[4]?.kind, { Entity: { hostile: false } });
  assert.deepEqual(o.visible[0]?.kind, "Block");
  assert.deepEqual(o.chat, [{ from: "alex", text: "hi" }]);
  assert.ok(o.resting && o.sheltered);
  assert.deepEqual(blockPos({ x: -0.1, y: 1.9, z: 2 }), { x: -1, y: 1, z: 2 });
});
