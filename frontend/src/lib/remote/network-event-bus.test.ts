/**
 * NetworkEventBus: the canonical resume rule (§3.4), the `H` barrier, the `cursorAck`
 * lease cadence, and the two properties that keep a remote environment honest —
 * `emit()` never reaches the socket (A-13) and the cursor never advances over an
 * uncommitted projection (P-24a).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MockEventBus, type EventBus } from "../event-bus";
import {
  NetworkEventBus,
  isLocalOnlyBackendEvent,
  type NetworkEventBusDeps,
  type StreamRestartCause,
} from "./network-event-bus";
import type { RemoteClientFrame, RemoteConnectOutcome } from "./stream-frames";

const ENV = "env-uuid-1";
const HOST_ENV = "host-env-abc";

function hello(overrides: Partial<RemoteConnectOutcome> = {}): RemoteConnectOutcome {
  return {
    environmentId: ENV,
    hostEnvironmentId: HOST_ENV,
    streamEpoch: "epoch-1",
    maxSeq: 100,
    heartbeatSecs: 20,
    protocolVersion: 1,
    ...overrides,
  };
}

interface Harness {
  readonly bus: NetworkEventBus;
  readonly localBus: MockEventBus;
  readonly sent: RemoteClientFrame[];
  readonly restarts: StreamRestartCause[];
  readonly sweep: ReturnType<typeof vi.fn>;
  readonly hydrate: ReturnType<typeof vi.fn>;
  /** Resolves the pending hydration, if `deferHydration` was used. */
  releaseHydration: () => void;
}

