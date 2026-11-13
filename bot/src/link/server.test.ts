import assert from "node:assert/strict";
import { test } from "node:test";

import { Priority } from "../control/lock.js";
import type { BodyState } from "../control/survival.js";
import type { BotConfig, Candidate, LinkRequest } from "../host.js";
import type { Snapshot } from "../perceive/observation.js";
import { FakeBody } from "../skills/fake.test-util.js";
import { BotServer, TASK } from "./server.js";

const config: BotConfig = {
  skills: { searchRadius: 64, oreDepth: 16, followRange: 3, followMs: 5000, restMs: 5000, searchStep: 24 },
  limits: { maxVisibleBlocks: 32, maxPerKind: 1 },
  thresholds: { criticalHealth: 0.2, starvingFood: 0.05 },
};

const snapshot: Snapshot = {
  age: 40,
  timeOfDay: 6000,
  health: 20,
  food: 20,
  position: { x: 0, y: 64, z: 0 },
  inventory: [],
  blocks: [
    { name: "stone", pos: { x: 1, y: 64, z: 0 } },
    { name: "stone", pos: { x: 2, y: 64, z: 0 } },
  ],
  entities: [],
  chat: [],
  damage: 0,
  sleeping: false,
  sheltered: false,
};

function setup() {
  const body = new FakeBody();
  const said: string[] = [];
  const server = new BotServer({
    names: () => ["stone", "oak_log"],
    snapshot: () => snapshot,
    body,
    say: (t) => said.push(t),
  });
  return { body, said, server };
}

const chop: Candidate = {
  id: 3,
  skill: "chop",
  target: { Pos: { x: 1, y: 64, z: 0 } },
  task_type: "mining",
  est_cost: 0.1,
  est_risk: 0,
  commanded: false,
};

const execute: LinkRequest = {
  op: "execute",
  action: chop,
  intensity: { effort: 1, focus: 1, external_drive: false },
  recipe: null,
};

test("nothing but hello before hello", async () => {
  const { server } = setup();
  const no = { reply: "failed", message: "say hello first" };
  assert.deepEqual(await server.handle({ op: "perceive" }), no);
  assert.deepEqual(await server.handle(execute), no);
  assert.deepEqual(await server.handle({ op: "say", text: "hi" }), no);
  assert.equal(server.config(), null);
  assert.deepEqual(await server.handle({ op: "hello", config }), { reply: "ready", names: ["stone", "oak_log"] });
  assert.deepEqual(server.config(), config);
});

test("perceive uses the runner's limits", async () => {
  const { server } = setup();
  await server.handle({ op: "hello", config });
  const r = await server.handle({ op: "perceive" });
  assert.equal(r.reply, "observed");
  const obs = r.reply === "observed" ? r.obs : null;
  assert.equal(obs?.tick, 40);
  assert.deepEqual(
    obs?.visible.map((s) => s.thing),
    ["block:stone"],
    "one per kind",
  );
});

test("execute runs the skill under the lock and reports it", async () => {
  const { body, server } = setup();
  let owner = "";
  body.effects.set("dig", () => {
    body.inv.set("oak_log", 1);
    owner = server.lock.holder()?.owner ?? "";
  });
  await server.handle({ op: "hello", config });
  const r = await server.handle(execute);
  assert.equal(r.reply, "done");
  const o = r.reply === "done" ? r.outcome : null;
  assert.equal(o?.success, true);
  assert.deepEqual(o?.new_things, ["item:oak_log"]);
  assert.equal(owner, "runner", "held by the runner while it ran");
  assert.equal(server.lock.holder(), null, "released");
  const again = await server.handle(execute);
  assert.deepEqual(again.reply === "done" ? again.outcome.new_things : null, [], "held is remembered");
});

test("a body held by survival refuses the runner", async () => {
  const { server } = setup();
  await server.handle({ op: "hello", config });
  server.lock.acquire("survival", Priority.Survival);
  assert.deepEqual(await server.handle(execute), { reply: "failed", message: "the body is busy" });
});

test("say goes to game chat", async () => {
  const { said, server } = setup();
  await server.handle({ op: "hello", config });
  assert.deepEqual(await server.handle({ op: "say", text: "On it." }), { reply: "said" });
  assert.deepEqual(said, ["On it."]);
});

test("answer turns lines into replies, and errors into failed", async () => {
  const { body, server } = setup();
  assert.equal(await server.answer(JSON.stringify({ op: "hello", config })), '{"reply":"ready","names":["stone","oak_log"]}');
  const bad = JSON.parse(await server.answer("{oops"));
  assert.equal(bad.reply, "failed");
  assert.match(bad.message, /SyntaxError/);
  body.effects.set("dig", () => {
    throw new Error("server gone");
  });
  const thrown = JSON.parse(await server.answer(JSON.stringify(execute)));
  assert.deepEqual(thrown, { reply: "failed", message: "Error: server gone" });
  assert.equal(server.lock.holder(), null, "released even when the skill throws");
});

test("watch interrupts a running skill only in an emergency", async () => {
  const { server } = setup();
  const calm: BodyState = { health: 1, food: 1, sheltered: false, hostileNear: false, hasFood: false };
  const dying: BodyState = { ...calm, health: 0.1, hostileNear: true };
  assert.equal(server.watch(dying), false, "no settings before hello");
  await server.handle({ op: "hello", config });
  server.lock.acquire(TASK, Priority.Task);
  const signal = server.lock.signal();
  assert.equal(server.watch(calm), false);
  assert.equal(signal.aborted, false);
  assert.equal(server.watch(dying), true);
  assert.equal(signal.aborted, true);
});

test("reset aborts whatever runs", () => {
  const { server } = setup();
  server.lock.acquire(TASK, Priority.Task);
  const signal = server.lock.signal();
  server.reset();
  assert.equal(signal.aborted, true);
  assert.equal(server.lock.holder(), null);
});
