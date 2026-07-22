import { describe, expect, it, vi } from "vitest";
import { typedInvoke } from "@/lib/tauri";
import { getProjectChatUsageStats } from "./metrics";

vi.mock("@/lib/tauri", () => ({
  typedInvoke: vi.fn(),
}));

describe("chat usage metrics contract", () => {
  it("preserves processed totals and accounting quality metadata", async () => {
    vi.mocked(typedInvoke).mockImplementation(async (_command, _args, schema) =>
      schema.parse({
        scopeType: "project",
        scopeId: "project-1",
        conversationCount: 2,
        messageUsageTotals: usage(110),
        runUsageTotals: usage(220),
        effectiveUsageTotals: usage(330),
        usageCoverage: {
          providerMessageCount: 3,
          providerMessagesWithUsage: 2,
          runCount: 2,
          runsWithUsage: 2,
          effectiveRunConversationCount: 1,
          effectiveMessageConversationCount: 1,
          legacyEstimatedSampleCount: 1,
          fallbackEstimatedSampleCount: 2,
          uncountedSampleCount: 3,
          effectiveTotalsSource: "mixed",
        },
        attributionCoverage: {
          providerMessageCount: 3,
          providerMessagesWithAttribution: 3,
          runCount: 2,
          runsWithAttribution: 2,
        },
        byContextType: [],
        byHarness: [],
        byUpstreamProvider: [],
        byModel: [],
        byEffort: [],
      }),
    );

    const result = await getProjectChatUsageStats("project-1");

    expect(result.effectiveUsageTotals.processedTokens).toBe(330);
    expect(result.usageCoverage).toMatchObject({
      effectiveTotalsSource: "mixed",
      effectiveRunConversationCount: 1,
      effectiveMessageConversationCount: 1,
      legacyEstimatedSampleCount: 1,
      fallbackEstimatedSampleCount: 2,
      uncountedSampleCount: 3,
    });
  });
});

function usage(processedTokens: number) {
  return {
    inputTokens: processedTokens - 10,
    outputTokens: 10,
    cacheCreationTokens: 0,
    cacheReadTokens: 0,
    processedTokens,
    estimatedUsd: null,
  };
}
