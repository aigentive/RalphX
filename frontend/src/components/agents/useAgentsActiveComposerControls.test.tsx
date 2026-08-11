import { act, renderHook } from "@testing-library/react";
import type { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AGENT_MODEL_CATALOG } from "@/lib/agent-models";

import type { AgentConversation } from "./agentConversations";
import { useAgentsActiveComposerControls } from "./useAgentsActiveComposerControls";

const { toastErrorMock, updateCoordinationModeMock } = vi.hoisted(() => ({
  toastErrorMock: vi.fn(),
  updateCoordinationModeMock: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: toastErrorMock,
  },
}));

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      updateAgentConversationCoordinationMode: updateCoordinationModeMock,
    },
  };
});

type ControlsArgs = Parameters<typeof useAgentsActiveComposerControls>[0];

function projectConversation(
  overrides: Partial<AgentConversation> = {},
): AgentConversation {
  return {
    id: "conversation-1",
    contextType: "project",
    contextId: "project-1",
    projectId: "project-1",
    ideationSessionId: null,
    claudeSessionId: null,
    providerSessionId: null,
    providerHarness: null,
    agentMode: "ideation",
    coordinationMode: "solo",
    title: "Conversation",
    messageCount: 0,
    lastMessageAt: null,
    createdAt: "2026-05-15T00:00:00.000Z",
    updatedAt: "2026-05-15T00:00:00.000Z",
    archivedAt: null,
    ...overrides,
  };
}

function controlsArgs(overrides: Partial<ControlsArgs> = {}): ControlsArgs {
  return {
    activeConversation: projectConversation(),
    activeProjectId: "project-1",
    activeWorkspace: null,
    defaultProjectId: "project-1",
    invalidateProjectConversations: vi.fn(),
    lastRuntimeByProjectId: {},
    modelRegistry: AGENT_MODEL_CATALOG,
    normalizedActiveRuntime: {
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "xhigh",
    },
    projects: [
      {
        id: "project-1",
        name: "RalphX",
        workingDirectory: "/tmp/ralphx",
        gitMode: "worktree",
        baseBranch: "main",
        worktreeParentDirectory: null,
        useFeatureBranches: true,
        mergeValidationMode: "block",
        detectedAnalysis: null,
        customAnalysis: null,
        analyzedAt: null,
        githubPrEnabled: false,
        createdAt: "2026-05-15T00:00:00.000Z",
        updatedAt: "2026-05-15T00:00:00.000Z",
      },
    ],
    queryClient: {
      invalidateQueries: vi.fn(),
      refetchQueries: vi.fn(),
    } as unknown as QueryClient,
    runtimeConversationId: "conversation-1",
    runtimeByConversationId: {},
    selectedConversationId: "conversation-1",
    setComposerRuntimeForConversation: vi.fn(),
    ...overrides,
  };
}

