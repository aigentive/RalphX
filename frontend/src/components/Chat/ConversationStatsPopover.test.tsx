import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { ConversationStatsPopover } from "./ConversationStatsPopover";
import type { ConversationStatsResponse } from "@/api/chat";

const mockUseConversationStats = vi.fn();

vi.mock("@/hooks/useConversationStats", () => ({
  useConversationStats: (...args: unknown[]) => mockUseConversationStats(...args),
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
      inputTokens: 76286,
      outputTokens: 12148,
      cacheCreationTokens: 12000,
      cacheReadTokens: 37920,
      processedTokens: 88434,
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
      inputTokens: 76286,
      outputTokens: 12148,
      cacheCreationTokens: 12000,
      cacheReadTokens: 37920,
      processedTokens: 88434,
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
      inputTokens: 76286,
      outputTokens: 12148,
      cacheCreationTokens: 12000,
      cacheReadTokens: 37920,
      processedTokens: 88434,
      estimatedUsd: null,
    } }],
    byModel: [{ key: "gpt-5.4", count: 1, usage: {
      inputTokens: 76286,
      outputTokens: 12148,
      cacheCreationTokens: 12000,
      cacheReadTokens: 37920,
      processedTokens: 88434,
      estimatedUsd: null,
    } }],
    byEffort: [{ key: "xhigh", count: 1, usage: {
      inputTokens: 76286,
      outputTokens: 12148,
      cacheCreationTokens: 12000,
      cacheReadTokens: 37920,
      processedTokens: 88434,
      estimatedUsd: null,
    } }],
    ...overrides,
  };
}

describe("ConversationStatsPopover", () => {
  beforeEach(() => {
    mockUseConversationStats.mockReset();
  });

  it("renders compact token totals for large conversations", async () => {
    mockUseConversationStats.mockReturnValue({
      data: makeStats(),
      isLoading: false,
    });

    render(
      <ConversationStatsPopover
        conversationId="conv-1"
        fallbackConversation={null}
        fallbackMessages={null}
      />,
    );

    fireEvent.click(screen.getByTestId("chat-session-stats-button"));

    expect(await screen.findByText("Conversation stats")).toBeInTheDocument();
    expect(screen.getByText("88.4k")).toBeInTheDocument();
    expect(screen.getByText("76.3k")).toBeInTheDocument();
    expect(screen.getByText("12.1k")).toBeInTheDocument();
    expect(screen.getByText("49.9k")).toBeInTheDocument();
    expect(screen.getByText(/already included in Codex Input/)).toBeInTheDocument();
  });

  it("hides run coverage rows when no run aggregates exist", async () => {
    mockUseConversationStats.mockReturnValue({
      data: makeStats(),
      isLoading: false,
    });

    render(
      <ConversationStatsPopover
        conversationId="conv-1"
        fallbackConversation={null}
        fallbackMessages={null}
      />,
    );

    fireEvent.click(screen.getByTestId("chat-session-stats-button"));

    expect(await screen.findByText("Coverage")).toBeInTheDocument();
    expect(screen.getByText("Usage captured on all provider turns")).toBeInTheDocument();
    expect(screen.getByText("Attribution captured on all provider turns")).toBeInTheDocument();
    expect(screen.queryByText(/Runs:/)).not.toBeInTheDocument();
  });

  it("provides an accessible app tooltip for the icon-only trigger", async () => {
    mockUseConversationStats.mockReturnValue({
      data: makeStats(),
      isLoading: false,
    });

    render(<ConversationStatsPopover conversationId="conv-1" />);
    const trigger = screen.getByRole("button", { name: "Conversation stats" });
    fireEvent.pointerMove(trigger);

    expect(await screen.findByRole("tooltip")).toHaveTextContent("Conversation stats");
  });

  it("shows pending usage copy during an active turn when provider totals have not arrived yet", async () => {
    mockUseConversationStats.mockReturnValue({
      data: makeStats({
        effectiveUsageTotals: {
          inputTokens: 0,
          outputTokens: 0,
          cacheCreationTokens: 0,
          cacheReadTokens: 0,
          processedTokens: null,
          estimatedUsd: null,
        },
        usageCoverage: {
          providerMessageCount: 1,
          providerMessagesWithUsage: 0,
          runCount: 0,
          runsWithUsage: 0,
          effectiveRunConversationCount: 0,
          effectiveMessageConversationCount: 0,
          legacyEstimatedSampleCount: 0,
          fallbackEstimatedSampleCount: 0,
          uncountedSampleCount: 0,
          effectiveTotalsSource: "none",
        },
      }),
      isLoading: false,
    });

    render(
      <ConversationStatsPopover
        conversationId="conv-1"
        fallbackConversation={null}
        fallbackMessages={null}
        isLiveTurnActive={true}
      />,
    );

    fireEvent.click(screen.getByTestId("chat-session-stats-button"));

    expect(await screen.findByText("Usage totals are pending until the provider reports the current turn.")).toBeInTheDocument();
    expect(screen.getAllByText("Pending")).toHaveLength(5);
  });

  it("discloses estimated and uncounted capture quality", async () => {
    mockUseConversationStats.mockReturnValue({
      data: makeStats({
        usageCoverage: {
          ...makeStats().usageCoverage,
          legacyEstimatedSampleCount: 2,
          fallbackEstimatedSampleCount: 1,
          uncountedSampleCount: 3,
        },
      }),
      isLoading: false,
    });

    render(<ConversationStatsPopover conversationId="conv-1" />);
    fireEvent.click(screen.getByRole("button", { name: "Conversation stats" }));

    expect(await screen.findByText("2 legacy-estimated sample(s)")).toBeInTheDocument();
    expect(screen.getByText("1 provider-fallback sample(s)")).toBeInTheDocument();
    expect(screen.getByText("3 uncounted sample(s)")).toBeInTheDocument();
  });
});
