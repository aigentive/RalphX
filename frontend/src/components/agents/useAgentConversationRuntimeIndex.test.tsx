import type { PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  chatApi,
  type AgentConversationRuntimeIndexResponse,
  type AgentConversationRuntimeIndexRow,
} from "@/api/chat";

import {
  runtimeIndexToConversationStatus,
  useAgentConversationRuntimeIndex,
} from "./useAgentConversationRuntimeIndex";

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      getAgentConversationRuntimeIndex: vi.fn(),
    },
  };
});

function row(
  overrides: Partial<AgentConversationRuntimeIndexRow> = {},
): AgentConversationRuntimeIndexRow {
  return {
    id: "runtime-1",
    group: "main",
    kind: "workspace",
    lifecycle: "running",
    statusLabel: "Agent working",
    title: "Agent",
    mode: "chat",
    orderIndex: 0,
    orderStartedAt: null,
    completedAt: null,
    conversationId: "conversation-1",
    contextType: "project",
    contextId: "conversation-1",
    taskId: null,
    agentRunId: null,
    parentSessionId: null,
    childSessionId: null,
    providerHarness: null,
    providerSessionId: null,
    errorMessage: null,
    ...overrides,
  };
}

function index(
  rows: AgentConversationRuntimeIndexRow[],
): AgentConversationRuntimeIndexResponse {
  return { conversationId: "conversation-1", rows };
}

describe("runtimeIndexToConversationStatus", () => {
  it.each([
    ["main-active", row(), "generating", "workspace"],
    [
      "verification-only-active",
      row({ group: "ideation_verification", kind: "verification" }),
      "generating",
      "verification",
    ],
    [
      "pipeline-only-active",
      row({ group: "pipeline", kind: "task" }),
      "generating",
      "task_execution",
    ],
    [
      "waiting-main",
      row({ lifecycle: "waiting" }),
      "waiting_for_input",
      "workspace",
    ],
    [
      "workspace-review-only-active",
      row({ group: "ideation_verification", kind: "workspace_review" }),
      "generating",
      "workspace_review",
    ],
  ])("converts %s", (_name, runtimeRow, agentStatus, primarySource) => {
    expect(runtimeIndexToConversationStatus(index([runtimeRow]))).toMatchObject({
      isRunning: true,
      agentStatus,
      primarySource,
    });
  });

  it("reports idle when no row is active", () => {
    expect(
      runtimeIndexToConversationStatus(index([row({ lifecycle: "completed" })])),
    ).toMatchObject({ isRunning: false, agentStatus: "generating" });
  });
});

describe("useAgentConversationRuntimeIndex", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("sustains polling while only a pipeline row is active", async () => {
    vi.useFakeTimers();
    vi.mocked(chatApi.getAgentConversationRuntimeIndex).mockResolvedValue(
      index([row({ group: "pipeline", kind: "task" })]),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    renderHook(
      () => useAgentConversationRuntimeIndex("conversation-1"),
      { wrapper },
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_100);
    });

    expect(chatApi.getAgentConversationRuntimeIndex).toHaveBeenCalledTimes(2);
    queryClient.clear();
  });
});
