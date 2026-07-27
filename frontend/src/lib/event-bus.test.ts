import { describe, expect, it, vi } from "vitest";
import { emit, listen } from "@tauri-apps/api/event";

import { createEventBus, MockEventBus, TauriEventBus } from "./event-bus";
import { isTauriMode } from "./tauri-detection";

vi.mock("./tauri-detection", () => ({
  isTauriMode: vi.fn(() => false),
}));

function flushAsyncCleanup() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

describe("TauriEventBus", () => {
  it("buffers payloads until the native listener is ready and delivers later payloads directly", async () => {
    const unlisten = vi.fn();
    let nativeHandler: ((event: { payload: string }) => void) | null = null;
    let resolveListen: ((value: () => void) => void) | null = null;
    vi.mocked(listen).mockImplementationOnce((_event, handler) => {
      nativeHandler = handler as (event: { payload: string }) => void;
      return new Promise((resolve) => {
        resolveListen = resolve;
      });
    });
    const handler = vi.fn();

    const unsubscribe = new TauriEventBus().subscribe("agent:test", handler);
    let ready = false;
    void unsubscribe.ready.then(() => {
      ready = true;
    });

    nativeHandler?.({ payload: "early" });
    expect(handler).not.toHaveBeenCalled();
    expect(ready).toBe(false);

    resolveListen?.(unlisten);
    await unsubscribe.ready;

    expect(ready).toBe(true);
    expect(handler).toHaveBeenCalledWith("early");

    nativeHandler?.({ payload: "late" });
    expect(handler).toHaveBeenCalledWith("late");

    unsubscribe();
    await flushAsyncCleanup();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("drops buffered and later payloads after unsubscribe", async () => {
    const unlisten = vi.fn();
    let nativeHandler: ((event: { payload: string }) => void) | null = null;
    let resolveListen: ((value: () => void) => void) | null = null;
    vi.mocked(listen).mockImplementationOnce((_event, handler) => {
      nativeHandler = handler as (event: { payload: string }) => void;
      return new Promise((resolve) => {
        resolveListen = resolve;
      });
    });
    const handler = vi.fn();

    const unsubscribe = new TauriEventBus().subscribe("agent:test", handler);
    nativeHandler?.({ payload: "early" });
    unsubscribe();
    nativeHandler?.({ payload: "after-unsubscribe" });
    resolveListen?.(unlisten);
    await flushAsyncCleanup();

    expect(handler).not.toHaveBeenCalled();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("logs handler errors without breaking event delivery", async () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => undefined);
    let nativeHandler: ((event: { payload: string }) => void) | null = null;
    vi.mocked(listen).mockImplementationOnce((_event, handler) => {
      nativeHandler = handler as (event: { payload: string }) => void;
      return Promise.resolve(vi.fn());
    });
    const handler = vi.fn(() => {
      throw new Error("handler failed");
    });

    new TauriEventBus().subscribe("agent:test", handler);
    await flushAsyncCleanup();
    nativeHandler?.({ payload: "late" });

    expect(handler).toHaveBeenCalledWith("late");
    expect(error).toHaveBeenCalledWith(
      "[TauriEventBus] Error in handler for agent:test:",
      expect.any(Error),
    );
  });

  it("logs native listen failures and returns a safe no-op cleanup", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.mocked(listen).mockRejectedValueOnce(new Error("listen failed"));

    const unsubscribe = new TauriEventBus().subscribe("agent:test", vi.fn());
    await expect(unsubscribe.ready).resolves.toBeUndefined();
    unsubscribe();
    await flushAsyncCleanup();

    expect(warn).toHaveBeenCalledWith(
      "[TauriEventBus] Failed to listen for agent:test:",
      expect.any(Error),
    );
  });

  it("makes unsubscribe idempotent when the native listener resolves late", async () => {
    const unlisten = vi.fn();
    let resolveListen: ((value: () => void) => void) | null = null;
    vi.mocked(listen).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveListen = resolve;
      }),
    );

    const unsubscribe = new TauriEventBus().subscribe("agent:test", vi.fn());
    unsubscribe();
    unsubscribe();
    resolveListen?.(unlisten);
    await flushAsyncCleanup();

    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("swallows native unlisten failures during cleanup", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const unlisten = vi.fn(() => {
      throw new TypeError("undefined is not an object");
    });
    vi.mocked(listen).mockResolvedValueOnce(unlisten);

    const unsubscribe = new TauriEventBus().subscribe("agent:test", vi.fn());
    await flushAsyncCleanup();
    unsubscribe();
    await flushAsyncCleanup();

    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalledWith(
      "[TauriEventBus] Failed to unlisten for agent:test:",
      expect.any(TypeError),
    );
  });

  it("logs native emit failures", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    vi.mocked(emit).mockRejectedValueOnce(new Error("emit failed"));

    new TauriEventBus().emit("agent:test", { ok: false });
    await flushAsyncCleanup();

    expect(warn).toHaveBeenCalledWith(
      "[TauriEventBus] Failed to emit agent:test:",
      expect.any(Error),
    );
  });
});

describe("MockEventBus", () => {
  it("reports synchronous registration as immediately ready", async () => {
    const unsubscribe = new MockEventBus().subscribe("agent:test", vi.fn());

    await expect(unsubscribe.ready).resolves.toBeUndefined();
  });

  it("supports subscribe, emit, listener count, unsubscribe, and clear", () => {
    const bus = new MockEventBus();
    const handler = vi.fn();
    const throwingHandler = vi.fn(() => {
      throw new Error("mock handler failed");
    });
    const error = vi.spyOn(console, "error").mockImplementation(() => undefined);

    const unsubscribe = bus.subscribe("agent:test", handler);
    bus.subscribe("agent:test", throwingHandler);

    expect(bus.getListenerCount("agent:test")).toBe(2);
    bus.emit("agent:test", "payload");

    expect(handler).toHaveBeenCalledWith("payload");
    expect(error).toHaveBeenCalledWith(
      "[MockEventBus] Error in handler for agent:test:",
      expect.any(Error),
    );

    unsubscribe();
    expect(bus.getListenerCount("agent:test")).toBe(1);

    bus.clear();
    expect(bus.getListenerCount("agent:test")).toBe(0);
  });

  it("treats repeated subscriptions of one handler reference as independent", () => {
    const bus = new MockEventBus();
    const handler = vi.fn();

    const first = bus.subscribe("agent:test", handler);
    bus.subscribe("agent:test", handler);
    expect(bus.getListenerCount("agent:test")).toBe(2);

    // Unsubscribing one must not silently remove the sibling subscription.
    first();
    expect(bus.getListenerCount("agent:test")).toBe(1);

    bus.emit("agent:test", "payload");
    expect(handler).toHaveBeenCalledTimes(1);
  });
});

describe("createEventBus", () => {
  it("creates the Tauri bus in Tauri mode and the mock bus otherwise", () => {
    vi.mocked(isTauriMode).mockReturnValueOnce(true);
    expect(createEventBus()).toBeInstanceOf(TauriEventBus);

    vi.mocked(isTauriMode).mockReturnValueOnce(false);
    expect(createEventBus()).toBeInstanceOf(MockEventBus);
  });
});
