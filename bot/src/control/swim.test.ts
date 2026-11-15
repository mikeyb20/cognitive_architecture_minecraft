import assert from "node:assert/strict";
import { test } from "node:test";

import { SwimUp } from "./swim.js";

test("in water the bot holds jump, and lets go once on leaving", () => {
  const s = new SwimUp();
  assert.equal(s.tick(false), null, "on land the skill keeps the control");
  assert.equal(s.tick(true), true);
  assert.equal(s.tick(true), true, "held while in water");
  assert.equal(s.tick(false), false, "let go once");
  assert.equal(s.tick(false), null, "then the skill has it again");
});
