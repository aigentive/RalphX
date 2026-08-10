/**
 * The client-side budget fit for outbound remote calls.
 *
 * The regression this guards shipped: the hydration barrier sprayed every mounted
 * query concurrently, the host's per-device limits (8 slots, 10/s bucket) refused
 * most of them, the barrier failed, and the supervisor redialed forever against a
 * drained bucket. The pacer keeps any burst below the host budget.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { paceRemoteCall, resetRequestPacingForTest } from "./request-pacing";

function deferred(): { promise: Promise<void>; resolve: () => void } {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  resetRequestPacingForTest();
  vi.useRealTimers();
});

describe("paceRemoteCall", () => {
  it("never runs more than 6 calls concurrently", async () => {
    let inFlight = 0;
    let peak = 0;
    const gates = Array.from({ length: 20 }, () => deferred());

    const runs = gates.map((gate) =>
      paceRemoteCall("env-a", async () => {
        inFlight += 1;
        peak = Math.max(peak, inFlight);
        await gate.promise;
        inFlight -= 1;
      })
    );

    // Let spacing windows elapse far enough for every start that COULD happen.
    await vi.advanceTimersByTimeAsync(10_000);
    expect(peak).toBeLessThanOrEqual(6);
    expect(inFlight).toBe(6);

    for (const gate of gates) {
      gate.resolve();
    }
    await vi.advanceTimersByTimeAsync(10_000);
    await Promise.all(runs);
    expect(peak).toBeLessThanOrEqual(6);
    expect(inFlight).toBe(0);
  });

  it("spaces starts at least 110ms apart", async () => {
    const startedAt: number[] = [];
    const runs = Array.from({ length: 5 }, () =>
      paceRemoteCall("env-a", async () => {
        startedAt.push(Date.now());
      })
    );

    await vi.advanceTimersByTimeAsync(2_000);
    await Promise.all(runs);

    expect(startedAt).toHaveLength(5);
    for (let i = 1; i < startedAt.length; i += 1) {
      expect(startedAt[i]! - startedAt[i - 1]!).toBeGreaterThanOrEqual(110);
    }
  });

  it("releases the slot when a call rejects", async () => {
    const failure = paceRemoteCall("env-a", () =>
      Promise.reject(new Error("boom"))
    );
    await expect(failure).rejects.toThrow("boom");

    // All six slots must be free again: six new calls all start.
    let started = 0;
    const gates = Array.from({ length: 6 }, () => deferred());
    const runs = gates.map((gate) =>
      paceRemoteCall("env-a", async () => {
        started += 1;
        await gate.promise;
      })
    );
    await vi.advanceTimersByTimeAsync(2_000);
    expect(started).toBe(6);
    for (const gate of gates) {
      gate.resolve();
    }
    await Promise.all(runs);
  });

  it("gives each environment its own budget", async () => {
    const gateA = deferred();
    const gateB = deferred();
    let bStarted = false;

    const blockers = Array.from({ length: 6 }, () =>
      paceRemoteCall("env-a", () => gateA.promise)
    );
    const other = paceRemoteCall("env-b", async () => {
      bStarted = true;
      await gateB.promise;
    });

    await vi.advanceTimersByTimeAsync(2_000);
    // env-a is saturated; env-b must not be starved by it.
    expect(bStarted).toBe(true);

    gateA.resolve();
    gateB.resolve();
    await vi.advanceTimersByTimeAsync(2_000);
    await Promise.all([...blockers, other]);
  });
});
