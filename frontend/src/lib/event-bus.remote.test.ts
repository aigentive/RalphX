/**
 * `createEventBus()` environment selection (§6.4): local keeps the bus it has today,
 * remote gets a `NetworkEventBus`, and an unwired remote environment fails CLOSED
 * rather than projecting this Mac's own backend events as the paired host's.
 */

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  MockEventBus,
  createEventBus,
  registerRemoteEventBusFactory,
  resetRemoteEventBusFactory,
  type EventBus,
} from "./event-bus";
import { NetworkEventBus } from "./remote/network-event-bus";
import { LOCAL_ENVIRONMENT_ID } from "./remote/active-environment";

afterEach(() => {
  resetRemoteEventBusFactory();
  vi.restoreAllMocks();
});

describe("createEventBus environment selection", () => {
  it("defaults to the local bus, so existing callers are unchanged", () => {
    // EventProvider still memoizes one bus for the app's lifetime; the default must
    // stay exactly what it was before PR 2.4's keyed remount.
    expect(createEventBus()).toBeInstanceOf(MockEventBus);
    expect(createEventBus(LOCAL_ENVIRONMENT_ID)).toBeInstanceOf(MockEventBus);
  });

  it("builds a NetworkEventBus for a remote environment", () => {
    const bus = createEventBus("env-uuid-1");
    expect(bus).toBeInstanceOf(NetworkEventBus);
    expect((bus as NetworkEventBus).environmentId()).toBe("env-uuid-1");
  });

  it("uses the registered factory when the composition root supplied one", () => {
    const factory = vi.fn((_environmentId: string, localBus: EventBus) => localBus);
    registerRemoteEventBusFactory(factory);

    const bus = createEventBus("env-uuid-2");

    expect(factory).toHaveBeenCalledTimes(1);
    expect(factory.mock.calls[0]?.[0]).toBe("env-uuid-2");
    expect(bus).toBeInstanceOf(MockEventBus); // what this factory chose to return
  });

  it("hands the factory a working local bus for chrome delegation", () => {
    let captured: EventBus | null = null;
    registerRemoteEventBusFactory((_id, localBus) => {
      captured = localBus;
      return localBus;
    });
    createEventBus("env-uuid-3");
    expect(captured).toBeInstanceOf(MockEventBus);
  });
});

describe("fail-closed fallback for an unwired remote environment", () => {
  it("never returns the local bus for a remote environment", () => {
    // The dangerous outcome is silently projecting THIS Mac's task:*/agent:* events as
    // the paired host's. Delivering nothing is visible; bleeding is not.
    const bus = createEventBus("env-uuid-4");
    expect(bus).not.toBeInstanceOf(MockEventBus);
    expect(bus).toBeInstanceOf(NetworkEventBus);
  });

  it("delivers no host events while detached", () => {
    const bus = createEventBus("env-uuid-5");
    const handler = vi.fn();
    bus.subscribe("task:created", handler);
    // Nothing ever calls beginStream/handleFrame, so no host projection happens.
    expect(handler).not.toHaveBeenCalled();
  });

  it("still serves local in-app re-broadcast through emit()", () => {
    const bus = createEventBus("env-uuid-6");
    const handler = vi.fn();
    bus.subscribe("task:updated", handler);
    bus.emit("task:updated", { id: "t1" });
    expect(handler).toHaveBeenCalledWith({ id: "t1" });
  });

  it("keeps Local-only chrome working underneath a remote environment", () => {
    const bus = createEventBus("env-uuid-7");
    const handler = vi.fn();
    const unsubscribe = bus.subscribe("ralphx://check-for-updates", handler);
    // Routed to the wrapped local bus, so it is a live registration, not a black hole.
    expect(typeof unsubscribe).toBe("function");
    expect(unsubscribe.ready).toBeInstanceOf(Promise);
  });

  it("settles `ready` without any connection", async () => {
    const bus = createEventBus("env-uuid-8");
    const unsubscribe = bus.subscribe("task:created", vi.fn());
    await expect(unsubscribe.ready).resolves.toBeUndefined();
  });
});