function harness(options: { deferHydration?: boolean } = {}): Harness {
  const localBus = new MockEventBus();
  const sent: RemoteClientFrame[] = [];
  const restarts: StreamRestartCause[] = [];
  const sweep = vi.fn();

  let release: () => void = () => {};
  const hydrate = vi.fn(async () => {
    if (options.deferHydration === true) {
      await new Promise<void>((resolve) => {
        release = resolve;
      });
    }
  });

  const deps: NetworkEventBusDeps = {
    environmentId: ENV,
    localBus,
    sendFrame: async (frame) => {
      sent.push(frame);
    },
    hydrate,
    sweep,
    onRestartRequired: (cause) => {
      restarts.push(cause);
    },
  };

  return {
    bus: new NetworkEventBus(deps),
    localBus,
    sent,
    restarts,
    sweep,
    hydrate,
    releaseHydration: () => release(),
  };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

// ===========================================================================
// A-13 — emit() is local-only
// ===========================================================================

describe("emit() never writes to the WS (A-13)", () => {
  it("dispatches to local subscribers and sends no frame", async () => {
    const { bus, sent } = harness();
    await bus.beginStream(hello());
    sent.length = 0;

    const seen: unknown[] = [];
    bus.subscribe("task:updated", (payload) => seen.push(payload));
    bus.emit("task:updated", { id: "t1" });

    expect(seen).toEqual([{ id: "t1" }]);
    expect(sent).toEqual([]);
  });

  it("sends nothing even for a durable backend name", async () => {
    const { bus, sent } = harness();
    await bus.beginStream(hello());
    sent.length = 0;

    bus.emit("task:created", { id: "t2" });
    bus.emit("notification:created", {});
    bus.emit("agent:run_completed", {});

    expect(sent).toEqual([]);
  });

  it("has no sendFrame reference on the emit path — a stray emit cannot reach the host", () => {
    // Shape half of the proof: emit() reads only the listener registry.
    expect(NetworkEventBus.prototype.emit.toString()).not.toContain("sendFrame");
  });
});

// ===========================================================================
// EventBus contract
// ===========================================================================

describe("EventBus contract", () => {
  it("resolves `ready` as a registration barrier, without waiting for a connection", async () => {
    const { bus, sent } = harness();
    // Never connected: no hello, no socket, nothing sent.
    const unsubscribe = bus.subscribe("task:updated", () => {});
    await expect(unsubscribe.ready).resolves.toBeUndefined();
    expect(sent).toEqual([]);
  });

  it("keeps two subscriptions of the same handler independent", () => {
    const { bus } = harness();
    const handler = vi.fn();
    const first = bus.subscribe("task:updated", handler);
    bus.subscribe("task:updated", handler);

    first();
    bus.emit("task:updated", {});
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it("never throws from unsubscribe, including twice", () => {
    const { bus } = harness();
    const unsubscribe = bus.subscribe("task:updated", () => {});
    expect(() => {
      unsubscribe();
      unsubscribe();
    }).not.toThrow();
  });

  it("swallows handler exceptions on emit, matching the EventBus contract", () => {
    const { bus } = harness();
    vi.spyOn(console, "error").mockImplementation(() => {});
    bus.subscribe("task:updated", () => {
      throw new Error("boom");
    });
    const after = vi.fn();
    bus.subscribe("task:updated", after);

    expect(() => bus.emit("task:updated", {})).not.toThrow();
    expect(after).toHaveBeenCalledTimes(1);
  });
});

describe("Local-only backend names route to the wrapped local bus", () => {
  it("mirrors the protocol crate's Local-only backend set", () => {
    expect(isLocalOnlyBackendEvent("ralphx://check-for-updates")).toBe(true);
    expect(isLocalOnlyBackendEvent("gh-auth:login_prompt")).toBe(true);
    expect(isLocalOnlyBackendEvent("remote:session_connected")).toBe(true);
    // Webview-origin synthetics stay on THIS bus so emit() and subscribe() meet.
    expect(isLocalOnlyBackendEvent("task:updated")).toBe(false);
    expect(isLocalOnlyBackendEvent("task:created")).toBe(false);
  });

  it("delegates chrome subscriptions so they still fire while remote is active", () => {
    const { bus, localBus } = harness();
    const handler = vi.fn();
    bus.subscribe("ralphx://check-for-updates", handler);

    localBus.emit("ralphx://check-for-updates", { v: 1 });
    expect(handler).toHaveBeenCalledWith({ v: 1 });
  });

  it("unsubscribing a delegated chrome handler removes it from the local bus", () => {
    const { bus, localBus } = harness();
    const unsubscribe = bus.subscribe("gh-auth:login_prompt", vi.fn());
    expect(localBus.getListenerCount("gh-auth:login_prompt")).toBe(1);
    unsubscribe();
    expect(localBus.getListenerCount("gh-auth:login_prompt")).toBe(0);
  });
});

// ===========================================================================
// Cold hydration — the H barrier (§3.4)
// ===========================================================================

describe("cold hydration observes the H barrier", () => {
  it("subscribes at H BEFORE hydrating", async () => {
    const { bus, sent, hydrate } = harness({ deferHydration: true });
    const order: string[] = [];
    hydrate.mockImplementation(async () => {
      order.push("hydrate");
    });

    await bus.beginStream(hello({ maxSeq: 100 }));

    expect(sent[0]).toEqual({
      type: "subscribe",
      afterSeq: 100,
      streamEpoch: "epoch-1",
    });
    expect(order).toEqual(["hydrate"]);
  });

  it("buffers durable frames > H unapplied, then applies them in seq order", async () => {
    const h = harness({ deferHydration: true });
    const seen: number[] = [];
    h.bus.subscribe<{ n: number }>("task:created", (payload) => seen.push(payload.n));

    const started = h.bus.beginStream(hello({ maxSeq: 100 }));
    await Promise.resolve();

    // Arrive out of order while the snapshot loads.
    h.bus.handleFrame({ type: "event", seq: 102, name: "task:created", payload: { n: 102 } });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: { n: 101 } });
    expect(seen).toEqual([]); // unapplied — the snapshot is not in yet

    h.releaseHydration();
    await started;

    expect(seen).toEqual([101, 102]);
    expect(h.bus.cursor().lastSeq).toBe(102);
  });

  it("drops buffered frames at or below H — the snapshot already reflects them", async () => {
    const h = harness({ deferHydration: true });
    const seen: number[] = [];
    h.bus.subscribe<{ n: number }>("task:created", (payload) => seen.push(payload.n));

    const started = h.bus.beginStream(hello({ maxSeq: 100 }));
    await Promise.resolve();
    h.bus.handleFrame({ type: "event", seq: 99, name: "task:created", payload: { n: 99 } });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: { n: 101 } });
    h.releaseHydration();
    await started;

    expect(seen).toEqual([101]);
  });

  it("sends the first cursorAck AFTER the buffered apply, releasing the hydration span", async () => {
    const h = harness({ deferHydration: true });
    h.bus.subscribe("task:created", () => {});

    const started = h.bus.beginStream(hello({ maxSeq: 100 }));
    await Promise.resolve();
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: {} });
    expect(h.sent.filter((frame) => frame.type === "cursorAck")).toEqual([]);

    h.releaseHydration();
    await started;

    expect(h.sent.filter((frame) => frame.type === "cursorAck")).toEqual([
      { type: "cursorAck", seq: 101 },
    ]);
  });

  it("dispatches transient frames during hydration — they carry no cursor", async () => {
    const h = harness({ deferHydration: true });
    const chunks: unknown[] = [];
    h.bus.subscribe("agent:chunk", (payload) => chunks.push(payload));

    const started = h.bus.beginStream(hello({ maxSeq: 100 }));
    await Promise.resolve();
    h.bus.handleFrame({ type: "event", seq: null, name: "agent:chunk", payload: "tok" });

    expect(chunks).toEqual(["tok"]);
    h.releaseHydration();
    await started;
    expect(h.bus.cursor().lastSeq).toBe(100); // unchanged by a transient
  });

  it("treats hydration-buffer overflow as a reset and asks the supervisor to restart", async () => {
    const localBus = new MockEventBus();
    const restarts: StreamRestartCause[] = [];
    let release: () => void = () => {};
    const bus = new NetworkEventBus({
      environmentId: ENV,
      localBus,
      sendFrame: async () => {},
      hydrate: async () => {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      },
      sweep: vi.fn(),
      onRestartRequired: (cause) => restarts.push(cause),
      hydrationBufferLimit: 2,
    });

    const started = bus.beginStream(hello({ maxSeq: 100 }));
    await Promise.resolve();
    bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: {} });
    bus.handleFrame({ type: "event", seq: 102, name: "task:created", payload: {} });
    bus.handleFrame({ type: "event", seq: 103, name: "task:created", payload: {} });

    expect(restarts).toEqual([{ kind: "buffer_overflow" }]);
    release();
    await started;
    expect(bus.cursor().valid).toBe(false);
  });

  it("discards a hydration that a mid-hydration reset already superseded", async () => {
    const h = harness({ deferHydration: true });
    const seen: number[] = [];
    h.bus.subscribe<{ n: number }>("task:created", (p) => seen.push(p.n));

    const started = h.bus.beginStream(hello({ maxSeq: 100 }));
    await Promise.resolve();
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: { n: 101 } });
    h.bus.handleFrame({ type: "reset", reason: "epoch_changed" });

    h.releaseHydration();
    await started;

    // The buffered frame must NOT be spliced into a stream the reset abandoned.
    expect(seen).toEqual([]);
    expect(h.restarts).toEqual([{ kind: "reset", reason: "epoch_changed" }]);
    expect(h.bus.cursor().valid).toBe(false);
  });
});

