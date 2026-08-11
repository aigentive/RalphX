import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { ChatSessionToolbar } from "./ChatSessionToolbar";
import type { ConversationStatsResponse } from "@/api/chat";

const mockUseConversationStats = vi.fn();
const mockUseFeatureFlags = vi.fn();

vi.mock("@/hooks/useConversationStats", () => ({
  useConversationStats: (...args: unknown[]) => mockUseConversationStats(...args),
}));

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: () => mockUseFeatureFlags(),
}));

function makeStats(
  overrides: Partial<ConversationStatsResponse> = {},
): ConversationStatsResponse {
  return {
    conversationId: "conv-1",
    contextType: "ideation",
    contextId: "session-1",
    providerHarness: "codex",
    upstreamProvider: "openai",
    providerProfile: null,
    messageUsageTotals: {
      inputTokens: 200,
      outputTokens: 40,
      cacheCreationTokens: 10,
      cacheReadTokens: 20,
      processedTokens: 240,
      estimatedUsd: null,
    },
    runUsageTotals: {
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      processedTokens: null,
      estimatedUsd: null,
    },
    effectiveUsageTotals: {
      inputTokens: 200,
      outputTokens: 40,
      cacheCreationTokens: 10,
      cacheReadTokens: 20,
      processedTokens: 240,
      estimatedUsd: null,
    },
    usageCoverage: {
      providerMessageCount: 1,
      providerMessagesWithUsage: 1,
      runCount: 0,
      runsWithUsage: 0,
      effectiveRunConversationCount: 0,
      effectiveMessageConversationCount: 1,
      legacyEstimatedSampleCount: 0,
      fallbackEstimatedSampleCount: 0,
      uncountedSampleCount: 0,
      effectiveTotalsSource: "messages",
    },
    attributionCoverage: {
      providerMessageCount: 1,
      providerMessagesWithAttribution: 1,
      runCount: 0,
      runsWithAttribution: 0,
    },
    byHarness: [],
    byUpstreamProvider: [{ key: "openai", count: 1, usage: {
      inputTokens: 200,
      outputTokens: 40,
      cacheCreationTokens: 10,
      cacheReadTokens: 20,
      processedTokens: 240,
      estimatedUsd: null,
    } }],
    byModel: [{ key: "gpt-5.4", count: 1, usage: {
      inputTokens: 200,
      outputTokens: 40,
      cacheCreationTokens: 10,
      cacheReadTokens: 20,
      processedTokens: 240,
      estimatedUsd: null,
    } }],
    byEffort: [{ key: "xhigh", count: 1, usage: {
      inputTokens: 200,
      outputTokens: 40,
      cacheCreationTokens: 10,
      cacheReadTokens: 20,
      processedTokens: 240,
      estimatedUsd: null,
    } }],
    ...overrides,
  };
}

describe("ChatSessionToolbar", () => {
  beforeEach(() => {
    mockUseConversationStats.mockReset();
    mockUseFeatureFlags.mockReset();
    mockUseFeatureFlags.mockReturnValue({
      data: { activityPage: false },
    });
  });

  it("hides the toolbar when there is no provider context, stats, or live status to show", () => {
    mockUseConversationStats.mockReturnValue({
      data: null,
      isLoading: false,
    });

    const { container } = render(
      <ChatSessionToolbar
        isAgentActive={false}
        agentType="agent"
        contextType="ideation"
        contextId="session-1"
        conversationId="conv-1"
        fallbackConversation={null}
        fallbackMessages={null}
      />,
    );

    expect(screen.queryByTestId("chat-session-toolbar-row")).not.toBeInTheDocument();
    expect(container.firstChild).toBeNull();
  });

  it("does not reserve an empty status slot for historical activity when the activity page flag is disabled", () => {
    mockUseConversationStats.mockReturnValue({
      data: null,
      isLoading: false,
    });
    mockUseFeatureFlags.mockReturnValue({
      data: { activityPage: false },
    });

    const { container } = render(
      <ChatSessionToolbar
        isAgentActive={false}
        agentType="agent"
        contextType="ideation"
        contextId="session-1"
        conversationId="conv-1"
        hasActivity={true}
        fallbackConversation={null}
        fallbackMessages={null}
      />,
    );

    expect(screen.queryByTestId("chat-session-toolbar-row")).not.toBeInTheDocument();
    expect(container.firstChild).toBeNull();
  });

  it("keeps provider context and live status on the same row", () => {
    mockUseConversationStats.mockReturnValue({
      data: makeStats(),
      isLoading: false,
    });

    render(
      <ChatSessionToolbar
        isAgentActive={true}
        agentType="agent"
        contextType="ideation"
        contextId="session-1"
        conversationId="conv-1"
        providerHarness="codex"
        providerSessionId="thread-1"
        upstreamProvider="openai"
        fallbackConversation={null}
        fallbackMessages={null}
        modelDisplay={{ id: "gpt-5.4", label: "gpt-5.4" }}
        agentStatus="generating"
      />,
    );

    const row = screen.getByTestId("chat-session-toolbar-row");
    expect(row).toContainElement(screen.getByTestId("chat-session-provider-context"));
    expect(row).toContainElement(screen.getByTestId("chat-session-status-inline"));
    expect(screen.getByText("Agent responding...")).toBeInTheDocument();
  });

  it("builds a synthetic fallback conversation when metadata has not hydrated yet", () => {
    mockUseConversationStats.mockReturnValue({
      data: null,
      isLoading: false,
    });

    render(
      <ChatSessionToolbar
        isAgentActive={false}
        agentType="agent"
        contextType="ideation"
        contextId="session-1"
        conversationId="conv-1"
        providerHarness="codex"
        providerSessionId="thread-1"
        upstreamProvider="openai"
        providerProfile="default"
        fallbackConversation={null}
        fallbackMessages={[]}
      />,
    );

    expect(mockUseConversationStats).toHaveBeenCalledWith("conv-1", {
      fallbackConversation: expect.objectContaining({
        id: "conv-1",
        contextType: "ideation",
        contextId: "session-1",
        providerHarness: "codex",
        providerSessionId: "thread-1",
        upstreamProvider: "openai",
        providerProfile: "default",
      }),
      fallbackMessages: [],
    });
  });
});
