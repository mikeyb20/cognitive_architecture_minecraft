import assert from "node:assert/strict";
import { test } from "node:test";

import { DOOR_PASSED, DOOR_REACH, shouldClose } from "./door.js";

test("the home door closes behind the bot, never in its way", () => {
  const v = { open: true, distance: 1, inDoorway: false, moving: false };
  assert.ok(shouldClose(v), "came in and stopped beside it");
  assert.ok(!shouldClose({ ...v, open: false }), "already shut");
  assert.ok(!shouldClose({ ...v, inDoorway: true }), "standing in it");
  assert.ok(!shouldClose({ ...v, moving: true }), "walking through it");
  assert.ok(shouldClose({ ...v, moving: true, distance: 2 }), "walked past it");
  assert.ok(!shouldClose({ ...v, moving: true, distance: DOOR_PASSED }), "not yet past");
  assert.ok(shouldClose({ ...v, distance: DOOR_REACH }), "at the edge of reach");
  assert.ok(!shouldClose({ ...v, distance: DOOR_REACH + 0.1 }), "out of reach");
  assert.deepEqual([DOOR_REACH, DOOR_PASSED], [4.5, 1.8]);
});