// ===========================================================================
// Warm resume
// ===========================================================================

describe("warm resume", () => {
  async function connectedAt(seq: number): Promise<Harness> {
    const h = harness();
    h.bus.subscribe("task:created", () => {});
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    for (let next = 101; next <= seq; next += 1) {
      h.bus.handleFrame({ type: "event", seq: next, name: "task:created", payload: {} });
    }
    return h;
  }

  it("resumes at lastSeq when every frame through it committed", async () => {
    const h = await connectedAt(103);
    expect(h.bus.cursor().lastSeq).toBe(103);

    h.bus.handleStreamClosed();
    h.sent.length = 0;
    h.hydrate.mockClear();

    await h.bus.beginStream(hello({ maxSeq: 110 }));

    expect(h.sent[0]).toEqual({
      type: "subscribe",
      afterSeq: 103,
      streamEpoch: "epoch-1",
    });
    expect(h.hydrate).not.toHaveBeenCalled();
  });

  it("cold-hydrates when the epoch rolled", async () => {
    const h = await connectedAt(103);
    h.bus.handleStreamClosed();
    h.sent.length = 0;
    h.hydrate.mockClear();

    await h.bus.beginStream(hello({ maxSeq: 200, streamEpoch: "epoch-2" }));

    expect(h.sent[0]).toEqual({
      type: "subscribe",
      afterSeq: 200,
      streamEpoch: "epoch-2",
    });
    expect(h.hydrate).toHaveBeenCalledTimes(1);
  });

  it("discards the cursor when the host answers with a different environmentId", async () => {
    const h = await connectedAt(103);
    h.bus.handleStreamClosed();
    h.sent.length = 0;
    h.hydrate.mockClear();

    await h.bus.beginStream(hello({ maxSeq: 300, hostEnvironmentId: "other-host" }));

    expect(h.hydrate).toHaveBeenCalledTimes(1);
    expect(h.sent[0]).toEqual({
      type: "subscribe",
      afterSeq: 300,
      streamEpoch: "epoch-1",
    });
  });

  it("cold-hydrates when lastSeq is above the host's maxSeq", async () => {
    const h = await connectedAt(103);
    h.bus.handleStreamClosed();
    h.hydrate.mockClear();

    await h.bus.beginStream(hello({ maxSeq: 50 }));
    expect(h.hydrate).toHaveBeenCalledTimes(1);
  });

  it("a fresh bus always cold-hydrates — no cursor is persisted (A-12)", async () => {
    const h = harness();
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    expect(h.hydrate).toHaveBeenCalledTimes(1);
  });
});

