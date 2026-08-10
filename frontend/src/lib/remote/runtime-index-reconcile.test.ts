/**
 * Runtime-index reconcile signal semantics. The reconcile fan-out itself is covered
 * where the global lifecycle owner lives; this pins scope and A-5 scheduling.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  onRuntimeIndexReconcile,
  requestRuntimeIndexReconcile,
} from "./runtime-index-reconcile";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  expect(
    vi.getTimerCount(),
    "the reconcile signal scheduled a timer; A-5 makes the supervisor the sole retry owner",
  ).toBe(0);
  vi.useRealTimers();
});

describe("requestRuntimeIndexReconcile", () => {
  it("delivers the announcing environment id to every listener until detached", () => {
    const seen: string[] = [];
    const detach = onRuntimeIndexReconcile(({ environmentId }) =>
      seen.push(environmentId),
    );

    requestRuntimeIndexReconcile("env-a");
    requestRuntimeIndexReconcile("env-b");
    detach();
    requestRuntimeIndexReconcile("env-c");

    expect(seen).toEqual(["env-a", "env-b"]);
  });

  it("announces synchronously, scheduling nothing", () => {
    const listener = vi.fn();
    const detach = onRuntimeIndexReconcile(listener);

    requestRuntimeIndexReconcile("env-a");

    expect(listener).toHaveBeenCalledTimes(1);
    detach();
  });
});
