import assert from "node:assert/strict";
import { once } from "node:events";
import { type AddressInfo, connect } from "node:net";
import { test } from "node:test";

import { Lines, listen } from "./socket.js";

test("lines are split across chunks, without CR or blanks", () => {
  const l = new Lines();
  assert.deepEqual(l.push('{"a":1}\n{"b"'), ['{"a":1}']);
  assert.deepEqual(l.push(":2}\r\n\n  \n"), ['{"b":2}']);
  assert.deepEqual(l.push("x"), []);
  assert.deepEqual(l.push("y\n"), ["xy"]);
  assert.deepEqual(l.push("a\rb\r\n"), ["a\rb"], "only a trailing CR goes");
});

test("replies come back in request order", async () => {
  const slow = (line: string) =>
    new Promise<string>((resolve) => setTimeout(() => resolve(line.toUpperCase()), line === "first" ? 30 : 0));
  const server = await listen(0, "127.0.0.1", slow);
  const { port } = server.address() as AddressInfo;
  const c = connect(port, "127.0.0.1");
  c.setEncoding("utf8");
  let got = "";
  c.on("data", (d: string) => {
    got += d;
  });
  c.write("first\nsecond\n");
  while (got.split("\n").length < 3) {
    await once(c, "data");
  }
  assert.equal(got, "FIRST\nSECOND\n");
  c.destroy();
  server.close();
});

test("a character split across packets arrives whole", async () => {
  const got: string[] = [];
  const server = await listen(0, "127.0.0.1", async (line) => {
    got.push(line);
    return "ok";
  });
  const { port } = server.address() as AddressInfo;
  const c = connect(port, "127.0.0.1");
  await once(c, "connect");
  const e = Buffer.from("é\n");
  c.write(e.subarray(0, 1));
  await new Promise((r) => setTimeout(r, 20));
  c.write(e.subarray(1));
  await once(c, "data");
  assert.deepEqual(got, ["é"]);
  c.destroy();
  server.close();
});

test("a client that goes away gets nothing and breaks nothing", async () => {
  let release = () => {};
  const held = new Promise<void>((r) => {
    release = r;
  });
  let answered = 0;
  const server = await listen(0, "127.0.0.1", async (line) => {
    await held;
    answered += 1;
    return line;
  });
  const { port } = server.address() as AddressInfo;
  const c = connect(port, "127.0.0.1");
  await once(c, "connect");
  c.write("bye\n");
  await new Promise((r) => setTimeout(r, 20));
  c.resetAndDestroy();
  await new Promise((r) => setTimeout(r, 20));
  release();
  await new Promise((r) => setTimeout(r, 20));
  assert.equal(answered, 1);
  server.close();
});
