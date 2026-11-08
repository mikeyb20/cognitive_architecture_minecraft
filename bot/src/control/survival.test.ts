import assert from "node:assert/strict";
import { test } from "node:test";

import { ActionLock, Priority } from "./lock.js";
import { type BodyState, emergency, interruptIfEmergency } from "./survival.js";

const t = { criticalHealth: 0.2, starvingFood: 0.05 };
const calm: BodyState = { health: 1, food: 1, sheltered: false, hostileNear: false, hasFood: true };

test("emergencies match the shared reflex", () => {
  assert.ok(!emergency(calm, t));
  const hurt = { ...calm, health: 0.2, hostileNear: true };
  assert.ok(emergency(hurt, t), "at the threshold counts");
  assert.ok(!emergency({ ...hurt, health: 0.21 }, t));
  assert.ok(!emergency({ ...hurt, sheltered: true }, t), "safe inside");
  assert.ok(!emergency({ ...hurt, hostileNear: false }, t), "nothing to flee");
  const starving = { ...calm, food: 0.05 };
  assert.ok(emergency(starving, t));
  assert.ok(!emergency({ ...starving, food: 0.06 }, t));
  assert.ok(!emergency({ ...starving, hasFood: false }, t), "nothing to eat");
});

test("an emergency aborts the running skill and frees the body", () => {
  const lock = new ActionLock();
  const starving = { ...calm, food: 0 };
  assert.ok(!interruptIfEmergency(lock, starving, t), "nothing running");
  lock.acquire("runner", Priority.Task);
  const s = lock.signal();
  assert.ok(!interruptIfEmergency(lock, calm, t));
  assert.ok(!s.aborted);
  assert.ok(interruptIfEmergency(lock, starving, t));
  assert.ok(s.aborted);
  assert.match(String(s.reason), /survival/);
  assert.equal(lock.holder(), null);
});
