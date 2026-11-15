import assert from "node:assert/strict";
import { test } from "node:test";

import { DANGER, DangerWatch, inDanger } from "./danger.js";
import { ActionLock, Priority } from "./lock.js";

const open = { sheltered: false, nearestHostile: null };
const near = (d: number) => ({ sheltered: false, nearestHostile: d });

test("danger is a hostile within range of the unsheltered bot", () => {
  assert.ok(!inDanger(open, 8), "nothing in view");
  assert.ok(inDanger(near(8), 8), "at the range counts");
  assert.ok(!inDanger(near(8.01), 8));
  assert.ok(inDanger(near(0), 8));
  assert.ok(!inDanger({ sheltered: true, nearestHostile: 1 }, 8), "safe inside");
  assert.ok(!inDanger(near(0), 0), "range 0 is off");
});

test("danger stops a skill begun calm, once", () => {
  const lock = new ActionLock();
  const w = new DangerWatch();
  w.begin(open, 8);
  assert.ok(!w.check(lock, near(3), 8), "nothing running");
  lock.acquire("runner", Priority.Task);
  const s = lock.signal();
  assert.ok(!w.check(lock, near(9), 8), "still out of range");
  assert.ok(!s.aborted);
  assert.ok(w.check(lock, near(3), 8));
  assert.ok(s.aborted);
  assert.equal(String(s.reason), `Error: preempted by ${DANGER}`);
  assert.equal(DANGER, "danger");
  assert.equal(lock.holder(), null, "the body is free to choose again");
  lock.acquire("runner", Priority.Task);
  assert.ok(!w.check(lock, near(2), 8), "once per skill");
});

test("a skill begun in danger, or unseen, runs on", () => {
  const lock = new ActionLock();
  lock.acquire("runner", Priority.Task);
  const w = new DangerWatch();
  assert.ok(!w.check(lock, near(2), 8), "no skill begun");
  w.begin(near(5), 8);
  assert.ok(!w.check(lock, near(2), 8), "chosen with the hostile in view");
  w.begin(null, 8);
  assert.ok(!w.check(lock, near(2), 8), "no body state yet");
  w.begin(open, 0);
  assert.ok(!w.check(lock, near(2), 0), "off");
  assert.ok(!lock.signal().aborted);
});
