/**
 * The connection supervisor: the FSM driven from the checked-in transition table, the
 * one-shot connect sequence, and the four proof obligations that live here —
 *
 *  - P-9  dead host detected ≤ 60 s; lands in `reconnecting`, never `connected`
 *  - P-10 version skew parks in `blocked` with zero scheduled retries
 *  - P-14 a remote supervisor cycling backoff produces no local-environment activity
 *  - P-25 `suspended` parks without burning attempts; resume reopens the WS and
 *         completes hello/replay BEFORE the probe
 *  - P-28 effective scopes are refreshed on every (re)connect, and an introspection
 *         failure fails closed
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  SUPERVISOR_TRANSITIONS,
  type SupervisorState,
} from "./supervisor-transition-table";
import {
  ConnectAttemptError,
  ConnectionSupervisor,
  SUSPEND_DEBOUNCE_MS,
  type EnvironmentDescriptorView,
  type SupervisorDeps,
} from "./supervisor";
import type { RemoteConnectOutcome } from "./stream-frames";
import { RemoteTransportError } from "./transport-errors";

const ENV = "env-uuid-1";
const HOST_ENV = "host-env-abc";

const DESCRIPTOR: EnvironmentDescriptorView = {
  environmentId: HOST_ENV,
  protocolVersion: 1,
  minClientProtocol: 1,
};

const OUTCOME: RemoteConnectOutcome = {
  environmentId: ENV,
  hostEnvironmentId: HOST_ENV,
  streamEpoch: "epoch-1",
  maxSeq: 100,
  heartbeatSecs: 20,
  protocolVersion: 1,
};

interface Spies {
  fetchDescriptor: ReturnType<typeof vi.fn>;
  openStream: ReturnType<typeof vi.fn>;
  refreshScopes: ReturnType<typeof vi.fn>;
  applyScopes: ReturnType<typeof vi.fn>;
  beginStream: ReturnType<typeof vi.fn>;
  probe: ReturnType<typeof vi.fn>;
  releaseStream: ReturnType<typeof vi.fn>;
  onBlocked: ReturnType<typeof vi.fn>;
  onScopeRefreshFailed: ReturnType<typeof vi.fn>;
}

interface Rig {
  readonly supervisor: ConnectionSupervisor;
  readonly spies: Spies;
  readonly order: string[];
  socketLive: boolean;
  setSocketLive: (live: boolean) => void;
}

function rig(overrides: Partial<SupervisorDeps> = {}): Rig {
  const order: string[] = [];
  let socketLive = true;

  // Every spy wraps the dep the supervisor actually receives — an override replaces the
  // default BEHAVIOUR while staying the same observable spy, so a test that overrides
  // `openStream` still asserts against the function the FSM called.
  const spies: Spies = {
    fetchDescriptor: vi.fn(
      overrides.fetchDescriptor ??
        (async () => {
          order.push("descriptor");
          return DESCRIPTOR;
        })
    ),
    openStream: vi.fn(
      overrides.openStream ??
        (async () => {
          order.push("openStream");
          socketLive = true;
          return OUTCOME;
        })
    ),
    refreshScopes: vi.fn(
      overrides.refreshScopes ??
        (async () => {
          order.push("refreshScopes");
          return ["ui:read", "ui:operate"];
        })
    ),
    applyScopes: vi.fn(overrides.applyScopes ?? (() => order.push("applyScopes"))),
    beginStream: vi.fn(
      overrides.beginStream ??
        (async () => {
          order.push("beginStream");
        })
    ),
    probe: vi.fn(
      overrides.probe ??
        (async () => {
          order.push("probe");
        })
    ),
    releaseStream: vi.fn(
      overrides.releaseStream ??
        (async () => {
          order.push("releaseStream");
          socketLive = false;
        })
    ),
    onBlocked: vi.fn(overrides.onBlocked),
    onScopeRefreshFailed: vi.fn(overrides.onScopeRefreshFailed),
  };

  const supervisor = new ConnectionSupervisor({
    environmentId: ENV,
    expectedHostEnvironmentId: HOST_ENV,
    clientProtocolVersion: 1,
    clientMinProtocol: 1,
    fetchDescriptor: spies.fetchDescriptor,
    openStream: spies.openStream,
    refreshScopes: spies.refreshScopes,
    applyScopes: spies.applyScopes,
    beginStream: spies.beginStream,
    probe: spies.probe,
    releaseStream: spies.releaseStream,
    hasLiveSocket: overrides.hasLiveSocket ?? (() => socketLive),
    onBlocked: spies.onBlocked,
    onScopeRefreshFailed: spies.onScopeRefreshFailed,
    onStateChange: overrides.onStateChange,
  });

  return {
    supervisor,
    spies,
    order,
    get socketLive() {
      return socketLive;
    },
    setSocketLive: (live: boolean) => {
      socketLive = live;
    },
  };
}

/** Lets every pending microtask in the attempt chain settle under fake timers. */
async function settle(): Promise<void> {
  for (let index = 0; index < 12; index += 1) {
    await Promise.resolve();
  }
}

