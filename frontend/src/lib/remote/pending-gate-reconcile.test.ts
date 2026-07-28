/**
 * P-21 signal semantics (2.7-c). The reconcile fan-out itself is covered where the gate
 * owners live; this pins the SCOPE and fail-quiet rules the owners depend on.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  onPendingGateReconcile,
  requestPendingGateReconcile,
} from "./pending-gate-reconcile";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  expect(
    vi.getTimerCount(),
    "the reconcile signal scheduled a timer; A-5 makes the supervisor the sole retry owner"
  ).toBe(0);
  vi.useRealTimers();
});

describe("requestPendingGateReconcile", () => {
  it("delivers the announcing environment id to every listener", () => {
    const seen: string[] = [];
    const detach = onPendingGateReconcile(({ environmentId }) =>
      seen.push(environmentId)
    );

    requestPendingGateReconcile("env-a");
    requestPendingGateReconcile("env-b");
    detach();
    requestPendingGateReconcile("env-c");

    // Listeners scope on the id themselves; the signal never filters for them, so a
    // background environment's connect is always visible AND always distinguishable.
    expect(seen).toEqual(["env-a", "env-b"]);
  });

  it("announces synchronously, scheduling nothing", () => {
    const listener = vi.fn();
    const detach = onPendingGateReconcile(listener);

    requestPendingGateReconcile("env-a");

    expect(listener).toHaveBeenCalledTimes(1);
    detach();
  });
});
