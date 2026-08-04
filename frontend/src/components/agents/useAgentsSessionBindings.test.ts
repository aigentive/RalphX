import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";

import { useAgentsSessionBindings } from "./useAgentsSessionBindings";

describe("useAgentsSessionBindings", () => {
  beforeEach(() => {
    useAgentSessionStore.setState({
      focusedProjectId: null,
      selectedProjectId: null,
      selectedConversationId: null,
      lastSelectedConversationByProjectId: {},
      expandedProjectIds: {},
    });
    useProjectStore.setState({
      activeProjectId: "project-b",
      projects: {},
    });
    useUiStore.setState({ taskHistoryState: null });
  });

  it("sets the RX active project when selecting an Agents conversation", () => {
    const setOptimisticSelectedConversationId = vi.fn();
    const { result } = renderHook(() =>
      useAgentsSessionBindings({ setOptimisticSelectedConversationId })
    );

    act(() => {
      result.current.selectConversation("project-a", "conversation-a");
    });

    expect(useProjectStore.getState().activeProjectId).toBe("project-a");
    expect(useAgentSessionStore.getState().selectedProjectId).toBe("project-a");
    expect(useAgentSessionStore.getState().selectedConversationId).toBe(
      "conversation-a"
    );
  });

  it("clears stale task history when selecting a different Agents conversation", () => {
    useAgentSessionStore.setState({
      selectedProjectId: "project-a",
      selectedConversationId: "conversation-old",
    });
    useUiStore.setState({
      taskHistoryState: {
        status: "executing" as const,
        timestamp: "2026-07-07T15:00:00.000Z",
        conversationId: "stale-task-runtime-conversation",
      },
    });
    const setOptimisticSelectedConversationId = vi.fn();
    const { result } = renderHook(() =>
      useAgentsSessionBindings({ setOptimisticSelectedConversationId })
    );

    act(() => {
      result.current.selectConversation("project-a", "conversation-new");
    });

    expect(useUiStore.getState().taskHistoryState).toBeNull();
  });

  it("keeps task history when selecting the same Agents conversation", () => {
    const historyState = {
      status: "executing" as const,
      timestamp: "2026-07-07T15:00:00.000Z",
      conversationId: "task-runtime-conversation",
    };
    useAgentSessionStore.setState({
      selectedProjectId: "project-a",
      selectedConversationId: "conversation-a",
    });
    useUiStore.setState({ taskHistoryState: historyState });
    const setOptimisticSelectedConversationId = vi.fn();
    const { result } = renderHook(() =>
      useAgentsSessionBindings({ setOptimisticSelectedConversationId })
    );

    act(() => {
      result.current.selectConversation("project-a", "conversation-a");
    });

    expect(useUiStore.getState().taskHistoryState).toEqual(historyState);
  });

  it("clears stale task history when returning to the starter composer", () => {
    useUiStore.setState({
      taskHistoryState: {
        status: "executing" as const,
        timestamp: "2026-07-07T15:00:00.000Z",
        conversationId: "stale-task-runtime-conversation",
      },
    });
    const setOptimisticSelectedConversationId = vi.fn();
    const { result } = renderHook(() =>
      useAgentsSessionBindings({ setOptimisticSelectedConversationId })
    );

    act(() => {
      result.current.clearAgentConversationSelection();
    });

    expect(useUiStore.getState().taskHistoryState).toBeNull();
  });

  it("exposes transient composer runtime state through the session binding seam", () => {
    const { result } = renderHook(() =>
      useAgentsSessionBindings({ setOptimisticSelectedConversationId: vi.fn() })
    );

    act(() => {
      result.current.setComposerRuntimeForConversation(
        "conversation-a",
        null,
        { provider: "claude", modelId: "sonnet", effort: "high" },
      );
    });

    expect(result.current.composerRuntimeOverridesByConversationId).toEqual({
      "conversation-a": {
        provider: "claude",
        modelId: "sonnet",
        effort: "high",
      },
    });
    expect(useAgentSessionStore.getState().lastRuntimeByProjectId).toEqual({});
  });
});
