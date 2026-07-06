import { beforeEach, describe, expect, it } from "vitest";

import type { AgentConversationRuntimeStatus } from "@/api/chat";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { useChatStore } from "@/stores/chatStore";

import { reconcileAgentConversationRuntimeStatus } from "./agentConversationRuntimeStore";

function runtimeStatus(
  overrides: Partial<AgentConversationRuntimeStatus> = {},
): AgentConversationRuntimeStatus {
  return {
    conversationId: "conversation-1",
    isRunning: true,
    agentStatus: "generating",
    primarySource: "task_execution",
    summaryLabel: "Executing task",
    items: [
      {
        source: "task_execution",
        contextType: "task_execution",
        contextId: "task-1",
        label: "Executing",
        title: "Task execution",
        agentStatus: "generating",
        taskId: "task-1",
        internalStatus: "executing",
        runningProcess: null,
        ideationSession: null,
        parentSessionId: null,
        childSessionId: null,
        conversationId: "task-conversation-1",
      },
    ],
    ...overrides,
  };
}

describe("reconcileAgentConversationRuntimeStatus", () => {
  beforeEach(() => {
    useChatStore.setState({
      activeConversationIds: {},
      activeAgentRunIds: {},
      agentStatus: {},
      agentActivityLabels: {},
      isSending: {},
    });
  });

  it("can consume child-only aggregate runtime status without mirroring to the visible chat key", () => {
    const storeKey = buildStoreKey("project", "conversation-1");

    reconcileAgentConversationRuntimeStatus("conversation-1", runtimeStatus(), {
      mirrorToVisibleChatStatus: false,
      storeKey,
    });

    const state = useChatStore.getState();
    expect(state.agentStatus[storeKey]).toBeUndefined();
    expect(state.agentActivityLabels[storeKey]).toBeUndefined();
    expect(state.activeConversationIds[storeKey]).toBeUndefined();
  });

  it("mirrors only the selected visible chat status from mixed aggregate runtime rows", () => {
    const storeKey = buildStoreKey("project", "conversation-1");
    const childItem = runtimeStatus().items[0]!;
    const workspaceItem = {
      ...childItem,
      source: "workspace" as const,
      contextType: "project" as const,
      contextId: "conversation-1",
      label: "Waiting",
      title: "Workspace chat",
      agentStatus: "waiting_for_input" as const,
      taskId: null,
      internalStatus: null,
      conversationId: "conversation-1",
    };

    reconcileAgentConversationRuntimeStatus(
      "conversation-1",
      runtimeStatus({
        primarySource: "task_execution",
        summaryLabel: "Executing task",
        agentStatus: "generating",
        items: [workspaceItem, childItem],
      }),
      {
        storeKey,
        selectVisibleChatStatus: (status) =>
          status
            ? {
                ...status,
                primarySource: "workspace",
                summaryLabel: "Waiting",
                agentStatus: "waiting_for_input",
                items: [workspaceItem],
              }
            : status,
      },
    );

    const state = useChatStore.getState();
    expect(state.agentStatus[storeKey]).toBe("waiting_for_input");
    expect(state.agentActivityLabels[storeKey]).toBeUndefined();
  });

  it("clears stale visible chat status when child-only status stops mirroring", () => {
    const storeKey = buildStoreKey("project", "conversation-1");
    const workspaceItem = runtimeStatus().items[0]!;
    const mirrorWorkspaceRuntimeOnly = (
      status: AgentConversationRuntimeStatus | null | undefined,
    ) => status?.items.some((item) => item.source === "workspace") ?? false;

    reconcileAgentConversationRuntimeStatus(
      "conversation-1",
      runtimeStatus({
        primarySource: "workspace",
        summaryLabel: "Agent running",
        items: [
          {
            ...workspaceItem,
            source: "workspace",
            contextType: "project",
            contextId: "conversation-1",
            label: "Running",
            title: "Workspace chat",
            taskId: null,
            internalStatus: null,
            conversationId: "conversation-1",
          },
        ],
      }),
      { storeKey },
    );
    expect(useChatStore.getState().agentStatus[storeKey]).toBe("generating");

    reconcileAgentConversationRuntimeStatus("conversation-1", runtimeStatus(), {
      mirrorToVisibleChatStatus: mirrorWorkspaceRuntimeOnly,
      storeKey,
    });

    const state = useChatStore.getState();
    expect(state.agentStatus[storeKey]).toBeUndefined();
    expect(state.agentActivityLabels[storeKey]).toBeUndefined();
  });

  it("does not clear visible chat status for boolean opt-out consumers", () => {
    const storeKey = buildStoreKey("project", "conversation-1");
    useChatStore.getState().setAgentStatus(storeKey, "generating");
    useChatStore.getState().setAgentActivityLabel(storeKey, "running");

    reconcileAgentConversationRuntimeStatus("conversation-1", runtimeStatus(), {
      mirrorToVisibleChatStatus: false,
      storeKey,
    });

    const state = useChatStore.getState();
    expect(state.agentStatus[storeKey]).toBe("generating");
    expect(state.agentActivityLabels[storeKey]).toBe("running");
  });

  it("keeps mirroring enabled by default for global runtime consumers", () => {
    const storeKey = buildStoreKey("project", "conversation-1");

    reconcileAgentConversationRuntimeStatus("conversation-1", runtimeStatus(), {
      storeKey,
    });

    const state = useChatStore.getState();
    expect(state.agentStatus[storeKey]).toBe("generating");
    expect(state.activeConversationIds[storeKey]).toBe("conversation-1");
  });
});