describe("useAgentsActiveComposerControls", () => {
  beforeEach(() => {
    toastErrorMock.mockReset();
    updateCoordinationModeMock.mockReset();
    updateCoordinationModeMock.mockResolvedValue(projectConversation());
  });

  it("refetches the active workspace when the mode menu opens", () => {
    const refetchQueries = vi.fn();
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(
        controlsArgs({
          queryClient: {
            invalidateQueries: vi.fn(),
            refetchQueries,
          } as unknown as QueryClient,
        }),
      ),
    );

    act(() => {
      result.current.handleActiveConversationModeMenuOpen();
    });

    expect(refetchQueries).toHaveBeenCalledWith({
      queryKey: ["agents", "conversation-workspace", "conversation-1"],
      exact: true,
    });
  });

  it("does not refetch mode state without a selected conversation", () => {
    const refetchQueries = vi.fn();
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(
        controlsArgs({
          queryClient: {
            invalidateQueries: vi.fn(),
            refetchQueries,
          } as unknown as QueryClient,
          selectedConversationId: null,
        }),
      ),
    );

    act(() => {
      result.current.handleActiveConversationModeMenuOpen();
    });

    expect(refetchQueries).not.toHaveBeenCalled();
  });

  it("normalizes active model changes against provider-supported efforts", () => {
    const setRuntimeForConversation = vi.fn();
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(
        controlsArgs({
          normalizedActiveRuntime: {
            provider: "claude",
            modelId: "sonnet",
            effort: "medium",
          },
          setComposerRuntimeForConversation: setRuntimeForConversation,
        }),
      ),
    );

    act(() => {
      result.current.handleActiveModelChange("opus", [
        "low",
        "medium",
        "high",
        "max",
      ]);
    });

    expect(setRuntimeForConversation).toHaveBeenCalledWith(
      "conversation-1",
      "project-1",
      {
        provider: "claude",
        modelId: "opus",
        effort: "high",
      },
    );
  });

  it("normalizes active provider changes to a provider default model", () => {
    const setRuntimeForConversation = vi.fn();
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(
        controlsArgs({
          normalizedActiveRuntime: {
            provider: "claude",
            modelId: "opus",
            effort: "high",
          },
          setComposerRuntimeForConversation: setRuntimeForConversation,
        }),
      ),
    );

    let committedRuntime: ReturnType<
      typeof result.current.handleActiveProviderChange
    >;
    act(() => {
      committedRuntime = result.current.handleActiveProviderChange("codex", [
        "low",
        "medium",
        "high",
        "xhigh",
      ]);
    });

    expect(setRuntimeForConversation).toHaveBeenCalledWith(
      "conversation-1",
      "project-1",
      {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "xhigh",
      },
    );
    expect(committedRuntime!).toEqual({
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "xhigh",
    });
  });

  it("normalizes review provider changes through the provider default catalog", () => {
    const setRuntimeForConversation = vi.fn();
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(
        controlsArgs({
          normalizedActiveRuntime: {
            provider: "claude",
            modelId: "haiku",
            effort: "medium",
          },
          runtimeConversationId: "review-conversation-1",
          setComposerRuntimeForConversation: setRuntimeForConversation,
        }),
      ),
    );

    act(() => {
      result.current.handleActiveProviderChange("codex", [
        "low",
        "medium",
        "high",
        "xhigh",
      ]);
    });

    expect(setRuntimeForConversation).toHaveBeenCalledWith(
      "review-conversation-1",
      "project-1",
      {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "xhigh",
      },
    );
  });

  it("does not update provider runtime without a selected conversation", () => {
    const setRuntimeForConversation = vi.fn();
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(
        controlsArgs({
          runtimeConversationId: null,
          selectedConversationId: null,
          setComposerRuntimeForConversation: setRuntimeForConversation,
        }),
      ),
    );

    act(() => {
      result.current.handleActiveProviderChange("codex", [
        "low",
        "medium",
        "high",
        "xhigh",
      ]);
    });

    expect(setRuntimeForConversation).not.toHaveBeenCalled();
  });

  it("commits a private conversation runtime without project-scoped memory", () => {
    const setComposerRuntimeForConversation = vi.fn();
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(
        controlsArgs({
          activeProjectId: null,
          defaultProjectId: null,
          setComposerRuntimeForConversation,
        }),
      ),
    );

    let committedRuntime: ReturnType<
      typeof result.current.handleActiveProviderChange
    >;
    act(() => {
      committedRuntime = result.current.handleActiveProviderChange("claude");
    });

    expect(committedRuntime!).toEqual({
      provider: "claude",
      modelId: "sonnet",
      effort: "medium",
    });
    expect(setComposerRuntimeForConversation).toHaveBeenCalledWith(
      "conversation-1",
      null,
      committedRuntime!,
    );
  });

  it("updates the selected capability for the active project conversation", async () => {
    const invalidateProjectConversations = vi.fn().mockResolvedValue(undefined);
    const invalidateQueries = vi.fn().mockResolvedValue(undefined);
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(
        controlsArgs({
          invalidateProjectConversations,
          queryClient: {
            invalidateQueries,
            refetchQueries: vi.fn(),
          } as unknown as QueryClient,
        }),
      ),
    );

    await act(async () => {
      await result.current.handleActiveCapabilityChange("rx_native_team");
    });

    expect(updateCoordinationModeMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      coordinationMode: "rx_native_team",
    });
    expect(invalidateProjectConversations).toHaveBeenCalledWith("project-1");
    expect(invalidateQueries).toHaveBeenCalled();
    expect(result.current.updatingCapabilityConversationId).toBeNull();
  });

  it("passes the selected Codex model when enabling Ultra", async () => {
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(controlsArgs()),
    );

    await act(async () => {
      await result.current.handleActiveCapabilityChange("codex_native_ultra");
    });

    expect(updateCoordinationModeMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      coordinationMode: "codex_native_ultra",
      modelOverride: "gpt-5.5",
    });
  });

  it("does not update a capability when the requested state already matches", async () => {
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(
        controlsArgs({
          activeConversation: projectConversation({
            coordinationMode: "rx_native_team",
          }),
        }),
      ),
    );

    await act(async () => {
      await result.current.handleActiveCapabilityChange("rx_native_team");
    });

    expect(updateCoordinationModeMock).not.toHaveBeenCalled();
  });

  it("does not update a capability without an active project conversation", async () => {
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(
        controlsArgs({
          activeConversation: null,
          selectedConversationId: null,
        }),
      ),
    );

    await act(async () => {
      await result.current.handleActiveCapabilityChange("rx_native_team");
    });

    expect(updateCoordinationModeMock).not.toHaveBeenCalled();
  });

  it("clears capability pending state and reports update failures", async () => {
    updateCoordinationModeMock.mockRejectedValue(new Error("Team update failed"));
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(controlsArgs()),
    );

    await act(async () => {
      await result.current.handleActiveCapabilityChange("rx_native_team");
    });

    expect(updateCoordinationModeMock).toHaveBeenCalledWith({
      conversationId: "conversation-1",
      coordinationMode: "rx_native_team",
    });
    expect(toastErrorMock).toHaveBeenCalledWith("Team update failed");
    expect(result.current.updatingCapabilityConversationId).toBeNull();
  });

  it("normalizes active effort changes against provider-supported efforts", () => {
    const setRuntimeForConversation = vi.fn();
    const { result } = renderHook(() =>
      useAgentsActiveComposerControls(
        controlsArgs({
          normalizedActiveRuntime: {
            provider: "claude",
            modelId: "opus",
            effort: "high",
          },
          setComposerRuntimeForConversation: setRuntimeForConversation,
        }),
      ),
    );

    act(() => {
      result.current.handleActiveEffortChange("xhigh", [
        "low",
        "medium",
        "high",
        "max",
      ]);
    });

    expect(setRuntimeForConversation).toHaveBeenCalledWith(
      "conversation-1",
      "project-1",
      {
        provider: "claude",
        modelId: "opus",
        effort: "high",
      },
    );
  });
});
