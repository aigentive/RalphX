/**
 * Project stats types
 *
 * These types are used by the ProjectStatsCard component and the
 * useProjectStats hook. The Tauri backend exposes get_project_stats
 * which returns data shaped like ProjectStats.
 *
 * All fields are camelCase — backend uses #[serde(rename_all = "camelCase")].
 */

import { z } from "zod";

// ============================================================================
// Schemas
// ============================================================================

/**
 * Average time spent in a single pipeline phase (from LAG() window queries)
 */
export const CycleTimePhaseSchema = z.object({
  phase: z.string(),
  avgMinutes: z.number(),
  sampleSize: z.number(),
});

/**
 * Estimated Manual Effort range (low..high hours)
 * Only present when ≥5 tasks are merged
 */
export const EmeEstimateSchema = z.object({
  lowHours: z.number(),
  highHours: z.number(),
  scope: z.string(),
  scopeLabel: z.string(),
  taskCount: z.number(),
  earliestTaskDate: z.string().nullable(),
  latestTaskDate: z.string().nullable(),
});

export const ColumnDwellTimeSchema = z.object({
  columnId: z.string(),
  columnName: z.string(),
  avgMinutes: z.number(),
  sampleSize: z.number(),
});

export const ProjectStatsSchema = z.object({
  taskCount: z.number(),
  tasksCompletedToday: z.number(),
  tasksCompletedThisWeek: z.number(),
  tasksCompletedThisMonth: z.number(),
  agentSuccessRate: z.number(),
  agentSuccessCount: z.number(),
  agentTotalCount: z.number(),
  reviewPassRate: z.number(),
  reviewPassCount: z.number(),
  reviewTotalCount: z.number(),
  cycleTimeBreakdown: z.array(CycleTimePhaseSchema),
  columnDwellTimes: z.array(ColumnDwellTimeSchema),
  avgPipelineMinutes: z.number().nullable(),
  eme: EmeEstimateSchema.nullable(),
});

export const WeeklyDataPointSchema = z.object({
  weekStart: z.string(),
  value: z.number(),
  sampleSize: z.number(),
});

export const DeliveryWeeklyThroughputPointSchema = z.object({
  weekStart: z.string(),
  unifiedDeliveries: z.number(),
  taskDeliveries: z.number(),
  workspaceDeliveries: z.number(),
  mergedPrs: z.number(),
  sampleSize: z.number(),
});

export const ProjectTrendsSchema = z.object({
  weeklyThroughput: z.array(WeeklyDataPointSchema),
  weeklyDeliveryThroughput: z.array(DeliveryWeeklyThroughputPointSchema),
  weeklyCycleTime: z.array(WeeklyDataPointSchema),
  weeklyPipelineCycleTime: z.array(WeeklyDataPointSchema),
  weeklySuccessRate: z.array(WeeklyDataPointSchema),
});

export const PrInsightsSummarySchema = z.object({
  totalPrs: z.number(),
  directWorkspacePrs: z.number(),
  taskPipelinePrs: z.number(),
  executionOwnedWorkspaceRefs: z.number(),
  mergedPrs: z.number(),
  openPrs: z.number(),
  draftPrs: z.number(),
  changesRequestedPrs: z.number(),
  closedPrs: z.number(),
  needsAgentPrs: z.number(),
  unpushedWorkspacePrs: z.number(),
  totalWorkspaces: z.number(),
  directWorkspaces: z.number(),
  directWorkspacesWithPrs: z.number(),
  directWorkspacePrConversionRate: z.number(),
  terminalMergeRate: z.number(),
  avgWorkspacePrCycleHours: z.number().nullable(),
  avgPlanPrWaitHours: z.number().nullable(),
  requestedChangesEvents: z.number(),
  autofixNeededEvents: z.number(),
  agentFixCompletedEvents: z.number(),
  supervisionEnabledWorkspaces: z.number(),
  autoMergeDesiredWorkspaces: z.number(),
  autoMergeActiveWorkspaces: z.number(),
});

