import assert from "node:assert/strict";
import { test } from "node:test";

import { ENCLOSURE, enclosed } from "./shelter.js";

test("walled in on four sides at both heights with a roof", () => {
  assert.ok(enclosed(() => true));
  assert.ok(!enclosed(() => false));
  assert.equal(ENCLOSURE.length, 9);
  for (const [gx, gy, gz] of ENCLOSURE) {
    const gap = enclosed((dx, dy, dz) => !(dx === gx && dy === gy && dz === gz));
    assert.ok(!gap, `a gap at ${gx},${gy},${gz} lets mobs in`);
  }
  assert.ok(enclosed((dx, dy) => !(dx === 0 && dy === 3)), "open sky above the roof is fine");
  assert.deepEqual(
    ENCLOSURE.map((o) => [...o]),
    [
      [1, 0, 0],
      [-1, 0, 0],
      [0, 0, 1],
      [0, 0, -1],
      [1, 1, 0],
      [-1, 1, 0],
      [0, 1, 1],
      [0, 1, -1],
      [0, 2, 0],
    ],
  );
});