// ===========================================================================
// P-24 — the canonical resume rule
// ===========================================================================

describe("P-24a: an uncommitted projection does not advance the cursor", () => {
  it("freezes lastSeq when a handler throws and forces a cold hydrate next reconnect", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const h = harness();
    let failNext = false;
    h.bus.subscribe("task:created", () => {
      if (failNext) {
        throw new Error("projection failed");
      }
    });

    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: {} });
    expect(h.bus.cursor().lastSeq).toBe(101);

    failNext = true;
    h.bus.handleFrame({ type: "event", seq: 102, name: "task:created", payload: {} });

    expect(h.bus.cursor().lastSeq).toBe(101); // frozen, not advanced
    expect(h.bus.cursor().valid).toBe(false);

    h.bus.handleStreamClosed();
    h.hydrate.mockClear();
    h.sent.length = 0;
    await h.bus.beginStream(hello({ maxSeq: 150 }));

    expect(h.hydrate).toHaveBeenCalledTimes(1); // cold, not warm
    expect(h.sent[0]).toEqual({
      type: "subscribe",
      afterSeq: 150,
      streamEpoch: "epoch-1",
    });
  });

  it("never acks past a frozen cursor", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const h = harness();
    h.bus.subscribe("task:created", () => {
      throw new Error("projection failed");
    });

    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.sent.length = 0;
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: {} });
    h.bus.handleFrame({ type: "heartbeat", t: 1 });
    await Promise.resolve();

    expect(h.sent.filter((frame) => frame.type === "cursorAck")).toEqual([]);
  });

  it("still delivers later frames to the UI while the cursor stays frozen", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const h = harness();
    let fail = true;
    const seen: number[] = [];
    h.bus.subscribe<{ n: number }>("task:created", (payload) => {
      if (fail) {
        fail = false;
        throw new Error("projection failed");
      }
      seen.push(payload.n);
    });

    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: { n: 101 } });
    h.bus.handleFrame({ type: "event", seq: 102, name: "task:created", payload: { n: 102 } });

    expect(seen).toEqual([102]);
    expect(h.bus.cursor().lastSeq).toBe(100);
  });
});

