import { BarChart2 } from "lucide-react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { useConversationStats } from "@/hooks/useConversationStats";
import type { ChatMessageResponse } from "@/api/chat";
import type { ChatConversation } from "@/types/chat-conversation";

interface ConversationStatsPopoverProps {
  conversationId: string | null;
  fallbackConversation?: ChatConversation | null | undefined;
  fallbackMessages?: ChatMessageResponse[] | null | undefined;
  stats?: ReturnType<typeof useConversationStats>["data"];
  isLoading?: boolean;
  isLiveTurnActive?: boolean;
}

function formatInteger(value: number): string {
  return new Intl.NumberFormat("en-US").format(value);
}

function formatCompactInteger(value: number): string {
  if (Math.abs(value) < 10_000) {
    return formatInteger(value);
  }

  return new Intl.NumberFormat("en-US", {
    notation: "compact",
    maximumFractionDigits: 1,
  })
    .format(value)
    .replace("K", "k")
    .replace("M", "m")
    .replace("B", "b");
}

function formatUsd(value: number | null): string {
  if (value == null) {
    return "Unavailable";
  }

  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: 4,
  }).format(value);
}

function formatCoverageLine(label: string, complete: number, total: number): string {
  if (total <= 0) {
    return `${label} unavailable`;
  }
  if (complete >= total) {
    return `${label} captured on all provider turns`;
  }
  return `${label} captured on ${complete} of ${total} provider turns`;
}

export function ConversationStatsPopover({
  conversationId,
  fallbackConversation,
  fallbackMessages,
  stats: providedStats,
  isLoading: providedIsLoading,
  isLiveTurnActive = false,
}: ConversationStatsPopoverProps) {
  const statsQuery = useConversationStats(conversationId, {
    fallbackConversation,
    fallbackMessages,
  });
  const stats = providedStats ?? statsQuery.data;
  const isLoading = providedIsLoading ?? statsQuery.isLoading;
  const usagePending =
    isLiveTurnActive && stats?.usageCoverage.effectiveTotalsSource === "none";

  if (!conversationId) {
    return null;
  }

  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="flex items-center justify-center w-6 h-6 rounded text-text-primary/38 hover:text-text-primary/72 hover:bg-[var(--overlay-faint)] transition-colors"
          aria-label="Conversation stats"
          data-testid="chat-session-stats-button"
        >
          <BarChart2 className="w-3.5 h-3.5" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="w-80 p-0 border-[var(--border-subtle)] bg-[color-mix(in_srgb,_var(--bg-base)_96%,_transparent)] shadow-xl"
      >
        <div className="p-3 border-b border-[var(--border-subtle)]">
          <div className="text-sm font-medium text-text-primary/90">Conversation stats</div>
          <div className="text-[0.6875rem] text-text-primary/45 mt-1">
            {usagePending
              ? "Usage totals are pending until the provider reports the current turn."
              : `Aggregated from ${stats?.usageCoverage.effectiveTotalsSource ?? "available data"}.`}
          </div>
        </div>

        {isLoading ? (
          <div className="p-3 text-sm text-text-primary/55">Loading conversation stats...</div>
        ) : !stats ? (
          <div className="p-3 text-sm text-text-primary/55">Stats are not available for this conversation.</div>
        ) : (
          <div className="p-3 space-y-3">
            <div className="grid grid-cols-2 gap-2">
              <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--overlay-faint)] p-2">
                <div className="text-[0.625rem] uppercase tracking-[0.08em] text-text-primary/38">Input</div>
                <div className="text-sm text-text-primary/88 mt-1">
                  {usagePending ? "Pending" : formatCompactInteger(stats.effectiveUsageTotals.inputTokens)}
                </div>
              </div>
              <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--overlay-faint)] p-2">
                <div className="text-[0.625rem] uppercase tracking-[0.08em] text-text-primary/38">Output</div>
                <div className="text-sm text-text-primary/88 mt-1">
                  {usagePending ? "Pending" : formatCompactInteger(stats.effectiveUsageTotals.outputTokens)}
                </div>
              </div>
              <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--overlay-faint)] p-2">
                <div className="text-[0.625rem] uppercase tracking-[0.08em] text-text-primary/38">Cache</div>
                <div className="text-sm text-text-primary/88 mt-1">
                  {usagePending
                    ? "Pending"
                    : formatCompactInteger(
                      stats.effectiveUsageTotals.cacheCreationTokens +
                        stats.effectiveUsageTotals.cacheReadTokens,
                    )}
                </div>
              </div>
              <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--overlay-faint)] p-2">
                <div className="text-[0.625rem] uppercase tracking-[0.08em] text-text-primary/38">Est. cost</div>
                <div className="text-sm text-text-primary/88 mt-1">
                  {usagePending ? "Pending" : formatUsd(stats.effectiveUsageTotals.estimatedUsd)}
                </div>
              </div>
            </div>

            <div className="grid grid-cols-2 gap-3 text-[0.6875rem]">
              <div>
                <div className="uppercase tracking-[0.08em] text-text-primary/38">Coverage</div>
                <div className="mt-1 text-text-primary/70">
                  {formatCoverageLine(
                    "Usage",
                    stats.usageCoverage.providerMessagesWithUsage,
                    stats.usageCoverage.providerMessageCount,
                  )}
                </div>
                {stats.usageCoverage.runCount > 0 && (
                  <div className="text-text-primary/70">
                    {`Run usage captured on ${stats.usageCoverage.runsWithUsage} of ${stats.usageCoverage.runCount} runs`}
                  </div>
                )}
              </div>
              <div>
                <div className="uppercase tracking-[0.08em] text-text-primary/38">Attribution</div>
                <div className="mt-1 text-text-primary/70">
                  {formatCoverageLine(
                    "Attribution",
                    stats.attributionCoverage.providerMessagesWithAttribution,
                    stats.attributionCoverage.providerMessageCount,
                  )}
                </div>
                {stats.attributionCoverage.runCount > 0 && (
                  <div className="text-text-primary/70">
                    {`Run attribution captured on ${stats.attributionCoverage.runsWithAttribution} of ${stats.attributionCoverage.runCount} runs`}
                  </div>
                )}
              </div>
            </div>

            <div className="space-y-2">
              <div className="uppercase tracking-[0.08em] text-[0.625rem] text-text-primary/38">Top breakdowns</div>
              {stats.byModel[0] && (
                <div className="flex items-center justify-between text-[0.6875rem] text-text-primary/72">
                  <span>Model</span>
                  <span className="truncate max-w-[12rem] text-right">{stats.byModel[0].key}</span>
                </div>
              )}
              {stats.byEffort[0] && (
                <div className="flex items-center justify-between text-[0.6875rem] text-text-primary/72">
                  <span>Effort</span>
                  <span>{stats.byEffort[0].key}</span>
                </div>
              )}
              {stats.byUpstreamProvider[0] && (
                <div className="flex items-center justify-between text-[0.6875rem] text-text-primary/72">
                  <span>Upstream</span>
                  <span>{stats.byUpstreamProvider[0].key}</span>
                </div>
              )}
            </div>
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}
