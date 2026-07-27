import { beforeEach, describe, expect, it, vi } from "vitest";

import { remoteEnvironmentsApi } from "@/api/remote-environments";
import type { RemoteEnvironmentSummary } from "@/api/remote-environments";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "./environmentStore";

vi.mock("@/api/remote-environments", () => ({
  remoteEnvironmentsApi: {
    pair: vi.fn(),
    list: vi.fn(),
    remove: vi.fn(),
    getActiveEnvironment: vi.fn(),
    setActiveEnvironment: vi.fn(),
  },
}));

const mockedApi = vi.mocked(remoteEnvironmentsApi);

const summary = (
  overrides: Partial<RemoteEnvironmentSummary> = {}
): RemoteEnvironmentSummary => ({
  id: "row-1",
  environmentId: "env-1",
  name: "Mac Studio",
  baseUrl: "https://mac-studio.tailnet.ts.net",
  candidateUrls: [],
  scopes: ["ui:read", "ui:operate"],
  protocolVersion: 1,
  status: "active",
  createdAt: "2026-07-27T19:15:00+00:00",
  lastConnectedAt: null,
  ...overrides,
});

function resetStore() {
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
    ],
    connectionStates: { [LOCAL_ENVIRONMENT_ID]: "connected" },
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  resetStore();
});

describe("environmentStore identity (§6.4)", () => {
  it('always contains "local", which has no supervisor', () => {
    const state = useEnvironmentStore.getState();
    expect(state.environments.map((entry) => entry.id)).toContain(
      LOCAL_ENVIRONMENT_ID
    );
    expect(state.activeEnvironmentId).toBe(LOCAL_ENVIRONMENT_ID);
    expect(state.connectionStates[LOCAL_ENVIRONMENT_ID]).toBe("connected");

    // Local never gets a supervisor-driven connection state.
    state.setConnectionState(LOCAL_ENVIRONMENT_ID, "backoff");
    expect(
      useEnvironmentStore.getState().connectionStates[LOCAL_ENVIRONMENT_ID]
    ).toBe("connected");
  });

  it("keeps local first when the registry loads", async () => {
    mockedApi.list.mockResolvedValue([summary()]);

    await useEnvironmentStore.getState().loadEnvironments();

    const ids = useEnvironmentStore
      .getState()
      .environments.map((entry) => entry.id);
    expect(ids).toEqual([LOCAL_ENVIRONMENT_ID, "row-1"]);
  });
});

describe("setActiveEnvironment (first paint + Rust authority)", () => {
  beforeEach(() => {
    useEnvironmentStore
      .getState()
      .setEnvironments([summary()]);
  });

  it("updates the store synchronously before the Rust mirror resolves", async () => {
    let resolveMirror: (value: null) => void = () => {};
    mockedApi.setActiveEnvironment.mockImplementation(
      () =>
        new Promise<null>((resolve) => {
          resolveMirror = resolve;
        })
    );

    const pending = useEnvironmentStore
      .getState()
      .setActiveEnvironment("row-1");

    // First paint wins: state is switched while the invoke is still in flight.
    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe("row-1");
    expect(mockedApi.setActiveEnvironment).toHaveBeenCalledWith("row-1");

    resolveMirror(null);
    await pending;
    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe("row-1");
  });

  it("reverts when the Rust authority refuses the switch", async () => {
    mockedApi.setActiveEnvironment.mockRejectedValue(
      new Error("REMOTE_COMMAND_UNAVAILABLE: no paired remote environment")
    );

    await expect(
      useEnvironmentStore.getState().setActiveEnvironment("row-1")
    ).rejects.toThrow("REMOTE_COMMAND_UNAVAILABLE");

    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe(
      LOCAL_ENVIRONMENT_ID
    );
  });

  it("ignores unknown environment ids without touching Rust", async () => {
    await useEnvironmentStore.getState().setActiveEnvironment("ghost");

    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe(
      LOCAL_ENVIRONMENT_ID
    );
    expect(mockedApi.setActiveEnvironment).not.toHaveBeenCalled();
  });

  it("is a no-op when switching to the already-active environment", async () => {
    await useEnvironmentStore
      .getState()
      .setActiveEnvironment(LOCAL_ENVIRONMENT_ID);
    expect(mockedApi.setActiveEnvironment).not.toHaveBeenCalled();
  });
});

describe("registry refresh", () => {
  it("falls back to local when the active environment disappears", async () => {
    useEnvironmentStore.getState().setEnvironments([summary()]);
    mockedApi.setActiveEnvironment.mockResolvedValue(null);
    await useEnvironmentStore.getState().setActiveEnvironment("row-1");

    // The environment was removed backend-side; a refresh no longer lists it.
    useEnvironmentStore.getState().setEnvironments([]);

    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe(
      LOCAL_ENVIRONMENT_ID
    );
  });

  it("prunes connection states of removed environments", () => {
    useEnvironmentStore.getState().setEnvironments([summary()]);
    useEnvironmentStore.getState().setConnectionState("row-1", "connecting");

    useEnvironmentStore.getState().setEnvironments([]);

    expect(
      useEnvironmentStore.getState().connectionStates["row-1"]
    ).toBeUndefined();
    expect(
      useEnvironmentStore.getState().connectionStates[LOCAL_ENVIRONMENT_ID]
    ).toBe("connected");
  });
});

describe("hydrateActiveEnvironment", () => {
  it("adopts the Rust-side authoritative id when it is known", async () => {
    useEnvironmentStore.getState().setEnvironments([summary()]);
    mockedApi.getActiveEnvironment.mockResolvedValue("row-1");

    await useEnvironmentStore.getState().hydrateActiveEnvironment();

    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe("row-1");
  });

  it("loads the registry before deciding an id is unknown", async () => {
    // Hydration racing ahead of loadEnvironments must not clamp a live remote id.
    mockedApi.getActiveEnvironment.mockResolvedValue("row-1");
    mockedApi.list.mockResolvedValue([summary()]);

    await useEnvironmentStore.getState().hydrateActiveEnvironment();

    expect(mockedApi.list).toHaveBeenCalled();
    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe("row-1");
    expect(mockedApi.setActiveEnvironment).not.toHaveBeenCalled();
  });

  it("falls back to local for an unknown authoritative id and tells Rust", async () => {
    mockedApi.getActiveEnvironment.mockResolvedValue("stale-row");
    mockedApi.list.mockResolvedValue([]);
    mockedApi.setActiveEnvironment.mockResolvedValue(null);

    await useEnvironmentStore.getState().hydrateActiveEnvironment();

    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe(
      LOCAL_ENVIRONMENT_ID
    );
    // The mirror clamped, so the Rust authority must clamp with it.
    expect(mockedApi.setActiveEnvironment).toHaveBeenCalledWith(
      LOCAL_ENVIRONMENT_ID
    );
  });
});