describe("P-24b/c: the reconnect sweep", () => {
  it("runs after replay on a WARM reconnect", async () => {
    const h = harness();
    h.bus.subscribe("task:created", () => {});
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: {} });
    h.sweep.mockClear();

    h.bus.handleStreamClosed();
    await h.bus.beginStream(hello({ maxSeq: 110 }));
    expect(h.sweep).not.toHaveBeenCalled(); // not until replay completes
    h.bus.handleFrame({ type: "replayDone", throughSeq: 110 });

    expect(h.sweep).toHaveBeenCalledTimes(1);
  });

  it("runs after the buffered apply on a COLD reconnect", async () => {
    const h = harness();
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    expect(h.sweep).toHaveBeenCalledTimes(1);
  });

  it("repairs an invalidation lost to a drop before its refetch landed", async () => {
    // The handler enqueues an invalidation (returns fine) and the socket then dies
    // before React Query refetches. The sweep on the next reconnect re-invalidates.
    const h = harness();
    const invalidated: string[] = [];
    h.bus.subscribe("task:created", () => invalidated.push("tasks"));

    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: {} });
    h.bus.handleStreamClosed();
    h.sweep.mockClear();

    await h.bus.beginStream(hello({ maxSeq: 101 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 101 });

    expect(h.sweep).toHaveBeenCalledTimes(1);
  });
});

// ===========================================================================
// Reset, epoch, contiguity
// ===========================================================================

describe("reset and epoch handling (P-10 client half)", () => {
  it.each([
    "cursor_pruned",
    "epoch_changed",
    "after_seq_gt_max",
    "read_error",
    "revoked",
    "host_disabled",
  ] as const)("reset(%s) invalidates the cursor and asks for a restart", async (reason) => {
    const h = harness();
    h.bus.subscribe("task:created", () => {});
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: {} });

    h.bus.handleFrame({ type: "reset", reason });

    expect(h.restarts).toEqual([{ kind: "reset", reason }]);
    expect(h.bus.cursor().valid).toBe(false);
    expect(h.bus.cursor().lastSeq).toBe(0);
  });

  it("a reset always forces the next attempt cold, whatever the reason", async () => {
    const h = harness();
    h.bus.subscribe("task:created", () => {});
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: {} });
    h.bus.handleFrame({ type: "reset", reason: "cursor_pruned" });
    h.hydrate.mockClear();

    await h.bus.beginStream(hello({ maxSeq: 400, streamEpoch: "epoch-1" }));
    expect(h.hydrate).toHaveBeenCalledTimes(1);
  });

  it("treats a sequence hole as untrustworthy and restarts", async () => {
    const h = harness();
    h.bus.subscribe("task:created", () => {});
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.bus.handleFrame({ type: "event", seq: 105, name: "task:created", payload: {} });

    expect(h.restarts).toEqual([{ kind: "sequence_hole", expected: 101, received: 105 }]);
  });

  it("drops duplicate/replayed frames at or below the cursor without restarting", async () => {
    const h = harness();
    const seen: number[] = [];
    h.bus.subscribe<{ n: number }>("task:created", (p) => seen.push(p.n));
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: { n: 101 } });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: { n: 101 } });

    expect(seen).toEqual([101]);
    expect(h.restarts).toEqual([]);
  });

  it("treats an unexpected second hello as a protocol error", async () => {
    const h = harness();
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({
      type: "hello",
      protocolVersion: 1,
      environmentId: ENV,
      streamEpoch: "epoch-1",
      serverVersion: "1.0",
      maxSeq: 100,
      heartbeatSecs: 20,
    });

    expect(h.restarts[0]?.kind).toBe("protocol_error");
  });
});

