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
      onStateChange: (state: string) => void;
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
    authorityWithdrawals: string[];
    setState: (state: string) => void;
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
        authorityWithdrawals: [],
        setState: (state: string) => {
          this.state = state;
        },
      };
      supervisors.push(this.record);
    }

    state: string = "idle";

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
    expect(invalidate).toHaveBeenCalledWith(
      { refetchType: "all" },
      { throwOnError: true }
    );
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
    c.deps.onStateChange("connected");
    b.setState("connected");
    b.deps.onStateChange("connected");

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
    b.deps.onStateChange("connected");
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

  it("uses pairing scopes in background without a session fetch and records them", async () => {
    const { getConfirmedScopes, initializeEnvironmentRuntime } = await import(
      "./environment-runtime"
    );
    useEnvironmentStore.getState().setEnvironments([summary("env-b")]);
    setFlag(true);
    teardown = initializeEnvironmentRuntime();
    const runtime = supervisors[0];

    const scopes = await runtime?.deps.refreshScopes();
    runtime?.deps.applyScopes(scopes ?? []);

    expect(scopes).toEqual(["ui:read"]);
    expect(getConfirmedScopes("env-b")).toEqual(["ui:read"]);
  });
});
