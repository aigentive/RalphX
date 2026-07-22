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
  processedTokens: number | null;
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
  effectiveRunConversationCount: number;
  effectiveMessageConversationCount: number;
  legacyEstimatedSampleCount: number;
  fallbackEstimatedSampleCount: number;
  uncountedSampleCount: number;
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
  inputTokens: z.number(),
  outputTokens: z.number(),
  cacheCreationTokens: z.number(),
  cacheReadTokens: z.number(),
  processedTokens: z.number().nullable(),
  estimatedUsd: z.number().nullable(),
});

const ScopeUsageBucketSchema = z.object({
  key: z.string(),
  count: z.number(),
  usage: ScopeUsageTotalsSchema,
});

const ScopeUsageCoverageSchema = z.object({
  providerMessageCount: z.number(),
  providerMessagesWithUsage: z.number(),
  runCount: z.number(),
  runsWithUsage: z.number(),
  effectiveRunConversationCount: z.number(),
  effectiveMessageConversationCount: z.number(),
  legacyEstimatedSampleCount: z.number(),
  fallbackEstimatedSampleCount: z.number(),
  uncountedSampleCount: z.number(),
  effectiveTotalsSource: z.string(),
});

const ScopeAttributionCoverageSchema = z.object({
  providerMessageCount: z.number(),
  providerMessagesWithAttribution: z.number(),
  runCount: z.number(),
  runsWithAttribution: z.number(),
});

const ScopeUsageStatsSchema = z.object({
  scopeType: z.string(),
  scopeId: z.string(),
  conversationCount: z.number(),
  messageUsageTotals: ScopeUsageTotalsSchema,
  runUsageTotals: ScopeUsageTotalsSchema,
  effectiveUsageTotals: ScopeUsageTotalsSchema,
  usageCoverage: ScopeUsageCoverageSchema,
  attributionCoverage: ScopeAttributionCoverageSchema,
  byContextType: z.array(ScopeUsageBucketSchema),
  byHarness: z.array(ScopeUsageBucketSchema),
  byUpstreamProvider: z.array(ScopeUsageBucketSchema),
  byModel: z.array(ScopeUsageBucketSchema),
  byEffort: z.array(ScopeUsageBucketSchema),
});

function transformTotals(raw: z.infer<typeof ScopeUsageTotalsSchema>): ScopeUsageTotals {
  return {
    inputTokens: raw.inputTokens,
    outputTokens: raw.outputTokens,
    cacheCreationTokens: raw.cacheCreationTokens,
    cacheReadTokens: raw.cacheReadTokens,
    processedTokens: raw.processedTokens,
    estimatedUsd: raw.estimatedUsd,
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
    scopeType: raw.scopeType,
    scopeId: raw.scopeId,
    conversationCount: raw.conversationCount,
    messageUsageTotals: transformTotals(raw.messageUsageTotals),
    runUsageTotals: transformTotals(raw.runUsageTotals),
    effectiveUsageTotals: transformTotals(raw.effectiveUsageTotals),
    usageCoverage: {
      providerMessageCount: raw.usageCoverage.providerMessageCount,
      providerMessagesWithUsage: raw.usageCoverage.providerMessagesWithUsage,
      runCount: raw.usageCoverage.runCount,
      runsWithUsage: raw.usageCoverage.runsWithUsage,
      effectiveRunConversationCount:
        raw.usageCoverage.effectiveRunConversationCount,
      effectiveMessageConversationCount:
        raw.usageCoverage.effectiveMessageConversationCount,
      legacyEstimatedSampleCount:
        raw.usageCoverage.legacyEstimatedSampleCount,
      fallbackEstimatedSampleCount:
        raw.usageCoverage.fallbackEstimatedSampleCount,
      uncountedSampleCount: raw.usageCoverage.uncountedSampleCount,
      effectiveTotalsSource: raw.usageCoverage.effectiveTotalsSource,
    },
    attributionCoverage: {
      providerMessageCount: raw.attributionCoverage.providerMessageCount,
      providerMessagesWithAttribution: raw.attributionCoverage.providerMessagesWithAttribution,
      runCount: raw.attributionCoverage.runCount,
      runsWithAttribution: raw.attributionCoverage.runsWithAttribution,
    },
    byContextType: raw.byContextType.map(transformBucket),
    byHarness: raw.byHarness.map(transformBucket),
    byUpstreamProvider: raw.byUpstreamProvider.map(transformBucket),
    byModel: raw.byModel.map(transformBucket),
    byEffort: raw.byEffort.map(transformBucket),
  };
}

function insightsArgs(
  projectId?: string | null,
  weekStartDay?: number,
  tzOffsetMinutes?: number,
): Record<string, unknown> {
  return {
    ...(projectId != null && projectId.trim() !== "" && { projectId }),
    ...(weekStartDay !== undefined && { weekStartDay }),
    ...(tzOffsetMinutes !== undefined && { tzOffsetMinutes }),
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

export async function getInsightsStats(
  projectId?: string | null,
  weekStartDay?: number,
  tzOffsetMinutes?: number,
): Promise<ProjectStats> {
  return typedInvoke(
    "get_insights_stats",
    insightsArgs(projectId, weekStartDay, tzOffsetMinutes),
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

export async function getInsightsTrends(
  projectId?: string | null,
  weekStartDay?: number,
  tzOffsetMinutes?: number,
): Promise<ProjectTrends> {
  return typedInvoke(
    "get_insights_trends",
    insightsArgs(projectId, weekStartDay, tzOffsetMinutes),
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

export async function getInsightsPrInsights(
  projectId?: string | null,
  weekStartDay?: number,
  tzOffsetMinutes?: number,
): Promise<ProjectPrInsights> {
  return typedInvoke(
    "get_insights_pr_insights",
    insightsArgs(projectId, weekStartDay, tzOffsetMinutes),
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

export async function getInsightsChatUsageStats(
  projectId?: string | null,
): Promise<ScopeUsageStats> {
  const raw = await typedInvoke(
    "get_insights_chat_usage_stats",
    insightsArgs(projectId),
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
