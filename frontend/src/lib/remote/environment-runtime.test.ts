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

const { supervisors } = vi.hoisted(() => ({
  supervisors: [] as Array<{
    deps: {
      environmentId: string;
      refreshScopes: () => Promise<readonly string[]>;
      applyScopes: (scopes: readonly string[]) => void;
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
      };
      supervisors.push(this.record);
    }

    start(): void {
      this.record.starts += 1;
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