async function connect(r: Rig): Promise<void> {
  r.supervisor.start();
  await settle();
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

// ===========================================================================
// Table-driven FSM — every row of the checked-in artifact
// ===========================================================================

describe("FSM is driven from the checked-in transition table", () => {
  /** Drives a fresh supervisor to `state` using only real events. */
  function at(state: SupervisorState): ConnectionSupervisor {
    // Deps that never settle: the FSM is exercised in isolation from the attempt.
    const supervisor = new ConnectionSupervisor({
      environmentId: ENV,
      expectedHostEnvironmentId: HOST_ENV,
      clientProtocolVersion: 1,
      clientMinProtocol: 1,
      fetchDescriptor: () => new Promise<EnvironmentDescriptorView>(() => {}),
      openStream: () => new Promise<RemoteConnectOutcome>(() => {}),
      refreshScopes: () => new Promise<readonly string[]>(() => {}),
      applyScopes: () => {},
      beginStream: () => new Promise<void>(() => {}),
      probe: () => new Promise<void>(() => {}),
      releaseStream: async () => {},
      hasLiveSocket: () => true,
    });
    switch (state) {
      case "idle":
        break;
      case "connecting":
        supervisor.handleEvent("start");
        break;
      case "connected":
        supervisor.handleEvent("start");
        supervisor.handleEvent("connect_succeeded");
        break;
      case "backoff":
        supervisor.handleEvent("start");
        supervisor.handleEvent("connect_failed_transient");
        break;
      case "offline":
        supervisor.handleEvent("start");
        supervisor.handleEvent("offline");
        break;
      case "blocked":
        supervisor.handleEvent("start");
        supervisor.handleEvent("connect_failed_version");
        break;
      case "suspended":
        supervisor.handleEvent("start");
        supervisor.handleEvent("suspend");
        break;
    }
    expect(supervisor.currentState()).toBe(state);
    return supervisor;
  }

  it.each(SUPERVISOR_TRANSITIONS.map((row) => [row.from, row.event, row.next] as const))(
    "%s × %s → %s",
    (from, event, next) => {
      const supervisor = at(from);
      supervisor.handleEvent(event);
      expect(supervisor.currentState()).toBe(next);
    }
  );

  it("covers all seven states as a `from`", () => {
    const froms = new Set(SUPERVISOR_TRANSITIONS.map((row) => row.from));
    expect(froms.size).toBe(7);
  });
});

// ===========================================================================
// The connect attempt sequence — P-28 placement
// ===========================================================================

describe("connect attempt sequence (§6.5 key point 2)", () => {
  it("runs descriptor → stream → scope refresh → hydrate → probe, in that order", async () => {
    const r = rig();
    await connect(r);

    expect(r.order).toEqual([
      "descriptor",
      "openStream",
      "refreshScopes",
      "applyScopes",
      "beginStream",
      "probe",
    ]);
    expect(r.supervisor.currentState()).toBe("connected");
  });

  it("P-28: refreshes scopes AFTER hello and BEFORE the probe", async () => {
    const r = rig();
    await connect(r);

    const refresh = r.order.indexOf("refreshScopes");
    expect(refresh).toBeGreaterThan(r.order.indexOf("openStream"));
    expect(refresh).toBeLessThan(r.order.indexOf("probe"));
  });

  it("never reaches connected without the probe", async () => {
    const r = rig({
      probe: vi.fn(async () => {
        throw new Error("probe refused");
      }),
    });
    await connect(r);

    expect(r.supervisor.currentState()).toBe("backoff");
    expect(r.spies.beginStream).toHaveBeenCalledTimes(1);
  });

  it("never reaches connected without hydration completing", async () => {
    const r = rig({
      beginStream: vi.fn(async () => {
        throw new Error("hydration failed");
      }),
    });
    await connect(r);

    expect(r.supervisor.currentState()).toBe("backoff");
    expect(r.spies.probe).not.toHaveBeenCalled();
  });

  it("blocks on a descriptor whose environmentId is not the paired host", async () => {
    const r = rig({
      fetchDescriptor: vi.fn(async () => ({ ...DESCRIPTOR, environmentId: "someone-else" })),
    });
    await connect(r);

    expect(r.supervisor.currentState()).toBe("blocked");
    expect(r.supervisor.blocked()?.failure).toBe("malformed_descriptor");
    expect(r.spies.openStream).not.toHaveBeenCalled();
  });

  it("blocks on a hello whose host identity is not the paired host", async () => {
    const r = rig({
      openStream: vi.fn(async () => ({ ...OUTCOME, hostEnvironmentId: "someone-else" })),
    });
    await connect(r);

    expect(r.supervisor.currentState()).toBe("blocked");
    expect(r.supervisor.blocked()?.failure).toBe("malformed_descriptor");
    expect(r.spies.refreshScopes).not.toHaveBeenCalled();
  });

  it("treats an unreachable descriptor as transient, not as a malformed one", async () => {
    const r = rig({
      fetchDescriptor: vi.fn(async () => {
        throw new RemoteTransportError({
          code: "REMOTE_UNREACHABLE",
          message: "host down",
          environmentId: ENV,
        });
      }),
    });
    await connect(r);

    expect(r.supervisor.currentState()).toBe("backoff");
    expect(r.supervisor.blocked()).toBeNull();
  });

  it("blocks on 401 from anywhere in the attempt", async () => {
    const r = rig({
      openStream: vi.fn(async () => {
        throw new RemoteTransportError({
          code: "REMOTE_UNAUTHORIZED",
          message: "revoked",
          environmentId: ENV,
        });
      }),
    });
    await connect(r);

    expect(r.supervisor.currentState()).toBe("blocked");
    expect(r.supervisor.blocked()?.failure).toBe("unauthorized");
  });
});

// ===========================================================================
// P-10 — version skew
// ===========================================================================

describe("P-10: version skew parks in blocked with zero retries", () => {
  it("blocks when minClientProtocol exceeds this client", async () => {
    const r = rig({
      fetchDescriptor: vi.fn(async () => ({ ...DESCRIPTOR, minClientProtocol: 2 })),
    });
    await connect(r);

    expect(r.supervisor.currentState()).toBe("blocked");
    expect(r.supervisor.blocked()?.failure).toBe("version");
    expect(r.supervisor.blocked()?.message).toContain("2");
    expect(r.spies.onBlocked).toHaveBeenCalledWith("version", expect.any(String));
  });

  it("schedules no timers at all while blocked", async () => {
    const r = rig({
      fetchDescriptor: vi.fn(async () => ({ ...DESCRIPTOR, minClientProtocol: 2 })),
    });
    await connect(r);

    expect(r.supervisor.armedTimers()).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("never retries on its own — advancing time changes nothing", async () => {
    const r = rig({
      fetchDescriptor: vi.fn(async () => ({ ...DESCRIPTOR, minClientProtocol: 2 })),
    });
    await connect(r);
    r.spies.fetchDescriptor.mockClear();

    await vi.advanceTimersByTimeAsync(10 * 60_000);

    expect(r.spies.fetchDescriptor).not.toHaveBeenCalled();
    expect(r.supervisor.currentState()).toBe("blocked");
  });

  it("blocks when hello reports a protocol older than this client accepts", async () => {
    const r = rig({
      openStream: vi.fn(async () => ({ ...OUTCOME, protocolVersion: 0 })),
    });
    await connect(r);

    expect(r.supervisor.currentState()).toBe("blocked");
    expect(r.supervisor.blocked()?.failure).toBe("version");
  });

  it("blocks when hello CONTRADICTS the descriptor the version gate trusted", async () => {
    // The lying-cached-descriptor case: the descriptor admitted us, the live socket
    // reports a different version, so the gate we passed was evaluated against a value
    // this host does not serve.
    const r = rig({
      openStream: vi.fn(async () => ({ ...OUTCOME, protocolVersion: 2 })),
    });
    await connect(r);

    expect(r.supervisor.currentState()).toBe("blocked");
    expect(r.supervisor.blocked()?.failure).toBe("version");
    expect(r.supervisor.blocked()?.message).toContain("descriptor advertised");
  });

  it("accepts a host on a newer protocol that still admits this client", async () => {
    // A host may legitimately upgrade. As long as its minClientProtocol admits us AND
    // hello agrees with the descriptor, that is not skew — blocking here would park
    // every paired client the moment a host upgrades.
    const r = rig({
      fetchDescriptor: vi.fn(async () => ({
        environmentId: HOST_ENV,
        protocolVersion: 2,
        minClientProtocol: 1,
      })),
      openStream: vi.fn(async () => ({ ...OUTCOME, protocolVersion: 2 })),
    });
    await connect(r);

    expect(r.supervisor.currentState()).toBe("connected");
  });

  it("presents blocked as `error`", async () => {
    const r = rig({
      fetchDescriptor: vi.fn(async () => ({ ...DESCRIPTOR, minClientProtocol: 2 })),
    });
    await connect(r);
    expect(r.supervisor.presentation()).toBe("error");
  });

  it("wakes on an explicit retry without a timer having fired", async () => {
    const descriptor = vi
      .fn<() => Promise<EnvironmentDescriptorView>>()
      .mockResolvedValueOnce({ ...DESCRIPTOR, minClientProtocol: 2 })
      .mockResolvedValue(DESCRIPTOR);
    const r = rig({ fetchDescriptor: descriptor });
    await connect(r);
    expect(r.supervisor.currentState()).toBe("blocked");

    r.supervisor.retryNow();
    await settle();

    expect(r.supervisor.currentState()).toBe("connected");
  });
});

describe("authority withdrawal parks instead of redialling", () => {
  it("blocks with a populated reason and burns no ws-tickets", async () => {
    const r = rig();
    await connect(r);
    r.spies.openStream.mockClear();

    r.supervisor.authorityWithdrawn("The host ended this device's session (revoked).");

    expect(r.supervisor.currentState()).toBe("blocked");
    expect(r.supervisor.blocked()?.failure).toBe("unauthorized");
    expect(r.supervisor.blocked()?.message).toContain("revoked");
    expect(r.spies.onBlocked).toHaveBeenCalledWith("unauthorized", expect.any(String));
    expect(r.supervisor.armedTimers()).toEqual([]);

    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(r.spies.openStream).not.toHaveBeenCalled();
    expect(r.supervisor.presentation()).toBe("error");
  });
});

// ===========================================================================
// P-9 — dead-host detection
// ===========================================================================

describe("P-9: a dead host is detected within a minute", () => {
  it("closes and lands in reconnecting after 50 s of frame silence", async () => {
    const r = rig();
    await connect(r);
    expect(r.supervisor.currentState()).toBe("connected");

    await vi.advanceTimersByTimeAsync(49_000);
    expect(r.supervisor.currentState()).toBe("connected");

    // Just past the 50 s barrier, and short of the 1 s retry that follows it, so the
    // assertion sees the reconnecting state rather than the redial it schedules.
    await vi.advanceTimersByTimeAsync(1_100);

    expect(r.spies.releaseStream).toHaveBeenCalled();
    expect(r.supervisor.currentState()).toBe("backoff");
    expect(r.supervisor.presentation()).toBe("reconnecting");
  });

  it("never presents connected after the host goes silent", async () => {
    const r = rig();
    await connect(r);
    r.spies.openStream.mockImplementation(async () => {
      throw new Error("host gone");
    });

    await vi.advanceTimersByTimeAsync(60_000);

    expect(r.supervisor.presentation()).not.toBe("connected");
  });

  it("frame activity keeps the watchdog from firing", async () => {
    const r = rig();
    await connect(r);

    for (let tick = 0; tick < 6; tick += 1) {
      await vi.advanceTimersByTimeAsync(20_000);
      r.supervisor.noteFrameActivity();
    }

    expect(r.supervisor.currentState()).toBe("connected");
  });

  it("foreground with a live socket probes without reopening", async () => {
    const r = rig();
    await connect(r);
    r.spies.openStream.mockClear();
    r.spies.probe.mockClear();

    r.setSocketLive(true);
    r.supervisor.foreground();
    await settle();

    expect(r.spies.probe).toHaveBeenCalledTimes(1);
    expect(r.spies.openStream).not.toHaveBeenCalled();
    expect(r.supervisor.currentState()).toBe("connected");
  });

  it("foreground with a dead socket runs the full reopen-then-probe", async () => {
    const r = rig();
    await connect(r);
    r.spies.openStream.mockClear();
    r.spies.probe.mockClear();
    r.order.length = 0;

    r.setSocketLive(false);
    r.supervisor.foreground();
    await settle();

    expect(r.spies.openStream).toHaveBeenCalledTimes(1);
    expect(r.order.indexOf("openStream")).toBeLessThan(r.order.indexOf("probe"));
    expect(r.supervisor.currentState()).toBe("connected");
  });
});

// ===========================================================================
// P-25 — suspended
// ===========================================================================

describe("P-25: suspended parks without burning retries", () => {
  it("enters suspended on app background and releases the socket", async () => {
    const r = rig();
    await connect(r);

    r.supervisor.visibilityChanged(true);
    await vi.advanceTimersByTimeAsync(SUSPEND_DEBOUNCE_MS + 10);

    expect(r.supervisor.currentState()).toBe("suspended");
    expect(r.supervisor.presentation()).toBe("suspended");
    expect(r.spies.releaseStream).toHaveBeenCalled();
  });

  it("burns no retry attempts and arms no timers while parked", async () => {
    const r = rig();
    await connect(r);
    r.supervisor.visibilityChanged(true);
    await vi.advanceTimersByTimeAsync(SUSPEND_DEBOUNCE_MS + 10);
    const before = r.supervisor.attemptCount();

    await vi.advanceTimersByTimeAsync(10 * 60_000);

    expect(r.supervisor.attemptCount()).toBe(before);
    expect(r.supervisor.armedTimers()).toEqual([]);
    expect(r.supervisor.currentState()).toBe("suspended");
  });

  it("resume reopens the WS and completes hello/replay BEFORE the probe", async () => {
    const r = rig();
    await connect(r);
    r.supervisor.visibilityChanged(true);
    await vi.advanceTimersByTimeAsync(SUSPEND_DEBOUNCE_MS + 10);
    r.order.length = 0;

    r.supervisor.visibilityChanged(false);
    await settle();

    expect(r.order).toEqual([
      "descriptor",
      "openStream",
      "refreshScopes",
      "applyScopes",
      "beginStream",
      "probe",
    ]);
    expect(r.supervisor.currentState()).toBe("connected");
  });

  it("a bare successful probe against a socketless host never yields connected", async () => {
    // The host answers /health happily, but the stream cannot be opened.
    const r = rig({
      openStream: vi.fn(async () => {
        throw new Error("no socket for this device");
      }),
      probe: vi.fn(async () => {}),
    });
    r.supervisor.handleEvent("start");
    r.supervisor.handleEvent("suspend");
    expect(r.supervisor.currentState()).toBe("suspended");

    r.supervisor.visibilityChanged(false);
    await settle();

    expect(r.supervisor.currentState()).not.toBe("connected");
    expect(r.supervisor.currentState()).toBe("backoff");
  });

  it("a debounced visibility flicker is not a suspend (churn safety, D2)", async () => {
    const r = rig();
    await connect(r);
    r.spies.releaseStream.mockClear();

    r.supervisor.visibilityChanged(true);
    await vi.advanceTimersByTimeAsync(SUSPEND_DEBOUNCE_MS / 2);
    r.supervisor.visibilityChanged(false);
    await settle();

    expect(r.supervisor.currentState()).toBe("connected");
    expect(r.spies.releaseStream).not.toHaveBeenCalled();
  });

  it("suspending while in backoff keeps the attempt count", async () => {
    const r = rig({
      openStream: vi.fn(async () => {
        throw new Error("down");
      }),
    });
    await connect(r);
    await vi.advanceTimersByTimeAsync(1_100);
    await settle();
    const attempts = r.supervisor.attemptCount();
    expect(attempts).toBeGreaterThan(0);

    r.supervisor.handleEvent("suspend");
    expect(r.supervisor.currentState()).toBe("suspended");
    expect(r.supervisor.attemptCount()).toBe(attempts);
  });
});

// ===========================================================================
// Retry ladder, budget, stability
// ===========================================================================

describe("retry ladder and budgets", () => {
  it("walks 1,2,4,8,16 s and then clamps at 16 s", async () => {
    // Measure the GAP between consecutive redials rather than windowing each rung: the
    // rung is armed while the previous rejection settles, so absolute instants drift by
    // a tick while the intervals stay exact.
    const dialedAt: number[] = [];
    const r = rig({
      openStream: async () => {
        dialedAt.push(Date.now());
        throw new Error("down");
      },
    });
    await connect(r);

    await vi.advanceTimersByTimeAsync(120_000);
    await settle();

    const gaps = dialedAt.slice(1).map((at, index) => at - dialedAt[index]!);
    expect(gaps.slice(0, 6)).toEqual([1_000, 2_000, 4_000, 8_000, 16_000, 16_000]);
    // And it stays clamped rather than growing.
    expect(gaps.slice(6).every((gap) => gap === 16_000)).toBe(true);
  });

  it("resets the ladder after 30 s connected", async () => {
    const r = rig();
    await connect(r);
    expect(r.supervisor.attemptCount()).toBe(0);

    // Lose the socket: that burns one ladder rung.
    r.supervisor.streamLost();
    await settle();
    expect(r.supervisor.attemptCount()).toBe(1);

    // The 1 s retry reconnects, but connecting alone does NOT reset the ladder.
    await vi.advanceTimersByTimeAsync(1_100);
    await settle();
    expect(r.supervisor.currentState()).toBe("connected");
    expect(r.supervisor.attemptCount()).toBe(1);

    // Only 30 s of uninterrupted connection does.
    await vi.advanceTimersByTimeAsync(STABLE_PERIOD_MS_LOCAL);
    expect(r.supervisor.attemptCount()).toBe(0);
  });

  it("expires an attempt that overruns the 15 s budget", async () => {
    const r = rig({
      openStream: () => new Promise<RemoteConnectOutcome>(() => {}),
    });
    r.supervisor.start();
    await settle();
    expect(r.supervisor.currentState()).toBe("connecting");

    await vi.advanceTimersByTimeAsync(15_100);

    expect(r.supervisor.currentState()).toBe("backoff");
    expect(r.spies.releaseStream).toHaveBeenCalled();
  });

  it("a late resolution of an abandoned attempt cannot connect", async () => {
    let resolveStream: ((outcome: RemoteConnectOutcome) => void) | null = null;
    const r = rig({
      openStream: () =>
        new Promise<RemoteConnectOutcome>((resolve) => {
          resolveStream = resolve;
        }),
    });
    r.supervisor.start();
    await settle();
    await vi.advanceTimersByTimeAsync(15_100);
    expect(r.supervisor.currentState()).toBe("backoff");

    resolveStream?.(OUTCOME);
    await settle();

    expect(r.supervisor.currentState()).toBe("backoff");
    expect(r.spies.probe).not.toHaveBeenCalled();
  });

  it("going offline parks without burning an attempt", async () => {
    const r = rig({
      openStream: vi.fn(async () => {
        throw new Error("down");
      }),
    });
    await connect(r);
    const attempts = r.supervisor.attemptCount();

    r.supervisor.networkChanged(false);
    expect(r.supervisor.currentState()).toBe("offline");
    expect(r.supervisor.attemptCount()).toBe(attempts);
    expect(r.supervisor.armedTimers()).toEqual([]);

    await vi.advanceTimersByTimeAsync(5 * 60_000);
    expect(r.supervisor.attemptCount()).toBe(attempts);
  });

  it("coming back online retries immediately", async () => {
    const r = rig();
    await connect(r);
    r.supervisor.networkChanged(false);
    r.spies.openStream.mockClear();

    r.supervisor.networkChanged(true);
    await settle();

    expect(r.spies.openStream).toHaveBeenCalledTimes(1);
    expect(r.supervisor.currentState()).toBe("connected");
  });
});

const STABLE_PERIOD_MS_LOCAL = 30_100;

// ===========================================================================
// P-28 — scope refresh
// ===========================================================================

describe("P-28: effective scopes are refreshed on every (re)connect", () => {
  it("adopts a WIDENED grant on reconnect without re-pairing", async () => {
    const refreshScopes = vi
      .fn<() => Promise<readonly string[]>>()
      .mockResolvedValueOnce(["ui:read"])
      .mockResolvedValue(["ui:read", "ui:operate", "ui:agent"]);
    const r = rig({ refreshScopes });
    await connect(r);
    expect(r.supervisor.scopes()).toEqual(["ui:read"]);

    r.supervisor.streamLost();
    await vi.advanceTimersByTimeAsync(1_100);
    await settle();

    expect(r.supervisor.scopes()).toEqual(["ui:read", "ui:operate", "ui:agent"]);
    expect(r.spies.applyScopes).toHaveBeenLastCalledWith([
      "ui:read",
      "ui:operate",
      "ui:agent",
    ]);
  });

  it("adopts a NARROWED grant on reconnect", async () => {
    const refreshScopes = vi
      .fn<() => Promise<readonly string[]>>()
      .mockResolvedValueOnce(["ui:read", "ui:operate", "ui:agent"])
      .mockResolvedValue(["ui:read"]);
    const r = rig({ refreshScopes });
    await connect(r);

    r.supervisor.streamLost();
    await vi.advanceTimersByTimeAsync(1_100);
    await settle();

    expect(r.supervisor.scopes()).toEqual(["ui:read"]);
  });

  it("fails CLOSED on introspection failure: no widening, surfaced, not connected", async () => {
    const refreshScopes = vi
      .fn<() => Promise<readonly string[]>>()
      .mockResolvedValueOnce(["ui:read"])
      .mockRejectedValue(new Error("session endpoint unavailable"));
    const r = rig({ refreshScopes });
    await connect(r);
    expect(r.supervisor.scopes()).toEqual(["ui:read"]);
    r.spies.applyScopes.mockClear();

    r.supervisor.streamLost();
    await vi.advanceTimersByTimeAsync(1_100);
    await settle();

    // Gating never widens past the last confirmed set…
    expect(r.supervisor.scopes()).toEqual(["ui:read"]);
    expect(r.spies.applyScopes).not.toHaveBeenCalled();
    // …the failure is surfaced, not swallowed…
    expect(r.spies.onScopeRefreshFailed).toHaveBeenCalled();
    // …and the session never becomes connected on an unverified view.
    expect(r.supervisor.currentState()).not.toBe("connected");
  });

  it("blocks when introspection is refused with 401", async () => {
    const r = rig({
      refreshScopes: vi.fn(async () => {
        throw new RemoteTransportError({
          code: "REMOTE_UNAUTHORIZED",
          message: "revoked",
          environmentId: ENV,
        });
      }),
    });
    await connect(r);

    expect(r.supervisor.currentState()).toBe("blocked");
    expect(r.supervisor.blocked()?.failure).toBe("unauthorized");
  });

  it("does not probe when the scope refresh failed", async () => {
    const r = rig({
      refreshScopes: vi.fn(async () => {
        throw new Error("unavailable");
      }),
    });
    await connect(r);

    expect(r.spies.probe).not.toHaveBeenCalled();
  });
});

// ===========================================================================
// P-14 — flapping isolation
// ===========================================================================

describe("P-14: a flapping remote supervisor leaves other environments alone", () => {
  it("cycles backoff without touching another environment's deps", async () => {
    const a = rig();
    await connect(a);
    a.spies.fetchDescriptor.mockClear();
    a.spies.openStream.mockClear();
    a.spies.releaseStream.mockClear();
    a.spies.probe.mockClear();

    const b = rig({
      openStream: vi.fn(async () => {
        throw new Error("flapping");
      }),
    });
    b.supervisor.start();
    await settle();

    for (let cycle = 0; cycle < 5; cycle += 1) {
      await vi.advanceTimersByTimeAsync(20_000);
      // A's host keeps heartbeating normally throughout B's outage.
      a.supervisor.noteFrameActivity();
      await settle();
    }

    expect(b.supervisor.currentState()).toBe("backoff");
    expect(b.spies.openStream.mock.calls.length).toBeGreaterThan(2);

    // Zero local-environment activity: A neither redialled, reprobed, nor re-scoped.
    expect(a.supervisor.currentState()).toBe("connected");
    expect(a.spies.fetchDescriptor).not.toHaveBeenCalled();
    expect(a.spies.openStream).not.toHaveBeenCalled();
    expect(a.spies.probe).not.toHaveBeenCalled();
    expect(a.spies.releaseStream).not.toHaveBeenCalled();
    expect(a.spies.applyScopes).toHaveBeenCalledTimes(1); // the original connect only
  });

  it("blocking one environment does not block another", async () => {
    const a = rig();
    await connect(a);

    const b = rig({
      fetchDescriptor: vi.fn(async () => ({ ...DESCRIPTOR, minClientProtocol: 9 })),
    });
    await connect(b);

    expect(b.supervisor.currentState()).toBe("blocked");
    expect(a.supervisor.currentState()).toBe("connected");
  });
});

// ===========================================================================
// Teardown
// ===========================================================================

describe("stop", () => {
  it("releases the socket and clears every timer", async () => {
    const r = rig();
    await connect(r);
    r.spies.releaseStream.mockClear();

    r.supervisor.stop();

    expect(r.supervisor.currentState()).toBe("idle");
    expect(r.spies.releaseStream).toHaveBeenCalledTimes(1);
    expect(r.supervisor.armedTimers()).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("a stopped supervisor stays inert as time passes", async () => {
    const r = rig();
    await connect(r);
    r.supervisor.stop();
    r.spies.openStream.mockClear();

    await vi.advanceTimersByTimeAsync(10 * 60_000);

    expect(r.spies.openStream).not.toHaveBeenCalled();
    expect(r.supervisor.currentState()).toBe("idle");
  });
});

describe("typed failure classification (rule 5)", () => {
  it("classifies by type, never by message", async () => {
    const r = rig({
      openStream: vi.fn(async () => {
        throw new ConnectAttemptError("version", "anything at all");
      }),
    });
    await connect(r);
    expect(r.supervisor.blocked()?.failure).toBe("version");
  });
});
