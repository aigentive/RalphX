// PR 2.5: the runtime is the ONE place the registry is pulled from Rust.
//
// Before this, `loadEnvironments`/`hydrateActiveEnvironment` existed with zero
// production callers, so a paired environment was durable in SQLite and invisible in
// the app until something happened to call them. The composition root owns the load
// because it already owns "flag on ⇒ environments become live".

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { RemoteEnvironmentSummary } from "@/api/remote-environments";
import { resetQueryClient } from "@/lib/queryClient";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";
import { useUiStore } from "@/stores/uiStore";

const { listMock, getActiveMock, setActiveMock } = vi.hoisted(() => ({
  listMock: vi.fn(),
  getActiveMock: vi.fn(),
  setActiveMock: vi.fn(),
}));

vi.mock("@/api/remote-environments", () => ({
  remoteEnvironmentsApi: {
    list: listMock,
    getActiveEnvironment: getActiveMock,
    setActiveEnvironment: setActiveMock,
  },
}));

vi.mock("./supervisor", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  class FakeConnectionSupervisor {
    start(): void {}
    stop(): void {}
    currentState(): string {
      return "idle";
    }
    streamLost(): void {}
    authorityWithdrawn(): void {}
    noteFrameActivity(): void {}
    visibilityChanged(): void {}
    networkChanged(): void {}
  }
  return { ...actual, ConnectionSupervisor: FakeConnectionSupervisor };
});

vi.mock("./network-fetch", () => ({ networkFetch: vi.fn() }));
vi.mock("#tauri-core-primitive", () => ({ invoke: vi.fn(async () => undefined) }));

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
  useUiStore.setState({ featureFlags: { ...flags, remoteEnvironments: enabled } });
}

function resetStores(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [{ id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" }],
    connectionStates: { [LOCAL_ENVIRONMENT_ID]: "connected" },
  });
  setFlag(false);
}

/** Lets the runtime's floating registry-load promise settle. */
async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

let teardown: (() => void) | null = null;

beforeEach(() => {
  vi.clearAllMocks();
  listMock.mockResolvedValue([]);
  getActiveMock.mockResolvedValue(LOCAL_ENVIRONMENT_ID);
  setActiveMock.mockResolvedValue(null);
  resetStores();
  resetQueryClient();
});

afterEach(() => {
  teardown?.();
  teardown = null;
  resetStores();
  resetQueryClient();
});

describe("environment runtime registry loading", () => {
  it("loads registered environments into the store on init when the flag is on", async () => {
    listMock.mockResolvedValue([summary("env-a")]);
    setFlag(true);
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");

    teardown = initializeEnvironmentRuntime();
    await settle();

    expect(listMock).toHaveBeenCalledTimes(1);
    expect(useEnvironmentStore.getState().environments.map((e) => e.id)).toEqual([
      LOCAL_ENVIRONMENT_ID,
      "env-a",
    ]);
  });

  it("adopts the Rust-side active environment after the list lands", async () => {
    listMock.mockResolvedValue([summary("env-a")]);
    getActiveMock.mockResolvedValue("env-a");
    setFlag(true);
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");

    teardown = initializeEnvironmentRuntime();
    await settle();

    // Rust is the authority; the mirror follows it rather than clamping to local.
    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe("env-a");
    expect(setActiveMock).not.toHaveBeenCalled();
  });

  it("stays dark while the flag is off (P-11 / dark ship)", async () => {
    setFlag(false);
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");

    teardown = initializeEnvironmentRuntime();
    await settle();

    expect(listMock).not.toHaveBeenCalled();
    expect(getActiveMock).not.toHaveBeenCalled();
    expect(useEnvironmentStore.getState().environments).toHaveLength(1);
  });

  it("loads the registry when the flag flips on later", async () => {
    listMock.mockResolvedValue([summary("env-a")]);
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");

    teardown = initializeEnvironmentRuntime();
    await settle();
    expect(listMock).not.toHaveBeenCalled();

    setFlag(true);
    await settle();

    expect(listMock).toHaveBeenCalledTimes(1);
    expect(useEnvironmentStore.getState().environments.map((e) => e.id)).toContain(
      "env-a",
    );
  });

  it("leaves the store on local when the registry read fails, without throwing", async () => {
    listMock.mockRejectedValue(new Error("db down"));
    setFlag(true);
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");

    teardown = initializeEnvironmentRuntime();
    await expect(settle()).resolves.toBeUndefined();

    // Fail closed: an unreadable registry shows nothing, never a half-built list.
    expect(useEnvironmentStore.getState().environments).toHaveLength(1);
    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe(
      LOCAL_ENVIRONMENT_ID,
    );
  });

  it("does not re-list on an unrelated environment-store update", async () => {
    listMock.mockResolvedValue([summary("env-a")]);
    setFlag(true);
    const { initializeEnvironmentRuntime } = await import("./environment-runtime");

    teardown = initializeEnvironmentRuntime();
    await settle();
    expect(listMock).toHaveBeenCalledTimes(1);

    // The pane refreshing the list is the OTHER writer path; the runtime reacts to the
    // resulting store change by reconciling supervisors, not by listing again.
    useEnvironmentStore.getState().setEnvironments([summary("env-a"), summary("env-b")]);
    await settle();

    expect(listMock).toHaveBeenCalledTimes(1);
  });
});
