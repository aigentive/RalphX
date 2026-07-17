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

function createHarness() {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
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

    act(() => result.current.clearDefault("workspace_project"));
    await waitFor(() => {
      expect(manualRoleDefaultsApi.clear).toHaveBeenCalledWith({
        projectId: "project-1",
        role: "workspace_project",
      });
      expect(invalidate).toHaveBeenCalledTimes(2);
    });
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
