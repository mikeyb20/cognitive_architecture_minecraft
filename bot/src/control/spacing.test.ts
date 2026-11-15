import assert from "node:assert/strict";
import { test } from "node:test";

import { SPACING, Swing, nextMove } from "./spacing.js";

test("recharged: hit within reach, close in beyond it", () => {
  assert.equal(nextMove(3, true), "hit", "at the edge of reach");
  assert.equal(nextMove(0.5, true), "hit", "even when it is on top of the bot");
  assert.equal(nextMove(3.01, true), "close");
});

test("recharging: back off when it is too close, else hold", () => {
  assert.equal(nextMove(1.99, false), "back");
  assert.equal(nextMove(2, false), "hold", "at exactly the near limit");
  assert.equal(nextMove(5, false), "hold", "out of reach: wait, it is coming");
});

test("the limits can be set", () => {
  const wide = { reach: 4, tooClose: 3 };
  assert.equal(nextMove(4, true, wide), "hit");
  assert.equal(nextMove(2.5, false, wide), "back");
  assert.deepEqual(SPACING, { reach: 3, tooClose: 2 });
});

test("a swing is ready at the start and again after its cooldown", () => {
  const s = new Swing(12);
  assert.equal(s.cooldown, 12);
  assert.ok(s.ready());
  s.swung();
  for (let i = 0; i < 11; i++) {
    s.tick();
    assert.ok(!s.ready(), `tick ${i + 1}`);
  }
  s.tick();
  assert.ok(s.ready(), "12 ticks later");
});
