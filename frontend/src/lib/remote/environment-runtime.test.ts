import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RemoteEnvironmentSummary } from "@/api/remote-environments";
import { createEventBus, type EventBus } from "@/lib/event-bus";
import { getQueryClient, resetQueryClient } from "@/lib/queryClient";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";
import { useUiStore } from "@/stores/uiStore";

import type { RemoteStreamTarget } from "./stream-relay";
import { RemoteTransportError } from "./transport-errors";

const { supervisors } = vi.hoisted(() => ({
  supervisors: [] as Array<{
    deps: {
      environmentId: string;
      refreshScopes: () => Promise<readonly string[]>;
      applyScopes: (scopes: readonly string[]) => void;
      openStream: () => Promise<unknown>;
      onStateChange: (state: string, presentation: string) => void;
      beginStream: (outcome: {
        environmentId: string;
        hostEnvironmentId: string;
        streamEpoch: string;
        maxSeq: number;
        heartbeatSecs: number;
        protocolVersion: number;
      }) => Promise<void>;
    };
    starts: number;
    stops: number;
    visibility: boolean[];
    networks: boolean[];
    streamLosses: number;
    authorityVerifications: number;
    authorityWithdrawals: string[];
    setState: (state: string) => void;
    setBlocked: (
      blocked: { failure: string; message: string } | null
    ) => void;
  }>,
}));

vi.mock("./supervisor", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  class FakeConnectionSupervisor {
    readonly record;

    constructor(deps: (typeof supervisors)[number]["deps"]) {
      this.record = {
        deps,
        starts: 0,
        stops: 0,
        visibility: [],
        networks: [],
        streamLosses: 0,
        authorityVerifications: 0,
        authorityWithdrawals: [],
        setState: (state: string) => {
          this.state = state;
        },
        setBlocked: (blocked) => {
          this.blockedReason = blocked;
        },
      };
      supervisors.push(this.record);
    }

    state: string = "idle";
    blockedReason: { failure: string; message: string } | null = null;

    blocked(): { failure: string; message: string } | null {
      return this.state === "blocked" ? this.blockedReason : null;
    }

    start(): void {
      this.record.starts += 1;
    }
    currentState(): string {
      return this.state;
    }
    stop(): void {
      this.record.stops += 1;
    }
    streamLost(): void {
      this.record.streamLosses += 1;
    }
    verifyAuthorityNow(): void {
      this.record.authorityVerifications += 1;
    }
    authorityWithdrawn(message: string): void {
      this.record.authorityWithdrawals.push(message);
    }
    noteFrameActivity(): void {}
    visibilityChanged(hidden: boolean): void {
      this.record.visibility.push(hidden);
    }
    networkChanged(online: boolean): void {
      this.record.networks.push(online);
    }
  }
  return { ...actual, ConnectionSupervisor: FakeConnectionSupervisor };
});

vi.mock("./network-fetch", () => ({
  networkFetch: vi.fn(),
}));

vi.mock("#tauri-core-primitive", () => ({
  invoke: vi.fn(async () => undefined),
}));

const OUTCOME = {
  environmentId: "env-b",
  hostEnvironmentId: "host-env-b",
  streamEpoch: "epoch-1",
  maxSeq: 100,
  heartbeatSecs: 20,
  protocolVersion: 1,
};

function summary(id: string): RemoteEnvironmentSummary {
  return {
    id,
    environmentId: `host-${id}`,
    name: id,
    baseUrl: `https://${id}.example.test`,
    candidateUrls: [],
    scopes: ["ui:read"],
    protocolVersion: 1,
    status: "active",
    createdAt: "2026-07-28T00:00:00Z",
    lastConnectedAt: null,
  };
}

function setFlag(enabled: boolean): void {
  const flags = useUiStore.getState().featureFlags;
  useUiStore.setState({
    featureFlags: { ...flags, remoteEnvironments: enabled },
  });
}

function resetStores(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
    ],
    connectionStates: { [LOCAL_ENVIRONMENT_ID]: "connected" },
  });
  setFlag(false);
}

let teardown: (() => void) | null = null;

beforeEach(() => {
  supervisors.length = 0;
  resetStores();
  resetQueryClient();
});

afterEach(() => {
  teardown?.();
  teardown = null;
  resetStores();
  resetQueryClient();
});

