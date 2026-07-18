import { createElement, type ReactNode } from "react";
import {
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { manualRoleDefaultsApi } from "@/api/manual-role-defaults";
import type { ManualRoleDefault } from "@/api/manual-role-defaults.types";

import {
  manualRoleDefaultKeys,
  useConversationRoleDefault,
  useManualRoleDefaults,
  useStartComposerRoleDefault,
} from "./useManualRoleDefaults";

vi.mock("@/api/manual-role-defaults", () => ({
  manualRoleDefaultsApi: {
    list: vi.fn(),
    update: vi.fn(),
    clear: vi.fn(),
    getStartComposerDefault: vi.fn(),
    getConversationDefault: vi.fn(),
  },
}));

const value: ManualRoleDefault = {
  provider: "codex",
  model: "gpt-5.6",
  effort: "xhigh",
  serviceTier: "fast",
  coordinationMode: "solo",
  personaId: null,
  approvalPolicy: "never",
  sandboxMode: "workspace-write",
};

function createHarness(usePreviousData = false) {
  const client = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        gcTime: 0,
        ...(usePreviousData && {
          placeholderData: (previousData: unknown) => previousData,
        }),
      },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
  return { client, wrapper };
}

describe("useManualRoleDefaults", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(manualRoleDefaultsApi.list).mockResolvedValue({
      projectId: "project-1",
      roles: [],
    });
    vi.mocked(manualRoleDefaultsApi.update).mockResolvedValue(value);
    vi.mocked(manualRoleDefaultsApi.clear).mockResolvedValue(true);
    vi.mocked(manualRoleDefaultsApi.getStartComposerDefault).mockResolvedValue({
      role: "workspace_project",
      source: "project_ui",
      value,
    });
    vi.mocked(manualRoleDefaultsApi.getConversationDefault).mockResolvedValue({
      role: "workspace_project",
      source: "project_ui",
      value,
    });
  });

  it("loads scoped defaults and invalidates the role catalog after writes", async () => {
    const { client, wrapper } = createHarness();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const { result } = renderHook(
      () => useManualRoleDefaults("project-1"),
      { wrapper },
    );

    await waitFor(() => expect(result.current.catalog).toEqual({
      projectId: "project-1",
      roles: [],
    }));

    act(() => result.current.updateDefault("workspace_project", value));
    await waitFor(() => {
      expect(manualRoleDefaultsApi.update).toHaveBeenCalledWith({
        projectId: "project-1",
        role: "workspace_project",
        value,
      });
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: manualRoleDefaultKeys.all,
      });
    });

    act(() => void result.current.clearDefaultAsync("workspace_project"));
    await waitFor(() => {
      expect(manualRoleDefaultsApi.clear).toHaveBeenCalledWith({
        projectId: "project-1",
        role: "workspace_project",
      });
      expect(invalidate).toHaveBeenCalledTimes(2);
    });
  });

  it("does not expose Project A catalog as placeholder data while Project B loads", async () => {
    let resolveProjectB: ((value: { projectId: string; roles: [] }) => void) | undefined;
    vi.mocked(manualRoleDefaultsApi.list).mockImplementation((projectId) => {
      if (projectId === "project-a") {
        return Promise.resolve({ projectId: "project-a", roles: [] });
      }
      return new Promise((resolve) => {
        resolveProjectB = resolve;
      });
    });
    const { wrapper } = createHarness(true);
    const { result, rerender } = renderHook(
      ({ projectId }) => useManualRoleDefaults(projectId),
      { initialProps: { projectId: "project-a" }, wrapper },
    );

    await waitFor(() => expect(result.current.catalog?.projectId).toBe("project-a"));
    rerender({ projectId: "project-b" });

    expect(result.current.catalog).toBeNull();
    expect(result.current.isLoading).toBe(true);

    act(() => resolveProjectB?.({ projectId: "project-b", roles: [] }));
    await waitFor(() => expect(result.current.catalog?.projectId).toBe("project-b"));
  });

  it("keeps async clear pending through active-scope refetch", async () => {
    let resolveRefetch: (() => void) | undefined;
    vi.mocked(manualRoleDefaultsApi.list)
      .mockResolvedValueOnce({ projectId: "project-1", roles: [] })
      .mockImplementationOnce(
        () => new Promise((resolve) => {
          resolveRefetch = () => resolve({ projectId: "project-1", roles: [] });
        }),
      );
    const { wrapper } = createHarness();
    const { result } = renderHook(() => useManualRoleDefaults("project-1"), { wrapper });
    await waitFor(() => expect(result.current.catalog).not.toBeNull());

    let settled = false;
    act(() => {
      void result.current.clearDefaultAsync("workspace_project").then(() => {
        settled = true;
      });
    });
    await waitFor(() => expect(manualRoleDefaultsApi.clear).toHaveBeenCalledOnce());
    expect(result.current.isSaving).toBe(true);
    expect(settled).toBe(false);

    act(() => resolveRefetch?.());
    await waitFor(() => expect(settled).toBe(true));
    expect(result.current.isSaving).toBe(false);
  });

  it("exposes and dismisses update and clear failures without replacing catalog state", async () => {
    vi.mocked(manualRoleDefaultsApi.update).mockRejectedValueOnce(new Error("Update failed"));
    vi.mocked(manualRoleDefaultsApi.clear).mockRejectedValueOnce(new Error("Clear failed"));
    const { wrapper } = createHarness();
    const { result } = renderHook(() => useManualRoleDefaults("project-1"), { wrapper });
    await waitFor(() => expect(result.current.catalog).not.toBeNull());

    act(() => result.current.updateDefault("workspace_project", value));
    await waitFor(() => expect(result.current.saveError).toHaveProperty("message", "Update failed"));
    expect(result.current.catalog).toEqual({ projectId: "project-1", roles: [] });
    act(() => result.current.dismissSaveError());
    await waitFor(() => expect(result.current.saveError).toBeNull());

    await expect(
      result.current.clearDefaultAsync("workspace_project"),
    ).rejects.toThrow("Clear failed");
    await waitFor(() => expect(result.current.saveError).toHaveProperty("message", "Clear failed"));
    expect(result.current.catalog).toEqual({ projectId: "project-1", roles: [] });
  });

  it("does not leak pending or failed mutations across project scopes", async () => {
    let rejectProjectA: ((error: Error) => void) | undefined;
    vi.mocked(manualRoleDefaultsApi.update).mockImplementationOnce(
      () => new Promise((_resolve, reject) => {
        rejectProjectA = reject;
      }),
    );
    const { wrapper } = createHarness();
    const { result, rerender } = renderHook(
      ({ projectId }) => useManualRoleDefaults(projectId),
      { initialProps: { projectId: "project-a" }, wrapper },
    );
    await waitFor(() => expect(result.current.catalog).not.toBeNull());

    act(() => result.current.updateDefault("workspace_project", value));
    await waitFor(() => expect(result.current.isSaving).toBe(true));

    rerender({ projectId: "project-b" });
    expect(result.current.isSaving).toBe(false);
    expect(result.current.saveError).toBeNull();

    act(() => rejectProjectA?.(new Error("Project A failed")));
    await waitFor(() => expect(result.current.isSaving).toBe(false));
    expect(result.current.saveError).toBeNull();

    rerender({ projectId: "project-a" });
    expect(result.current.saveError).toBeNull();
  });

  it("loads composer defaults and leaves conversation queries idle without an id", async () => {
    const { wrapper } = createHarness();
    const composer = renderHook(
      () => useStartComposerRoleDefault("project-1", "edit"),
      { wrapper },
    );
    const conversation = renderHook(
      () => useConversationRoleDefault(null),
      { wrapper },
    );

    await waitFor(() => expect(composer.result.current.isSuccess).toBe(true));
    expect(manualRoleDefaultsApi.getStartComposerDefault).toHaveBeenCalledWith({
      projectId: "project-1",
      mode: "edit",
    });
    expect(conversation.result.current.fetchStatus).toBe("idle");
    expect(manualRoleDefaultsApi.getConversationDefault).not.toHaveBeenCalled();
  });

  it("loads a conversation role default when an id is available", async () => {
    const { wrapper } = createHarness();
    const { result } = renderHook(
      () => useConversationRoleDefault("conversation-1"),
      { wrapper },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(manualRoleDefaultsApi.getConversationDefault).toHaveBeenCalledWith({
      conversationId: "conversation-1",
    });
  });
});
