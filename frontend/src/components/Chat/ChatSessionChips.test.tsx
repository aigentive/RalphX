import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ConversationStatsResponse } from "@/api/chat";

import { ChatSessionChips } from "./ChatSessionChips";

const mockUseConversationStats = vi.fn();

vi.mock("@/hooks/useConversationStats", () => ({
  useConversationStats: (...args: unknown[]) => mockUseConversationStats(...args),
}));

function statsFixture(
  overrides: Partial<ConversationStatsResponse> = {},
): ConversationStatsResponse {
  return {
    conversationId: "conversation-1",
    contextType: "project",
    contextId: "project-1",
    providerHarness: "codex",
    upstreamProvider: "openai",
    providerProfile: null,
    messageUsageTotals: {
      inputTokens: 120,
      outputTokens: 40,
      cacheCreationTokens: 5,
      cacheReadTokens: 8,
      processedTokens: 160,
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
      inputTokens: 120,
      outputTokens: 40,
      cacheCreationTokens: 5,
      cacheReadTokens: 8,
      processedTokens: 160,
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
    byUpstreamProvider: [],
    byModel: [
      {
        key: "gpt-5.4",
        count: 1,
        usage: {
          inputTokens: 120,
          outputTokens: 40,
          cacheCreationTokens: 5,
          cacheReadTokens: 8,
          processedTokens: 160,
          estimatedUsd: null,
        },
      },
    ],
    byEffort: [
      {
        key: "xhigh",
        count: 1,
        usage: {
          inputTokens: 120,
          outputTokens: 40,
          cacheCreationTokens: 5,
          cacheReadTokens: 8,
          processedTokens: 160,
          estimatedUsd: null,
        },
      },
    ],
    ...overrides,
  };
}

function renderChips(props: Partial<Parameters<typeof ChatSessionChips>[0]> = {}) {
  return render(
    <ChatSessionChips
      contextType="project"
      contextId="project-1"
      isAgentActive={false}
      conversationId="conversation-1"
      providerHarness="codex"
      providerSessionId="thread-1"
      upstreamProvider="openai"
      providerProfile={null}
      modelDisplay={{ id: "gpt-5.4", label: "gpt-5.4" }}
      fallbackConversation={null}
      fallbackMessages={[]}
      {...props}
    />,
  );
}

describe("ChatSessionChips", () => {
  beforeEach(() => {
    mockUseConversationStats.mockReset();
    mockUseConversationStats.mockReturnValue({
      data: statsFixture(),
      isLoading: false,
    });
  });

  it("shows provider, model, effort, and stats by default", () => {
    renderChips();

    expect(screen.getByTestId("chat-session-provider-badge")).toHaveTextContent("Codex");
    expect(screen.getByText("gpt-5.4")).toBeInTheDocument();
    expect(screen.getByText("XHigh")).toBeInTheDocument();
    expect(screen.getByTestId("chat-session-stats-button")).toBeInTheDocument();
  });

  it("can show stats without provider and model chips", () => {
    renderChips({ showProviderModel: false });

    expect(screen.getByTestId("chat-session-stats-button")).toBeInTheDocument();
    expect(screen.queryByTestId("chat-session-provider-badge")).not.toBeInTheDocument();
    expect(screen.queryByText("gpt-5.4")).not.toBeInTheDocument();
    expect(screen.queryByText("XHigh")).not.toBeInTheDocument();
  });

  it("can show provider and model chips without stats", () => {
    renderChips({ showStats: false });

    expect(screen.getByTestId("chat-session-provider-badge")).toHaveTextContent("Codex");
    expect(screen.getByText("gpt-5.4")).toBeInTheDocument();
    expect(screen.queryByTestId("chat-session-stats-button")).not.toBeInTheDocument();
    expect(mockUseConversationStats).toHaveBeenCalledWith(null, expect.any(Object));
  });
});