describe("environment runtime composition", () => {
  it("is idempotent and teardown removes app adapters", async () => {
    const addDocument = vi.spyOn(document, "addEventListener");
    const removeDocument = vi.spyOn(document, "removeEventListener");
    const addWindow = vi.spyOn(window, "addEventListener");
    const removeWindow = vi.spyOn(window, "removeEventListener");
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");

    teardown = initializeEnvironmentRuntime();
    expect(initializeEnvironmentRuntime()).toBe(teardown);
    expect(addDocument).toHaveBeenCalledWith("visibilitychange", expect.any(Function));
    expect(addWindow).toHaveBeenCalledWith("online", expect.any(Function));
    expect(addWindow).toHaveBeenCalledWith("offline", expect.any(Function));

    teardown();
    teardown = null;
    expect(removeDocument).toHaveBeenCalledWith(
      "visibilitychange",
      expect.any(Function)
    );
    expect(removeWindow).toHaveBeenCalledWith("online", expect.any(Function));
    expect(removeWindow).toHaveBeenCalledWith("offline", expect.any(Function));
  });

  it("creates no supervisors while disabled and reconciles add/remove/disable", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b"), summary("env-c")]);
    teardown = initializeEnvironmentRuntime();
    expect(supervisors).toHaveLength(0);

    setFlag(true);
    expect(supervisors).toHaveLength(2);
    expect(supervisors.map((item) => item.deps.environmentId)).toEqual([
      "env-b",
      "env-c",
    ]);
    expect(supervisors.every((item) => item.starts > 0)).toBe(true);

    useEnvironmentStore.getState().setEnvironments([summary("env-c")]);
    expect(supervisors[0]?.stops).toBeGreaterThan(0);

    setFlag(false);
    expect(supervisors[1]?.stops).toBeGreaterThan(0);
  });

  it("clears env-scoped persisted slices when a reconciler removes the row", async () => {
    const { STORE_ISOLATION_INVENTORY } = await import("./store-isolation-inventory");
    const storageNames = STORE_ISOLATION_INVENTORY.flatMap((entry) =>
      entry.persisted ? [entry.persisted.storageName] : []
    );
    expect(storageNames.length).toBeGreaterThan(0);
    for (const name of storageNames) {
      localStorage.setItem(`${name}:env-b`, JSON.stringify({ state: {}, version: 0 }));
      localStorage.setItem(`${name}:env-c`, JSON.stringify({ state: {}, version: 0 }));
    }

    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b"), summary("env-c")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();

    // The Rust startup reconciler finished a staged removal: the row simply disappears.
    useEnvironmentStore.getState().setEnvironments([summary("env-c")]);

    for (const name of storageNames) {
      expect(localStorage.getItem(`${name}:env-b`)).toBeNull();
      expect(localStorage.getItem(`${name}:env-c`)).not.toBeNull();
    }
    for (const name of storageNames) {
      localStorage.removeItem(`${name}:env-c`);
    }
  });

  it("installs the active remote bus synchronously during a store switch", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();

    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    const bus = createEventBus("env-b");

    expect((bus as EventBus & RemoteStreamTarget).environmentId()).toBe("env-b");
    expect(supervisors[0]?.starts).toBeGreaterThan(1);
  });

  it.each(["revoked", "host_disabled"] as const)(
    "routes reset(%s) to the block path, never to the retry ladder",
    async (reason) => {
      const { initializeEnvironmentRuntime } = await import("./environment-runtime");
      useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
      setFlag(true);
      teardown = initializeEnvironmentRuntime();
      useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
      const runtime = supervisors[supervisors.length - 1];
      const bus = createEventBus("env-b") as EventBus & {
        handleFrame: (frame: { type: "reset"; reason: string }) => void;
      };

      bus.handleFrame({ type: "reset", reason });

      expect(runtime?.authorityWithdrawals).toHaveLength(1);
      expect(runtime?.authorityWithdrawals[0]).toContain(reason);
      expect(runtime?.streamLosses).toBe(0);
    }
  );

  it("routes a non-authority reset to the ordinary retry ladder", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    const runtime = supervisors[supervisors.length - 1];
    const bus = createEventBus("env-b") as EventBus & {
      handleFrame: (frame: { type: "reset"; reason: string }) => void;
    };

    bus.handleFrame({ type: "reset", reason: "cursor_pruned" });

    expect(runtime?.streamLosses).toBe(1);
    expect(runtime?.authorityWithdrawals).toEqual([]);
  });

  /**
   * P-2 / §6.5: the host reports a revocation on an established stream as
   * `error{REMOTE_UNAUTHORIZED}`, never as `reset(revoked)`. Discarding the typed code
   * into a transient protocol error cost a full backoff cycle before the ws-ticket 401
   * finally parked the device.
   */
  it("verifies once on error{REMOTE_UNAUTHORIZED} instead of entering the backoff ladder", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    const runtime = supervisors[supervisors.length - 1];
    const bus = createEventBus("env-b") as EventBus & {
      handleFrame: (frame: { type: "error"; code: string; message: string }) => void;
    };

    bus.handleFrame({
      type: "error",
      code: "REMOTE_UNAUTHORIZED",
      message: "device revoked",
    });

    expect(runtime?.authorityVerifications).toBe(1);
    expect(runtime?.streamLosses).toBe(0);
    // Not a blanket park: agent-control NARROWING reuses this rejection on a device that
    // stays paired, so the re-pair state must not be asserted from the frame alone.
    expect(runtime?.authorityWithdrawals).toEqual([]);
  });

  it("keeps a non-authority error frame on the ordinary retry ladder", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    const runtime = supervisors[supervisors.length - 1];
    const bus = createEventBus("env-b") as EventBus & {
      handleFrame: (frame: { type: "error"; code: string; message: string }) => void;
    };

    bus.handleFrame({
      type: "error",
      code: "REMOTE_INTERNAL_ERROR",
      message: "host fault",
    });

    expect(runtime?.streamLosses).toBe(1);
    expect(runtime?.authorityVerifications).toBe(0);
  });

  it("lifts a proxy refusal on the connect path into the transport taxonomy", async () => {
    const { invoke } = await import("#tauri-core-primitive");
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    const runtime = supervisors[supervisors.length - 1];

    // The Rust proxy rejects with its `"{CODE}: {message}"` rendering. Left raw, the
    // supervisor classifies a revoked device as `transient` and loops the ladder.
    vi.mocked(invoke).mockRejectedValueOnce(
      "REMOTE_UNAUTHORIZED: this device was revoked"
    );
    const failure = await runtime?.deps.openStream().catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(RemoteTransportError);
    expect((failure as RemoteTransportError).code).toBe("REMOTE_UNAUTHORIZED");
  });

  it("fails the hydration barrier when the snapshot refetch rejects", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });

    const client = getQueryClient("env-b");
    const invalidate = vi
      .spyOn(client, "invalidateQueries")
      .mockRejectedValue(new Error("host answered 500"));

    // A swallowed refetch failure would resolve the §3.4 barrier over an empty board.
    await expect(
      supervisors[supervisors.length - 1]?.deps.beginStream(OUTCOME)
    ).rejects.toThrow(/500/);
    expect(invalidate).toHaveBeenCalledWith({ refetchType: "all" });
  });

  it("fails the hydration barrier when a refetched query settles in a host error", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });

    const client = getQueryClient("env-b");
    await client.prefetchQuery({
      queryKey: ["projects", "list"],
      queryFn: () => Promise.reject(new Error("host answered 500")),
      retry: false,
    });
    vi.spyOn(client, "invalidateQueries").mockResolvedValue(undefined);

    // `invalidateQueries` without `throwOnError` swallows refetch rejections, so
    // the barrier must read the settled cache — a genuine host failure still
    // fails it (fail closed, §3.4).
    await expect(
      supervisors[supervisors.length - 1]?.deps.beginStream(OUTCOME)
    ).rejects.toThrow(/500/);
  });

  it("tolerates queries the remote facade refuses as unavailable", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });

    const client = getQueryClient("env-b");
    // The regression that shipped: ticketing/GitHub/provider queries use commands
    // the remote facade deliberately refuses. Those rejections are a capability
    // boundary, not a health signal — treating them as fatal makes connecting
    // structurally impossible while any such query is mounted.
    await client.prefetchQuery({
      queryKey: ["ticketing", "providers"],
      queryFn: () =>
        Promise.reject(
          new RemoteTransportError({
            code: "REMOTE_COMMAND_UNAVAILABLE",
            message: "Command `list_ticketing_providers` is not available remotely.",
            environmentId: "env-b",
            cmd: "list_ticketing_providers",
          })
        ),
      retry: false,
    });
    vi.spyOn(client, "invalidateQueries").mockResolvedValue(undefined);

    await expect(
      supervisors[supervisors.length - 1]?.deps.beginStream(OUTCOME)
    ).resolves.toBeUndefined();
  });

  it("keeps one bus identity per environment across reactivation", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    const first = createEventBus("env-b");

    // EventProvider memoizes the bus on environmentId alone, so a same-id
    // reactivation that rebuilt the bus would orphan every mounted subscriber.
    setFlag(false);
    setFlag(true);
    expect(createEventBus("env-b")).toBe(first);

    useEnvironmentStore.setState({ activeEnvironmentId: LOCAL_ENVIRONMENT_ID });
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    expect(createEventBus("env-b")).toBe(first);

    // A removed registry row does forget its bus.
    useEnvironmentStore.getState().setEnvironments([]);
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    expect(createEventBus("env-b")).not.toBe(first);
  });

  /**
   * 2.7-a: the composition root is the SINGLE writer of connection presentation, and it
   * writes the FSM state and the presentation projection in one commit — so no reader can
   * ever observe a blocked message under a still-connecting presentation.
   */
  it("writes presentation and blocked cause as one slice (single writer)", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    const b = supervisors.filter((item) => item.deps.environmentId === "env-b").at(-1)!;

    b.setState("backoff");
    b.deps.onStateChange("backoff", "reconnecting");
    expect(
      useEnvironmentStore.getState().connectionPresentations["env-b"]
    ).toEqual({
      presentation: "reconnecting",
      blockedFailure: null,
      blockedMessage: null,
    });

    b.setState("blocked");
    b.setBlocked({ failure: "unauthorized", message: "revoked" });
    b.deps.onStateChange("blocked", "error");
    expect(
      useEnvironmentStore.getState().connectionPresentations["env-b"]
    ).toEqual({
      presentation: "error",
      blockedFailure: "unauthorized",
      blockedMessage: "revoked",
    });

    // Leaving blocked clears the cause: no surface may keep offering re-pair over a
    // connection that is coming back.
    b.setState("connecting");
    b.setBlocked(null);
    b.deps.onStateChange("connecting", "reconnecting");
    expect(
      useEnvironmentStore.getState().connectionPresentations["env-b"]
    ).toEqual({
      presentation: "reconnecting",
      blockedFailure: null,
      blockedMessage: null,
    });
  });

  /**
   * P-24 (b)/(c) + P-21 ordering: `goLive` fires the env-scoped sweep on EVERY reconnect,
   * warm or cold, and the gate reconcile rides the same seam — replay → sweep → gates.
   */
  it("sweeps the env-scoped cache and announces a gate reconcile on goLive", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    const { PENDING_GATE_RECONCILE_EVENT } = await import(
      "./pending-gate-reconcile"
    );
    useEnvironmentStore.getState().setEnvironments([summary("env-b"), summary("env-c")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    const b = supervisors.filter((item) => item.deps.environmentId === "env-b").at(-1)!;

    const activeInvalidate = vi.spyOn(getQueryClient("env-b"), "invalidateQueries");
    const otherInvalidate = vi.spyOn(getQueryClient("env-c"), "invalidateQueries");
    const announced: string[] = [];
    const listener = (event: Event) => {
      if (event instanceof CustomEvent) {
        announced.push((event.detail as { environmentId: string }).environmentId);
      }
    };
    window.addEventListener(PENDING_GATE_RECONCILE_EVENT, listener);

    try {
      await b.deps.beginStream(OUTCOME);

      expect(activeInvalidate).toHaveBeenCalled();
      // A-8: env scoping is the KEYED client, so another environment's cache is
      // structurally unreachable from this sweep.
      expect(otherInvalidate).not.toHaveBeenCalled();
      expect(announced).toEqual(["env-b"]);
    } finally {
      window.removeEventListener(PENDING_GATE_RECONCILE_EVENT, listener);
    }
  });

  it("never paints a background environment as connected (P-25)", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b"), summary("env-c")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });

    const forEnvironment = (id: string) =>
      supervisors.filter((item) => item.deps.environmentId === id).at(-1)!;
    const b = forEnvironment("env-b");
    const c = forEnvironment("env-c");

    // env-c never sends `subscribe` and never projects: its attempt is a probe on a
    // socket nobody reads, so it must not wear the connected dot.
    c.setState("connected");
    c.deps.onStateChange("connected", "connected");
    b.setState("connected");
    b.deps.onStateChange("connected", "connected");

    expect(useEnvironmentStore.getState().connectionStates["env-c"]).toBe("health_only");
    expect(useEnvironmentStore.getState().connectionStates["env-b"]).toBe("connected");
  });

  it("demotes the outgoing environment's badge when the active one changes", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b"), summary("env-c")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    const b = supervisors.filter((item) => item.deps.environmentId === "env-b").at(-1)!;
    b.setState("connected");
    b.deps.onStateChange("connected", "connected");
    expect(useEnvironmentStore.getState().connectionStates["env-b"]).toBe("connected");

    useEnvironmentStore.setState({ activeEnvironmentId: "env-c" });

    // env-b lost its bus, so it stops projecting the instant the switch lands.
    expect(useEnvironmentStore.getState().connectionStates["env-b"]).toBe("health_only");
  });

  it("re-hydrates the target cache on every activation, local included", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();

    const local = vi
      .spyOn(getQueryClient(LOCAL_ENVIRONMENT_ID), "invalidateQueries")
      .mockResolvedValue();
    const remote = vi
      .spyOn(getQueryClient("env-b"), "invalidateQueries")
      .mockResolvedValue();

    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    expect(remote).toHaveBeenCalled();

    // Local has no supervisor to cold-hydrate it, so without an explicit sweep the
    // retained cache stays fresh for 5 minutes over whatever changed meanwhile.
    useEnvironmentStore.setState({ activeEnvironmentId: LOCAL_ENVIRONMENT_ID });
    expect(local).toHaveBeenCalled();
  });

  /**
   * P-28 BACKGROUND HALF (PR 3.3-a). A background environment cannot re-read
   * `GET /remote/v1/session`: `authorize_proxy_target` authorizes only the health ops
   * for a non-active environment (P-26), and widening the proxy to permit it is
   * exactly the erosion the badge slice is forbidden to cause.
   *
   * So the background refresh NARROWS. It previously fell back to
   * `entry.remote.scopes` — the PAIRING-TIME snapshot — which is a fail-open hole: a
   * device whose `ui:agent` grant was revoked host-side would have had it silently
   * restored by a background reconnect.
   */
  it("narrows to the empty set in background when nothing was ever confirmed", async () => {
    const { getConfirmedScopes, initializeEnvironmentRuntime } = await import(
      "./environment-runtime"
    );
    const { networkFetch } = await import("./network-fetch");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    const runtime = supervisors[0];
    vi.mocked(networkFetch).mockClear();

    const scopes = await runtime?.deps.refreshScopes();
    runtime?.deps.applyScopes(scopes ?? []);

    // NOT `["ui:read"]`: the pairing snapshot records what the host granted when the
    // environment was added and can be arbitrarily stale.
    expect(scopes).toEqual([]);
    expect(getConfirmedScopes("env-b")).toEqual([]);
    // And it stays a health-only environment — no session request was issued (P-26).
    expect(networkFetch).not.toHaveBeenCalled();
  });

  it("retains the last CONFIRMED set across a background reconnect", async () => {
    const { getConfirmedScopes, initializeEnvironmentRuntime } = await import(
      "./environment-runtime"
    );
    const { networkFetch } = await import("./network-fetch");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    const runtime = supervisors[0];
    // A previous active-session introspection confirmed a wider grant.
    runtime?.deps.applyScopes(["ui:read", "ui:operate"]);
    vi.mocked(networkFetch).mockClear();

    const scopes = await runtime?.deps.refreshScopes();

    // Retained, never widened past it, and never re-broadened from the pairing row.
    expect(scopes).toEqual(["ui:read", "ui:operate"]);
    expect(getConfirmedScopes("env-b")).toEqual(["ui:read", "ui:operate"]);
    expect(networkFetch).not.toHaveBeenCalled();
  });
});