export const PrInsightOriginBreakdownSchema = z.object({
  origin: z.string(),
  label: z.string(),
  countedInTotals: z.boolean(),
  totalPrs: z.number(),
  mergedPrs: z.number(),
  openPrs: z.number(),
  draftPrs: z.number(),
  changesRequestedPrs: z.number(),
  closedPrs: z.number(),
  needsAgentPrs: z.number(),
  unpushedWorkspacePrs: z.number(),
});

export const PrWeeklyThroughputPointSchema = z.object({
  weekStart: z.string(),
  opened: z.number(),
  merged: z.number(),
  sampleSize: z.number(),
});

export const WorkspaceStateDwellTimeSchema = z.object({
  stateFamily: z.string(),
  state: z.string(),
  label: z.string(),
  avgMinutes: z.number(),
  sampleSize: z.number(),
});

export const PrInsightItemSchema = z.object({
  origin: z.string(),
  label: z.string(),
  countedInTotals: z.boolean(),
  status: z.string(),
  prNumber: z.number().nullable(),
  prUrl: z.string().nullable(),
  branchName: z.string(),
  baseRef: z.string(),
  conversationId: z.string().nullable(),
  taskId: z.string().nullable(),
  planBranchId: z.string().nullable(),
  createdAt: z.string(),
  updatedAt: z.string().nullable(),
  mergedAt: z.string().nullable(),
});

export const ProjectPrInsightsSchema = z.object({
  summary: PrInsightsSummarySchema,
  origins: z.array(PrInsightOriginBreakdownSchema),
  weeklyThroughput: z.array(PrWeeklyThroughputPointSchema),
  workspaceDwellTimes: z.array(WorkspaceStateDwellTimeSchema),
  latestPrs: z.array(PrInsightItemSchema),
});

// ============================================================================
// Types
// ============================================================================

export type CycleTimePhase = z.infer<typeof CycleTimePhaseSchema>;
export type ColumnDwellTime = z.infer<typeof ColumnDwellTimeSchema>;
export type EmeEstimate = z.infer<typeof EmeEstimateSchema>;
export type ProjectStats = z.infer<typeof ProjectStatsSchema>;
export type WeeklyDataPoint = z.infer<typeof WeeklyDataPointSchema>;
export type DeliveryWeeklyThroughputPoint = z.infer<typeof DeliveryWeeklyThroughputPointSchema>;
export type ProjectTrends = z.infer<typeof ProjectTrendsSchema>;
export type PrInsightsSummary = z.infer<typeof PrInsightsSummarySchema>;
export type PrInsightOriginBreakdown = z.infer<typeof PrInsightOriginBreakdownSchema>;
export type PrWeeklyThroughputPoint = z.infer<typeof PrWeeklyThroughputPointSchema>;
export type WorkspaceStateDwellTime = z.infer<typeof WorkspaceStateDwellTimeSchema>;
export type PrInsightItem = z.infer<typeof PrInsightItemSchema>;
export type ProjectPrInsights = z.infer<typeof ProjectPrInsightsSchema>;

// ============================================================================
// Metrics config
// ============================================================================

/**
 * Per-project calibration config for EME (Estimated Manual Effort) computation.
 * Persisted via get_metrics_config / save_metrics_config Tauri commands.
 */
export const MetricsConfigSchema = z.object({
  simpleBaseHours: z.number().min(0.5).max(40),
  mediumBaseHours: z.number().min(0.5).max(40),
  complexBaseHours: z.number().min(0.5).max(40),
  calendarFactor: z.number().min(1).max(3),
  workingDaysPerWeek: z.number().int().min(1).max(7),
});

export type MetricsConfig = z.infer<typeof MetricsConfigSchema>;

export const DEFAULT_METRICS_CONFIG: MetricsConfig = {
  simpleBaseHours: 1,
  mediumBaseHours: 2,
  complexBaseHours: 4,
  calendarFactor: 1.3,
  workingDaysPerWeek: 5,
};
