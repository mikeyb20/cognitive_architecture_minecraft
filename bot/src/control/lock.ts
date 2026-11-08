// One body, one action at a time (spec §22, kept from v0.2 R7). The lock owns
// the abort signal: a higher-priority owner (survival) aborts whatever runs,
// and the caller that preempted it gets the body. Skills only read the
// signal; they never take or release the lock.

/** Priority levels from v0.2 R7. Higher preempts lower; equal keeps. */
export const Priority = {
  Idle: 0,
  Task: 1,
  Survival: 2,
  Critical: 3,
} as const;
export type Priority = (typeof Priority)[keyof typeof Priority];

export interface Holder {
  owner: string;
  priority: Priority;
}

export class ActionLock {
  #holder: Holder | null = null;
  #controller = new AbortController();
  #onRelease: Array<() => void> = [];

  /** Take the body, aborting a lower-priority holder. False if refused. */
  acquire(owner: string, priority: Priority): boolean {
    const h = this.#holder;
    if (h !== null && h.owner !== owner && h.priority >= priority) {
      return false;
    }
    if (h !== null && h.owner !== owner) {
      this.#controller.abort(new Error(`preempted by ${owner}`));
      this.#controller = new AbortController();
    }
    this.#holder = { owner, priority };
    return true;
  }

  /** Give the body back. A no-op unless `owner` holds it (v0.2 R7). */
  release(owner: string): void {
    if (this.#holder?.owner !== owner) {
      return;
    }
    this.#holder = null;
    this.#controller = new AbortController();
    for (const f of this.#onRelease) {
      f();
    }
  }

  /** Death or reconnect: abort everything and start clean. */
  reset(): void {
    this.#controller.abort(new Error("reset"));
    this.#controller = new AbortController();
    this.#holder = null;
  }

  holder(): Holder | null {
    return this.#holder;
  }

  /** The signal skills check between steps. */
  signal(): AbortSignal {
    return this.#controller.signal;
  }

  onRelease(f: () => void): void {
    this.#onRelease.push(f);
  }

  /**
   * Run `body` holding the lock, and always release it, even if `body`
   * throws (v0.2 Step 20 test 9: a lock that is never released deadlocks).
   * Returns `null` if the lock was refused.
   */
  async with<T>(
    owner: string,
    priority: Priority,
    body: (signal: AbortSignal) => Promise<T>,
  ): Promise<T | null> {
    if (!this.acquire(owner, priority)) {
      return null;
    }
    try {
      return await body(this.signal());
    } finally {
      this.release(owner);
    }
  }
}
