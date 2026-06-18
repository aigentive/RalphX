import { z } from "zod";
import { typedInvoke } from "@/lib/tauri";
import {
  ProjectPrInsightsSchema,
  ProjectStatsSchema,
  ProjectTrendsSchema,
} from "@/types/project-stats";
import type {
  ProjectPrInsights,
  ProjectStats,
  ProjectTrends,
} from "@/types/project-stats";

export interface ScopeUsageTotals {
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  estimatedUsd: number | null;
}

export interface ScopeUsageBucket {
  key: string;
  count: number;
  usage: ScopeUsageTotals;
}

export interface ScopeUsageCoverage {
  providerMessageCount: number;
  providerMessagesWithUsage: number;
  runCount: number;
  runsWithUsage: number;
  effectiveTotalsSource: string;
}

export interface ScopeAttributionCoverage {
  providerMessageCount: number;
  providerMessagesWithAttribution: number;
  runCount: number;
  runsWithAttribution: number;
}

export interface ScopeUsageStats {
  scopeType: string;
  scopeId: string;
  conversationCount: number;
  messageUsageTotals: ScopeUsageTotals;
  runUsageTotals: ScopeUsageTotals;
  effectiveUsageTotals: ScopeUsageTotals;
  usageCoverage: ScopeUsageCoverage;
  attributionCoverage: ScopeAttributionCoverage;
  byContextType: ScopeUsageBucket[];
  byHarness: ScopeUsageBucket[];
  byUpstreamProvider: ScopeUsageBucket[];
  byModel: ScopeUsageBucket[];
  byEffort: ScopeUsageBucket[];
}

const ScopeUsageTotalsSchema = z.object({
  input_tokens: z.number(),
  output_tokens: z.number(),
  cache_creation_tokens: z.number(),
  cache_read_tokens: z.number(),
  estimated_usd: z.number().nullable(),
});

const ScopeUsageBucketSchema = z.object({
  key: z.string(),
  count: z.number(),
  usage: ScopeUsageTotalsSchema,
});

const ScopeUsageCoverageSchema = z.object({
  provider_message_count: z.number(),
  provider_messages_with_usage: z.number(),
  run_count: z.number(),
  runs_with_usage: z.number(),
  effective_totals_source: z.string(),
});

const ScopeAttributionCoverageSchema = z.object({
  provider_message_count: z.number(),
  provider_messages_with_attribution: z.number(),
  run_count: z.number(),
  runs_with_attribution: z.number(),
});

const ScopeUsageStatsSchema = z.object({
  scope_type: z.string(),
  scope_id: z.string(),
  conversation_count: z.number(),
  message_usage_totals: ScopeUsageTotalsSchema,
  run_usage_totals: ScopeUsageTotalsSchema,
  effective_usage_totals: ScopeUsageTotalsSchema,
  usage_coverage: ScopeUsageCoverageSchema,
  attribution_coverage: ScopeAttributionCoverageSchema,
  by_context_type: z.array(ScopeUsageBucketSchema),
  by_harness: z.array(ScopeUsageBucketSchema),
  by_upstream_provider: z.array(ScopeUsageBucketSchema),
  by_model: z.array(ScopeUsageBucketSchema),
  by_effort: z.array(ScopeUsageBucketSchema),
});

function transformTotals(raw: z.infer<typeof ScopeUsageTotalsSchema>): ScopeUsageTotals {
  return {
    inputTokens: raw.input_tokens,
    outputTokens: raw.output_tokens,
    cacheCreationTokens: raw.cache_creation_tokens,
    cacheReadTokens: raw.cache_read_tokens,
    estimatedUsd: raw.estimated_usd,
  };
}

function transformBucket(raw: z.infer<typeof ScopeUsageBucketSchema>): ScopeUsageBucket {
  return {
    key: raw.key,
    count: raw.count,
    usage: transformTotals(raw.usage),
  };
}

function transformScopeUsageStats(
  raw: z.infer<typeof ScopeUsageStatsSchema>,
): ScopeUsageStats {
  return {
    scopeType: raw.scope_type,
    scopeId: raw.scope_id,
    conversationCount: raw.conversation_count,
    messageUsageTotals: transformTotals(raw.message_usage_totals),
    runUsageTotals: transformTotals(raw.run_usage_totals),
    effectiveUsageTotals: transformTotals(raw.effective_usage_totals),
    usageCoverage: {
      providerMessageCount: raw.usage_coverage.provider_message_count,
      providerMessagesWithUsage: raw.usage_coverage.provider_messages_with_usage,
      runCount: raw.usage_coverage.run_count,
      runsWithUsage: raw.usage_coverage.runs_with_usage,
      effectiveTotalsSource: raw.usage_coverage.effective_totals_source,
    },
    attributionCoverage: {
      providerMessageCount: raw.attribution_coverage.provider_message_count,
      providerMessagesWithAttribution: raw.attribution_coverage.provider_messages_with_attribution,
      runCount: raw.attribution_coverage.run_count,
      runsWithAttribution: raw.attribution_coverage.runs_with_attribution,
    },
    byContextType: raw.by_context_type.map(transformBucket),
    byHarness: raw.by_harness.map(transformBucket),
    byUpstreamProvider: raw.by_upstream_provider.map(transformBucket),
    byModel: raw.by_model.map(transformBucket),
    byEffort: raw.by_effort.map(transformBucket),
  };
}

export async function getProjectStats(
  projectId: string,
  weekStartDay?: number,
  tzOffsetMinutes?: number,
): Promise<ProjectStats> {
  return typedInvoke(
    "get_project_stats",
    {
      projectId,
      ...(weekStartDay !== undefined && { weekStartDay }),
      ...(tzOffsetMinutes !== undefined && { tzOffsetMinutes }),
    },
    ProjectStatsSchema,
  );
}

export async function getProjectTrends(
  projectId: string,
  weekStartDay?: number,
  tzOffsetMinutes?: number,
): Promise<ProjectTrends> {
  return typedInvoke(
    "get_project_trends",
    {
      projectId,
      ...(weekStartDay !== undefined && { weekStartDay }),
      ...(tzOffsetMinutes !== undefined && { tzOffsetMinutes }),
    },
    ProjectTrendsSchema,
  );
}

export async function getProjectPrInsights(
  projectId: string,
  weekStartDay?: number,
  tzOffsetMinutes?: number,
): Promise<ProjectPrInsights> {
  return typedInvoke(
    "get_project_pr_insights",
    {
      projectId,
      ...(weekStartDay !== undefined && { weekStartDay }),
      ...(tzOffsetMinutes !== undefined && { tzOffsetMinutes }),
    },
    ProjectPrInsightsSchema,
  );
}

export async function getProjectChatUsageStats(projectId: string): Promise<ScopeUsageStats> {
  const raw = await typedInvoke(
    "get_project_chat_usage_stats",
    { projectId },
    ScopeUsageStatsSchema,
  );
  return transformScopeUsageStats(raw);
}

export async function getTaskChatUsageStats(taskId: string): Promise<ScopeUsageStats> {
  const raw = await typedInvoke(
    "get_task_chat_usage_stats",
    { taskId },
    ScopeUsageStatsSchema,
  );
  return transformScopeUsageStats(raw);
}
