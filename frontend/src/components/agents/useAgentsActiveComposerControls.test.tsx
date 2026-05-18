import { act, renderHook } from "@testing-library/react";
import type { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import { AGENT_MODEL_CATALOG } from "@/lib/agent-models";

import type { AgentConversation } from "./agentConversations";
import { useAgentsActiveComposerControls } from "./useAgentsActiveComposerControls";

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
    runtimeByConversationId: {},
    selectedConversationId: "conversation-1",
    setRuntimeForConversation: vi.fn(),
    ...overrides,
  };
}

describe("useAgentsActiveComposerControls", () => {
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
          setRuntimeForConversation,
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
          setRuntimeForConversation,
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
