import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ScopeUsageStats } from "@/api/metrics";
import { UsageInsightsCard } from "./UsageInsightsCard";

const usageStats: ScopeUsageStats = {
  scopeType: "project",
  scopeId: "project-1",
  conversationCount: 3,
  messageUsageTotals: {
    inputTokens: 1200,
    outputTokens: 340,
    cacheCreationTokens: 50,
    cacheReadTokens: 80,
    processedTokens: 1540,
    estimatedUsd: 1.23,
  },
  runUsageTotals: {
    inputTokens: 1400,
    outputTokens: 360,
    cacheCreationTokens: 50,
    cacheReadTokens: 80,
    processedTokens: 1760,
    estimatedUsd: 1.4,
  },
  effectiveUsageTotals: {
    inputTokens: 1200,
    outputTokens: 340,
    cacheCreationTokens: 50,
    cacheReadTokens: 80,
    processedTokens: 1540,
    estimatedUsd: 1.23,
  },
  usageCoverage: {
    providerMessageCount: 12,
    providerMessagesWithUsage: 10,
    runCount: 6,
    runsWithUsage: 5,
    effectiveRunConversationCount: 1,
    effectiveMessageConversationCount: 2,
    legacyEstimatedSampleCount: 0,
    fallbackEstimatedSampleCount: 0,
    uncountedSampleCount: 0,
    effectiveTotalsSource: "messages",
  },
  attributionCoverage: {
    providerMessageCount: 12,
    providerMessagesWithAttribution: 11,
    runCount: 6,
    runsWithAttribution: 5,
  },
  byContextType: [
    {
      key: "task_execution",
      count: 2,
      usage: {
        inputTokens: 900,
        outputTokens: 200,
        cacheCreationTokens: 50,
        cacheReadTokens: 80,
        processedTokens: 1100,
        estimatedUsd: 0.9,
      },
    },
  ],
  byHarness: [
    {
      key: "codex",
      count: 2,
      usage: {
        inputTokens: 900,
        outputTokens: 200,
        cacheCreationTokens: 50,
        cacheReadTokens: 80,
        processedTokens: 1100,
        estimatedUsd: 0.9,
      },
    },
  ],
  byUpstreamProvider: [
    {
      key: "openai",
      count: 2,
      usage: {
        inputTokens: 900,
        outputTokens: 200,
        cacheCreationTokens: 50,
        cacheReadTokens: 80,
        processedTokens: 1100,
        estimatedUsd: 0.9,
      },
    },
  ],
  byModel: [
    {
      key: "gpt-5.4",
      count: 2,
      usage: {
        inputTokens: 900,
        outputTokens: 200,
        cacheCreationTokens: 50,
        cacheReadTokens: 80,
        processedTokens: 1100,
        estimatedUsd: 0.9,
      },
    },
  ],
  byEffort: [
    {
      key: "high",
      count: 2,
      usage: {
        inputTokens: 900,
        outputTokens: 200,
        cacheCreationTokens: 50,
        cacheReadTokens: 80,
        processedTokens: 1100,
        estimatedUsd: 0.9,
      },
    },
  ],
};

describe("UsageInsightsCard", () => {
  it("renders aggregated usage totals and dominant breakdowns", () => {
    render(<UsageInsightsCard stats={usageStats} />);

    expect(screen.getByText("AI Usage")).toBeInTheDocument();
    expect(screen.getByText("Processed")).toBeInTheDocument();
    expect(screen.getByText("1,540")).toBeInTheDocument();
    expect(screen.getByText("1,200")).toBeInTheDocument();
    expect(screen.getByText("340")).toBeInTheDocument();
    expect(screen.getByText("$1.23")).toBeInTheDocument();
    expect(screen.getByText("Harness: codex")).toBeInTheDocument();
    expect(screen.getByText("Provider: openai")).toBeInTheDocument();
    expect(screen.getByText("Model: gpt-5.4")).toBeInTheDocument();
    expect(screen.getByText("Conversations: 3")).toBeInTheDocument();
  });

  it("discloses estimated and uncounted capture quality", () => {
    render(
      <UsageInsightsCard
        stats={{
          ...usageStats,
          usageCoverage: {
            ...usageStats.usageCoverage,
            legacyEstimatedSampleCount: 2,
            fallbackEstimatedSampleCount: 1,
            uncountedSampleCount: 3,
          },
        }}
      />,
    );

    expect(screen.getByText("2 legacy-estimated samples")).toBeInTheDocument();
    expect(screen.getByText("1 provider-fallback sample")).toBeInTheDocument();
    expect(screen.getByText("3 uncounted samples")).toBeInTheDocument();
  });
});
