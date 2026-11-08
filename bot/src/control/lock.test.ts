import assert from "node:assert/strict";
import { test } from "node:test";

import { ActionLock, Priority } from "./lock.js";

test("higher priority preempts and aborts; equal or lower is refused", () => {
  const lock = new ActionLock();
  assert.equal(lock.holder(), null);
  assert.ok(lock.acquire("runner", Priority.Task));
  const taskSignal = lock.signal();
  assert.ok(lock.acquire("runner", Priority.Task), "the holder may re-acquire");
  assert.equal(lock.signal(), taskSignal, "re-acquiring keeps the signal");
  assert.ok(!lock.acquire("other", Priority.Task), "same priority keeps the holder");
  assert.ok(!lock.acquire("other", Priority.Idle));
  assert.ok(!taskSignal.aborted);
  assert.ok(lock.acquire("survival", Priority.Survival));
  assert.ok(taskSignal.aborted, "the running skill sees the abort");
  assert.match(String(taskSignal.reason), /preempted by survival/);
  assert.ok(!lock.signal().aborted, "the new holder gets a fresh signal");
  assert.deepEqual(lock.holder(), { owner: "survival", priority: Priority.Survival });
  assert.ok(lock.acquire("reflex", Priority.Critical));
  assert.ok(!lock.acquire("survival", Priority.Survival));
});

test("release is idempotent, wakes listeners and refreshes the signal", () => {
  const lock = new ActionLock();
  let woken = 0;
  lock.onRelease(() => {
    woken += 1;
  });
  lock.acquire("survival", Priority.Survival);
  const before = lock.signal();
  lock.release("runner");
  assert.equal(woken, 0, "only the holder can release");
  assert.equal(lock.holder()?.owner, "survival");
  lock.release("survival");
  assert.equal(woken, 1);
  assert.equal(lock.holder(), null);
  assert.notEqual(lock.signal(), before);
  assert.ok(!before.aborted, "a plain release does not abort");
  lock.release("survival");
  assert.equal(woken, 1, "a second release is a no-op");
});

test("reset aborts and frees without waking listeners", () => {
  const lock = new ActionLock();
  let woken = 0;
  lock.onRelease(() => {
    woken += 1;
  });
  lock.acquire("runner", Priority.Task);
  const s = lock.signal();
  lock.reset();
  assert.ok(s.aborted);
  assert.match(String(s.reason), /reset/);
  assert.equal(lock.holder(), null);
  assert.ok(!lock.signal().aborted);
  assert.equal(woken, 0);
});

test("with always releases, even when the body throws", async () => {
  const lock = new ActionLock();
  const got = await lock.with("runner", Priority.Task, async (signal) => {
    assert.ok(!signal.aborted);
    assert.equal(lock.holder()?.owner, "runner");
    return 7;
  });
  assert.equal(got, 7);
  assert.equal(lock.holder(), null);
  await assert.rejects(
    lock.with("runner", Priority.Task, async () => {
      throw new Error("skill crashed");
    }),
    /skill crashed/,
  );
  assert.equal(lock.holder(), null, "no deadlock after a crash (v0.2 test 9)");
  lock.acquire("survival", Priority.Survival);
  assert.equal(await lock.with("runner", Priority.Task, async () => 1), null);
  assert.equal(lock.holder()?.owner, "survival", "a refused caller changes nothing");
});

test("survival preempting a running skill leaves survival holding the lock", async () => {
  const lock = new ActionLock();
  const run = lock.with("runner", Priority.Task, async (signal) => {
    await new Promise<void>((resolve) => {
      signal.addEventListener("abort", () => resolve());
    });
    return signal.aborted ? "interrupted" : "done";
  });
  assert.ok(lock.acquire("survival", Priority.Survival));
  assert.equal(await run, "interrupted");
  assert.equal(lock.holder()?.owner, "survival", "the runner's release is a no-op");
  lock.release("survival");
  assert.equal(lock.holder(), null);
});