/**
 * The connection journal exists for exactly the failure this branch shipped: a host
 * that accepts the socket but fails hydration loops "Reconnecting…" forever with no
 * user-visible reason. Every verdict the runtime reaches must leave a readable trace.
 */
describe("connection journal instrumentation", () => {
  async function journalOf(environmentId: string) {
    const { useRemoteConnectionJournalStore } = await import(
      "@/stores/remoteConnectionJournalStore"
    );
    return useRemoteConnectionJournalStore.getState().journals[environmentId] ?? [];
  }

  beforeEach(async () => {
    const { useRemoteConnectionJournalStore } = await import(
      "@/stores/remoteConnectionJournalStore"
    );
    useRemoteConnectionJournalStore.setState({ journals: {} });
  });

  it("records the offending query key when the hydration barrier fails", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });

    const client = getQueryClient("env-b");
    await client.prefetchQuery({
      queryKey: ["projects", "list"],
      queryFn: () => Promise.reject(new Error("host answered 500")),
      retry: false,
    });
    vi.spyOn(client, "invalidateQueries").mockResolvedValue(undefined);
    await expect(
      supervisors[supervisors.length - 1]?.deps.beginStream(OUTCOME)
    ).rejects.toThrow(/500/);

    const entries = (await journalOf("env-b")).filter(
      (entry) => entry.kind === "barrier"
    );
    expect(entries.length).toBeGreaterThan(0);
    expect(entries[0]?.message).toContain('["projects","list"]');
    expect(entries[0]?.detail).toContain("host answered 500");
  });

  it("records tolerated command-unavailable queries without failing the barrier", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });

    const client = getQueryClient("env-b");
    await client.prefetchQuery({
      queryKey: ["ticketing", "providers"],
      queryFn: () =>
        Promise.reject(
          new RemoteTransportError({
            code: "REMOTE_COMMAND_UNAVAILABLE",
            message: "Command `list_ticketing_providers` is not available remotely.",
            environmentId: "env-b",
            cmd: "list_ticketing_providers",
          })
        ),
      retry: false,
    });
    vi.spyOn(client, "invalidateQueries").mockResolvedValue(undefined);

    await expect(
      supervisors[supervisors.length - 1]?.deps.beginStream(OUTCOME)
    ).resolves.toBeUndefined();

    const entries = await journalOf("env-b");
    expect(entries.some((entry) => entry.kind === "barrier")).toBe(false);
    const tolerated = entries.find((entry) => entry.kind === "info");
    expect(tolerated?.message).toContain("not supported by this host");
    expect(tolerated?.detail).toContain('["ticketing","providers"]');
  });

  it("records supervisor state changes together with the blocked cause", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    const b = supervisors.filter((item) => item.deps.environmentId === "env-b").at(-1)!;

    b.setState("blocked");
    b.setBlocked({ failure: "unauthorized", message: "host refused this device" });
    b.deps.onStateChange("blocked", "error");

    const entries = (await journalOf("env-b")).filter(
      (entry) => entry.kind === "state"
    );
    expect(entries.length).toBeGreaterThan(0);
    const last = entries[entries.length - 1];
    expect(last?.message).toContain("blocked");
    expect(last?.detail).toContain("host refused this device");
  });

  it("records a host stream withdrawal as a stream event", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    const bus = createEventBus("env-b") as EventBus & {
      handleFrame: (frame: { type: "reset"; reason: string }) => void;
    };

    bus.handleFrame({ type: "reset", reason: "revoked" });

    const entries = (await journalOf("env-b")).filter(
      (entry) => entry.kind === "stream"
    );
    expect(entries.length).toBeGreaterThan(0);
    expect(entries[0]?.message).toContain("revoked");
  });

  it("journals one line per (state, presentation) pair, not per repaint", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    useEnvironmentStore.setState({ activeEnvironmentId: "env-b" });
    const b = supervisors.filter((item) => item.deps.environmentId === "env-b").at(-1)!;
    const before = (await journalOf("env-b")).length;

    // A presentation-only repaint of the SAME pair (the syncing escalation tick can
    // re-notify) must not spam the journal.
    b.setState("connecting");
    b.deps.onStateChange("connecting", "syncing");
    b.deps.onStateChange("connecting", "syncing");
    b.deps.onStateChange("connecting", "reconnecting");

    const entries = (await journalOf("env-b")).slice(before);
    expect(entries.map((entry) => entry.message)).toEqual([
      "Connection state: connecting (shown as syncing).",
      "Connection state: connecting (shown as reconnecting).",
    ]);
  });

  it("drops the journal when the registry row is removed", async () => {
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");
    const { recordConnectionEvent } = await import(
      "@/stores/remoteConnectionJournalStore"
    );
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    recordConnectionEvent("env-b", "state", "seed");

    useEnvironmentStore.getState().setEnvironments([]);

    expect(await journalOf("env-b")).toEqual([]);
  });
});
