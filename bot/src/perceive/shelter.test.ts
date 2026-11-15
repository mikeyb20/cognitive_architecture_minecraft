import assert from "node:assert/strict";
import { test } from "node:test";

import { ENCLOSURE, enclosed } from "./shelter.js";

test("walled in on four sides at both heights with a roof", () => {
  assert.ok(enclosed(() => true));
  assert.ok(!enclosed(() => false));
  assert.equal(ENCLOSURE.length, 9);
  const hole = (dx: number, dy: number, dz: number) => ENCLOSURE.some(([x, y, z]) => x === dx && y === dy && z === dz);
  assert.ok(enclosed(hole), "a 1x1 hole in open ground");
  for (const [gx, gy, gz] of ENCLOSURE) {
    const gap = enclosed((dx, dy, dz) => hole(dx, dy, dz) && !(dx === gx && dy === gy && dz === gz));
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

/** A room of `w` by `w` columns from (0,0) at the corner, walled 2 high and roofed, in open ground. */
function room(w: number, gaps: string[] = []) {
  return (dx: number, dy: number, dz: number) => {
    if (gaps.includes(`${dx},${dy},${dz}`)) {
      return false;
    }
    const inside = (v: number) => v >= 0 && v < w;
    const wall = (v: number) => v >= -1 && v <= w;
    if (dy === 2) {
      return wall(dx) && wall(dz);
    }
    return (dy === 0 || dy === 1) && wall(dx) && wall(dz) && !(inside(dx) && inside(dz));
  };
}

test("a 3x3 room counts from any spot in it; larger rooms or gaps do not", () => {
  assert.ok(enclosed(room(3)), "from a corner");
  assert.ok(enclosed((dx, dy, dz) => room(3)(dx + 1, dy, dz + 1)), "from the middle");
  assert.ok(enclosed(room(1)), "a 1x1 hole");
  assert.ok(!enclosed(room(4)), "a 4x4 room reaches too far from its corner");
  assert.ok(!enclosed(room(3, ["1,2,1"])), "a hole in the roof");
  assert.ok(!enclosed(room(3, ["3,0,1", "3,1,1"])), "a doorway left open");
  assert.ok(!enclosed(room(3, ["3,1,1"])), "a wall only one block high");
  const tunnel = (dx: number, dy: number, dz: number) => dy === 2 || dz !== 0 || dx < 0 || dx > 3;
  assert.ok(!enclosed(tunnel), "a roofed tunnel four long reaches too far");
  const mid = (gaps: string[]) => (dx: number, dy: number, dz: number) => room(3, gaps)(dx + 1, dy, dz + 1);
  for (const door of [["3,0,1", "3,1,1"], ["-1,0,1", "-1,1,1"], ["1,0,3", "1,1,3"], ["1,0,-1", "1,1,-1"]]) {
    assert.ok(!enclosed(mid(door)), `a doorway at ${door[0]} seen from the middle`);
  }
  assert.ok(!enclosed(room(3, ["2,2,2"])), "a hole in the roof over the far corner");
});
