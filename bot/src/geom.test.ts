import assert from "node:assert/strict";
import { test } from "node:test";

import { distance } from "./geom.js";

test("distance uses all three axes", () => {
  assert.equal(distance({ x: 1, y: 2, z: 3 }, { x: 4, y: 6, z: 3 }), 5);
  assert.equal(distance({ x: 2, y: 3, z: 5 }, { x: 2, y: 3, z: 1 }), 4);
  assert.equal(distance({ x: -1, y: -1, z: -1 }, { x: -1, y: -1, z: -1 }), 0);
});
