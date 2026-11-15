import assert from "node:assert/strict";
import { test } from "node:test";

import { FIGHT, FightWatch, weak } from "./fight.js";
import { ActionLock, Priority } from "./lock.js";

test("weak is strictly below the gate, and a gate of 0 is off", () => {
  assert.ok(weak(0.49, 0.5));
  assert.ok(!weak(0.5, 0.5));
  assert.ok(!weak(0, 0));
});

test("an attack begun strong stops once, when health falls below the gate", () => {
  const lock = new ActionLock();
  const w = new FightWatch();
  lock.acquire("runner", Priority.Task);
  assert.ok(!w.check(lock, 0.1, 0.5), "nothing begun yet");
  w.begin(true, 1, 0.5);
  assert.ok(!w.check(lock, 0.5, 0.5));
  const s = lock.signal();
  assert.ok(w.check(lock, 0.4, 0.5));
  assert.ok(s.aborted);
  assert.equal(FIGHT, "fight-gate");
  assert.match(String(s.reason), /fight-gate/);
  assert.equal(lock.holder(), null);
  lock.acquire("runner", Priority.Task);
  assert.ok(!w.check(lock, 0.3, 0.5), "once per skill");
  for (const [attack, health] of [
    [false, 1],
    [true, 0.4],
    [true, 0],
  ] as const) {
    w.begin(attack, health, 0.5);
    assert.ok(!w.check(lock, 0.1, 0.5), `${attack} ${health} runs on`);
  }
  w.begin(true, 1, 0.5);
  lock.release("runner");
  assert.ok(!w.check(lock, 0.1, 0.5), "nothing running");
});