// ===========================================================================
// cursorAck cadence + A-5
// ===========================================================================

describe("cursorAck cadence (§3.4 lease lifecycle)", () => {
  it("acks at heartbeat cadence as the committed cursor advances", async () => {
    const h = harness();
    h.bus.subscribe("task:created", () => {});
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.sent.length = 0;

    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: {} });
    h.bus.handleFrame({ type: "heartbeat", t: 1 });
    await Promise.resolve();
    h.bus.handleFrame({ type: "event", seq: 102, name: "task:created", payload: {} });
    h.bus.handleFrame({ type: "heartbeat", t: 2 });
    await Promise.resolve();

    expect(h.sent).toEqual([
      { type: "cursorAck", seq: 101 },
      { type: "cursorAck", seq: 102 },
    ]);
  });

  it("does not re-ack a cursor that has not moved", async () => {
    const h = harness();
    h.bus.subscribe("task:created", () => {});
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: {} });
    h.bus.handleFrame({ type: "heartbeat", t: 1 });
    await Promise.resolve();
    h.sent.length = 0;

    h.bus.handleFrame({ type: "heartbeat", t: 2 });
    h.bus.handleFrame({ type: "heartbeat", t: 3 });
    await Promise.resolve();

    expect(h.sent).toEqual([]);
  });

  it("never acks a transient frame", async () => {
    const h = harness();
    h.bus.subscribe("agent:chunk", () => {});
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.sent.length = 0;

    h.bus.handleFrame({ type: "event", seq: null, name: "agent:chunk", payload: "x" });
    h.bus.handleFrame({ type: "heartbeat", t: 1 });
    await Promise.resolve();

    expect(h.sent).toEqual([]);
  });
});

describe("A-5: the bus is not a retry owner", () => {
  it("schedules no timers on any path", async () => {
    const setTimeoutSpy = vi.spyOn(globalThis, "setTimeout");
    const setIntervalSpy = vi.spyOn(globalThis, "setInterval");

    const h = harness();
    h.bus.subscribe("task:created", () => {});
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.bus.handleFrame({ type: "event", seq: 101, name: "task:created", payload: {} });
    h.bus.handleFrame({ type: "heartbeat", t: 1 });
    h.bus.handleFrame({ type: "reset", reason: "read_error" });
    h.bus.handleStreamClosed();

    expect(setTimeoutSpy).not.toHaveBeenCalled();
    expect(setIntervalSpy).not.toHaveBeenCalled();
  });

  it("delegates every restart decision to the supervisor", async () => {
    const h = harness();
    h.bus.subscribe("task:created", () => {});
    await h.bus.beginStream(hello({ maxSeq: 100 }));
    h.bus.handleFrame({ type: "replayDone", throughSeq: 100 });
    h.sent.length = 0;
    h.bus.handleFrame({ type: "reset", reason: "epoch_changed" });

    // No re-subscribe, no re-dial — only a request to the supervisor.
    expect(h.sent).toEqual([]);
    expect(h.restarts).toHaveLength(1);
  });
});

describe("environment scoping", () => {
  it("reports the environment it projects, so a shared relay channel can be filtered", () => {
    const { bus } = harness();
    expect(bus.environmentId()).toBe(ENV);
  });

  it("is an EventBus", () => {
    const { bus } = harness();
    const asBus: EventBus = bus;
    expect(typeof asBus.subscribe).toBe("function");
    expect(typeof asBus.emit).toBe("function");
  });
});
