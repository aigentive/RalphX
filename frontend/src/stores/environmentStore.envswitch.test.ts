import { beforeEach, describe, expect, it, vi } from "vitest";

import { remoteEnvironmentsApi } from "@/api/remote-environments";
import { useAgentTerminalStore } from "@/components/agents/agentTerminalStore";
import { resetTransportEnvironmentId } from "@/lib/remote/active-environment";
import type { RemoteEnvironmentSummary } from "@/api/remote-environments";
import type { Task } from "@/types/task";
import { useProjectStore } from "./projectStore";
import { useTaskStore } from "./taskStore";
import { useUiStore } from "./uiStore";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "./environmentStore";

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
const remote: RemoteEnvironmentSummary = {
  id: "env-b",
  environmentId: "host-b",
  name: "Host B",
  baseUrl: "https://host-b.test",
  candidateUrls: [],
  scopes: ["ui:read", "ui:operate"],
  protocolVersion: 1,
  status: "active",
  createdAt: "2026-07-28T00:00:00Z",
  lastConnectedAt: null,
};
const localTask: Task = {
  id: "task-local",
  projectId: "proj-local",
  category: "feature",
  title: "Local task",
  description: null,
  priority: 0,
  internalStatus: "backlog",
  createdAt: "2026-07-28T00:00:00Z",
  updatedAt: "2026-07-28T00:00:00Z",
  startedAt: null,
  completedAt: null,
};

function resetAll(): void {
  localStorage.clear();
  resetTransportEnvironmentId();
  useEnvironmentStore.setState({
    activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: remote.id, name: remote.name, kind: "remote", remote },
    ],
    connectionStates: { [LOCAL_ENVIRONMENT_ID]: "connected" },
  });
  useProjectStore.setState(useProjectStore.getInitialState(), true);
  useTaskStore.setState(useTaskStore.getInitialState(), true);
  useAgentTerminalStore.setState(useAgentTerminalStore.getInitialState(), true);
  useUiStore.setState(useUiStore.getInitialState(), true);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockedApi.setActiveEnvironment.mockResolvedValue(null);
  resetAll();
});

describe("P-13 environment store isolation", () => {
  it("round-trips A to B to A with bidirectional absence", async () => {
    useProjectStore.getState().selectProject("proj-local");
    useTaskStore.getState().setTasks([localTask]);
    useAgentTerminalStore.getState().setOpen("conversation-local", true);

    await useEnvironmentStore.getState().setActiveEnvironment("env-b");
    expect(useProjectStore.getState().activeProjectId).toBeNull();
    expect(useTaskStore.getState().tasks).toEqual({});
    expect(useAgentTerminalStore.getState().openByConversationId).toEqual({});
    expect(localStorage.getItem("ralphx-project-store:env-b") ?? "").not.toContain("proj-local");
    expect(localStorage.getItem("ralphx-project-store")).toContain("proj-local");

    useProjectStore.getState().selectProject("proj-b");
    await useEnvironmentStore.getState().setActiveEnvironment(LOCAL_ENVIRONMENT_ID);
    expect(useProjectStore.getState().activeProjectId).toBe("proj-local");
    expect(JSON.stringify(useProjectStore.getState())).not.toContain("proj-b");
    expect(localStorage.getItem("ralphx-project-store")).not.toContain("proj-b");

    await useEnvironmentStore.getState().setActiveEnvironment("env-b");
    expect(useProjectStore.getState().activeProjectId).toBe("proj-b");
    for (let index = 0; index < localStorage.length; index += 1) {
      const key = localStorage.key(index);
      if (key?.startsWith("ralphx-") && key.endsWith(":env-b")) {
        expect(localStorage.getItem(key)).not.toMatch(/proj-local|task-local|conversation-local/);
      }
    }
  });

  it("rehydrates the previous environment after a rejected optimistic switch", async () => {
    useProjectStore.getState().selectProject("proj-local");
    mockedApi.setActiveEnvironment.mockRejectedValue(new Error("refused"));
    await expect(useEnvironmentStore.getState().setActiveEnvironment("env-b")).rejects.toThrow("refused");
    expect(useEnvironmentStore.getState().activeEnvironmentId).toBe(LOCAL_ENVIRONMENT_ID);
    expect(useProjectStore.getState().activeProjectId).toBe("proj-local");
  });

  it("isolates synchronously before the backend promise settles", async () => {
    let resolve: () => void = () => {};
    mockedApi.setActiveEnvironment.mockImplementation(
      () =>
        new Promise<null>((done) => {
          resolve = () => done(null);
        }),
    );
    useProjectStore.getState().selectProject("proj-local");
    const pending = useEnvironmentStore.getState().setActiveEnvironment("env-b");
    expect(useProjectStore.getState().activeProjectId).toBeNull();
    resolve();
    await pending;
  });
  /**
   * P-12's proof obligation is an ORDER, not a state snapshot: the DOM-visible state
   * change must precede the first proxy call. Asserting both after the fact establishes
   * nothing — an implementation that awaited the invoke first would pass. This records
   * both events into one ordered log.
   */
  it("writes the visible state before dispatching the first proxy call", async () => {
    const order: string[] = [];
    const unsubscribe = useEnvironmentStore.subscribe((state, previous) => {
      if (state.activeEnvironmentId !== previous.activeEnvironmentId) {
        order.push(`state:${state.activeEnvironmentId}`);
      }
    });
    mockedApi.setActiveEnvironment.mockImplementation(async () => {
      order.push("invoke");
      return null;
    });

    try {
      await useEnvironmentStore.getState().setActiveEnvironment("env-b");
    } finally {
      unsubscribe();
    }

    expect(order).toEqual(["state:env-b", "invoke"]);
  });

  it("does no storage work for a no-op switch", async () => {
    const getSpy = vi.spyOn(Storage.prototype, "getItem");
    const setSpy = vi.spyOn(Storage.prototype, "setItem");
    getSpy.mockClear();
    setSpy.mockClear();
    await useEnvironmentStore.getState().setActiveEnvironment(LOCAL_ENVIRONMENT_ID);
    expect(getSpy).not.toHaveBeenCalled();
    expect(setSpy).not.toHaveBeenCalled();
    getSpy.mockRestore();
    setSpy.mockRestore();
  });

  it("preserves client feature flags while resetting environment-owned UI state", async () => {
    const flags = {
      ...useUiStore.getState().featureFlags,
      remoteEnvironments: !useUiStore.getState().featureFlags.remoteEnvironments,
    };
    useUiStore.getState().setFeatureFlags(flags);
    useUiStore.getState().setBoardSearchQuery("host-specific query");

    await useEnvironmentStore.getState().setActiveEnvironment("env-b");

    expect(useUiStore.getState().featureFlags).toEqual(flags);
    expect(useUiStore.getState().boardSearchQuery).toBeNull();
  });
});
