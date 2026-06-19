/**
 * Workflow schema type definitions
 *
 * WorkflowSchema defines how external columns map to internal statuses,
 * enabling custom kanban workflows while preserving state machine behavior.
 *
 * Note: Backend outputs snake_case (Rust default). Response schemas validate
 * the raw data, then transform functions convert to camelCase for frontend.
 */

import { z } from "zod";
import { InternalStatusSchema, type InternalStatus } from "./status";

// ============================================
// External Sync Configuration Types (future)
// ============================================

/**
 * Supported external sync providers
 */
export const SyncProviderSchema = z.enum(["jira", "github", "linear", "notion"]);
export type SyncProvider = z.infer<typeof SyncProviderSchema>;

/**
 * All sync provider values as a readonly array
 */
export const SYNC_PROVIDER_VALUES = SyncProviderSchema.options;

/**
 * Sync direction options
 */
export const SyncDirectionSchema = z.enum(["pull", "push", "bidirectional"]);
export type SyncDirection = z.infer<typeof SyncDirectionSchema>;

/**
 * All sync direction values as a readonly array
 */
export const SYNC_DIRECTION_VALUES = SyncDirectionSchema.options;

/**
 * Conflict resolution strategy
 */
export const ConflictResolutionSchema = z.enum([
  "external_wins",
  "internal_wins",
  "manual",
]);
export type ConflictResolution = z.infer<typeof ConflictResolutionSchema>;

/**
 * All conflict resolution values as a readonly array
 */
export const CONFLICT_RESOLUTION_VALUES = ConflictResolutionSchema.options;

/**
 * Mapping from an external status to internal status
 */
export const ExternalStatusMappingSchema = z.object({
  /** The external status name from the provider */
  externalStatus: z.string(),
  /** The internal status to map to */
  internalStatus: InternalStatusSchema,
  /** The workflow column to display in */
  columnId: z.string(),
});
export type ExternalStatusMapping = z.infer<typeof ExternalStatusMappingSchema>;

/**
 * Sync direction settings
 */
export const SyncSettingsSchema = z.object({
  /** Sync direction */
  direction: SyncDirectionSchema,
  /** Enable webhook for real-time sync */
  webhook: z.boolean().optional(),
});
export type SyncSettings = z.infer<typeof SyncSettingsSchema>;

/**
 * External sync configuration (placeholder for future implementation)
 */
export const ExternalSyncConfigSchema = z.object({
  /** The external provider */
  provider: SyncProviderSchema,
  /** Status mapping from external to internal (keyed by external status) */
  mapping: z.record(z.string(), ExternalStatusMappingSchema).default({}),
  /** Sync settings */
  sync: SyncSettingsSchema,
  /** How to resolve conflicts */
  conflictResolution: ConflictResolutionSchema,
});
export type ExternalSyncConfig = z.infer<typeof ExternalSyncConfigSchema>;

/**
 * Raw external status mapping response schema (snake_case from Rust)
 */
export const ExternalStatusMappingResponseSchema = z.object({
  external_status: z.string(),
  internal_status: InternalStatusSchema,
  column_id: z.string(),
});

/**
 * Raw sync settings response schema (snake_case from Rust)
 */
export const SyncSettingsResponseSchema = z.object({
  direction: SyncDirectionSchema,
  webhook: z.boolean().optional(),
});

/**
 * Raw external sync configuration response schema (snake_case from Rust)
 */
export const ExternalSyncConfigResponseSchema = z.object({
  provider: SyncProviderSchema,
  mapping: z.record(z.string(), ExternalStatusMappingResponseSchema).default({}),
  sync: SyncSettingsResponseSchema,
  conflict_resolution: ConflictResolutionSchema,
});

// ============================================
// State Grouping Types (Multi-State Columns)
// ============================================

/**
 * State group within a column
 * Allows multiple internal statuses to be grouped and displayed within a single column
 */
export const StateGroupSchema = z.object({
  /** Unique group identifier within the column */
  id: z.string(),
  /** Display label for the group header (e.g., "Fresh Tasks", "Needs Revision") */
  label: z.string(),
  /** Internal statuses that belong to this group */
  statuses: z.array(InternalStatusSchema),
  /** Optional Lucide icon name for the group */
  icon: z.string().optional(),
  /** Optional accent color for the group (CSS color value) */
  accentColor: z.string().optional(),
  /** Whether tasks can be dragged FROM this group (default: true) */
  canDragFrom: z.boolean().optional(),
  /** Whether tasks can be dropped TO this group (default: true) */
  canDropTo: z.boolean().optional(),
});

export type StateGroup = z.infer<typeof StateGroupSchema>;

// ============================================
// Column Behavior and Workflow Types
// ============================================

/**
 * Column behavior configuration
 * Controls how tasks behave when moved to this column
 */
const ColumnBehaviorSchema = z.object({
  /** Skip AI review for tasks in this column */
  skipReview: z.boolean().optional(),
  /** Automatically advance to next column when execution completes */
  autoAdvance: z.boolean().optional(),
  /** Agent profile to use for tasks in this column */
  agentProfile: z.string().optional(),
});

/**
 * Schema for a workflow column
 * Maps an external column to an internal status
 */
export const WorkflowColumnSchema = z.object({
  /** Unique column identifier */
  id: z.string(),
  /** Display name for the column */
  name: z.string(),
  /** Optional color for the column header */
  color: z.string().optional(),
  /** Optional icon name for the column */
  icon: z.string().optional(),
  /** Internal status this column maps to (primary status for single-state columns) */
  mapsTo: InternalStatusSchema,
  /** Optional behavior configuration */
  behavior: ColumnBehaviorSchema.optional(),
  /** Optional state groups for multi-state columns */
  groups: z.array(StateGroupSchema).optional(),
});

export type WorkflowColumn = z.infer<typeof WorkflowColumnSchema>;

/**
 * Default agent profiles for the workflow
 */
const WorkflowDefaultsSchema = z.object({
  /** Default worker agent profile */
  workerProfile: z.string().optional(),
  /** Default reviewer agent profile */
  reviewerProfile: z.string().optional(),
});

/**
 * Schema for a complete workflow definition
 * Note: Named WorkflowSchemaZ to avoid collision with the TypeScript type
 */
export const WorkflowSchemaZ = z.object({
  /** Unique workflow identifier */
  id: z.string(),
  /** Display name for the workflow */
  name: z.string(),
  /** Optional description */
  description: z.string().optional(),
  /** Ordered list of columns in the workflow */
  columns: z.array(WorkflowColumnSchema),
  /** Optional external sync configuration (future implementation) */
  externalSync: ExternalSyncConfigSchema.optional(),
  /** Optional default agent profiles */
  defaults: WorkflowDefaultsSchema.optional(),
  /** Whether this is the default workflow */
  isDefault: z.boolean().default(false),
});

export type WorkflowSchema = z.infer<typeof WorkflowSchemaZ>;

// ============================================
// Response Schemas (snake_case from Rust backend)
// ============================================

/**
 * State group response schema (snake_case from Rust)
 */
export const StateGroupResponseSchema = z.object({
  id: z.string(),
  label: z.string(),
  statuses: z.array(z.string()),
  icon: z.string().optional(),
  accent_color: z.string().optional(),
  can_drag_from: z.boolean().optional(),
  can_drop_to: z.boolean().optional(),
});

/**
 * Workflow column response schema (snake_case from Rust)
 */
export const WorkflowColumnResponseSchema = z.object({
  id: z.string(),
  name: z.string(),
  maps_to: z.string(),
  color: z.string().optional(),
  icon: z.string().optional(),
  skip_review: z.boolean().optional(),
  auto_advance: z.boolean().optional(),
  agent_profile: z.string().optional(),
  groups: z.array(StateGroupResponseSchema).optional(),
});

/**
 * Workflow response schema (snake_case from Rust)
 */
export const WorkflowResponseSchema = z.object({
  id: z.string(),
  name: z.string(),
  description: z.string().optional(),
  columns: z.array(WorkflowColumnResponseSchema),
  external_sync: ExternalSyncConfigResponseSchema.optional(),
  is_default: z.boolean(),
  worker_profile: z.string().optional(),
  reviewer_profile: z.string().optional(),
});

// ============================================
// Transform Functions
// ============================================

/**
 * Transform state group from snake_case response to camelCase frontend type
 */
export function transformStateGroup(
  raw: z.infer<typeof StateGroupResponseSchema>
): StateGroup {
  return {
    id: raw.id,
    label: raw.label,
    statuses: raw.statuses as InternalStatus[],
    icon: raw.icon,
    accentColor: raw.accent_color,
    canDragFrom: raw.can_drag_from,
    canDropTo: raw.can_drop_to,
  };
}

/**
 * Transform workflow column from snake_case response to camelCase frontend type
 */
export function transformWorkflowColumn(
  raw: z.infer<typeof WorkflowColumnResponseSchema>
): WorkflowColumn {
  return {
    id: raw.id,
    name: raw.name,
    mapsTo: raw.maps_to as InternalStatus,
    color: raw.color,
    icon: raw.icon,
    behavior: (raw.skip_review !== undefined || raw.auto_advance !== undefined || raw.agent_profile !== undefined)
      ? {
          skipReview: raw.skip_review,
          autoAdvance: raw.auto_advance,
          agentProfile: raw.agent_profile,
        }
      : undefined,
    groups: raw.groups?.map(transformStateGroup),
  };
}

/**
 * Transform external status mapping from snake_case response to camelCase frontend type
 */
export function transformExternalStatusMapping(
  raw: z.infer<typeof ExternalStatusMappingResponseSchema>
): ExternalStatusMapping {
  return {
    externalStatus: raw.external_status,
    internalStatus: raw.internal_status,
    columnId: raw.column_id,
  };
}

/**
 * Transform external sync config from snake_case response to camelCase frontend type
 */
export function transformExternalSyncConfig(
  raw: z.infer<typeof ExternalSyncConfigResponseSchema>
): ExternalSyncConfig {
  return {
    provider: raw.provider,
    mapping: Object.fromEntries(
      Object.entries(raw.mapping).map(([key, value]) => [
        key,
        transformExternalStatusMapping(value),
      ])
    ),
    sync: {
      direction: raw.sync.direction,
      webhook: raw.sync.webhook,
    },
    conflictResolution: raw.conflict_resolution,
  };
}

/**
 * Transform workflow from snake_case response to camelCase frontend type
 */
export function transformWorkflow(
  raw: z.infer<typeof WorkflowResponseSchema>
): WorkflowSchema {
  return {
    id: raw.id,
    name: raw.name,
    description: raw.description,
    columns: raw.columns.map(transformWorkflowColumn),
    externalSync: raw.external_sync
      ? transformExternalSyncConfig(raw.external_sync)
      : undefined,
    isDefault: raw.is_default,
    defaults: (raw.worker_profile !== undefined || raw.reviewer_profile !== undefined)
      ? {
          workerProfile: raw.worker_profile,
          reviewerProfile: raw.reviewer_profile,
        }
      : undefined,
  };
}

/**
 * Default RalphX workflow with 5 columns
 * Maps to the standard kanban board structure
 * Multi-state columns use groups to provide visibility into task state
 */
export const defaultWorkflow: WorkflowSchema = {
  id: "ralphx-default",
  name: "RalphX Default",
  description: "Standard kanban workflow for AI-driven development",
  columns: [
    { id: "draft", name: "Draft", mapsTo: "backlog" },
    {
      id: "ready",
      name: "Ready",
      mapsTo: "ready",
      groups: [
        {
          id: "fresh",
          label: "Fresh Tasks",
          statuses: ["ready"],
          canDragFrom: true,
          canDropTo: true,
        },
        {
          id: "needs_revision",
          label: "Needs Revision",
          statuses: ["revision_needed"],
          icon: "RotateCcw",
          accentColor: "hsl(var(--warning))",
          canDragFrom: true,
          canDropTo: false, // Only review process can add here
        },
        {
          id: "blocked",
          label: "Blocked",
          statuses: ["blocked"],
          icon: "Ban",
          accentColor: "hsl(var(--warning))",
          canDragFrom: true,
          canDropTo: true,
        },
        {
          id: "paused",
          label: "Paused",
          statuses: ["paused"],
          icon: "Pause",
          accentColor: "hsl(var(--warning))",
          canDragFrom: false, // User must resume via execution bar
          canDropTo: false,
        },
      ],
    },
    {
      id: "in_progress",
      name: "In Progress",
      mapsTo: "executing",
      groups: [
        {
          id: "first_attempt",
          label: "First Attempt",
          statuses: ["executing"],
          canDragFrom: false, // System-managed (agent working)
          canDropTo: false,
        },
        {
          id: "revising",
          label: "Revising",
          statuses: ["re_executing"],
          icon: "RefreshCw",
          accentColor: "hsl(var(--warning))",
          canDragFrom: false, // System-managed (agent revising)
          canDropTo: false,
        },
      ],
    },
    {
      id: "in_review",
      name: "In Review",
      mapsTo: "pending_review",
      groups: [
        {
          id: "waiting_ai",
          label: "Waiting for AI",
          statuses: ["pending_review"],
          icon: "Clock",
          canDragFrom: false, // System-managed
          canDropTo: false,
        },
        {
          id: "ai_reviewing",
          label: "AI Reviewing",
          statuses: ["reviewing"],
          icon: "Bot",
          accentColor: "hsl(var(--primary))",
          canDragFrom: false, // System-managed (AI working)
          canDropTo: false,
        },
        {
          id: "ready_approval",
          label: "Ready for Approval",
          statuses: ["review_passed"],
          icon: "CheckCircle",
          accentColor: "hsl(var(--success))",
          canDragFrom: false, // User interacts via Approve/Revise buttons
          canDropTo: false,
        },
        {
          id: "escalated",
          label: "Escalated",
          statuses: ["escalated"],
          icon: "AlertTriangle",
          accentColor: "hsl(var(--warning))",
          canDragFrom: false, // User interacts via Approve/Request Changes buttons
          canDropTo: false,
        },
      ],
    },
    {
      id: "done",
      name: "Done",
      mapsTo: "approved",
      groups: [
        {
          id: "merging",
          label: "Merging",
          statuses: ["pending_merge", "merging", "waiting_on_pr"],
          icon: "GitMerge",
          canDragFrom: false, // System-managed
          canDropTo: false,
        },
        {
          id: "needs_attention",
          label: "Escalated",
          statuses: ["merge_incomplete", "merge_conflict"],
          icon: "AlertTriangle",
          accentColor: "hsl(var(--warning))",
          canDragFrom: false, // User interacts via action buttons
          canDropTo: false,
        },
        {
          id: "completed",
          label: "Completed",
          statuses: ["merged", "approved"],
          icon: "CheckCircle",
          accentColor: "hsl(var(--success))",
          canDragFrom: false,
          canDropTo: false,
        },
        {
          id: "cancelled",
          label: "Cancelled",
          statuses: ["cancelled"],
          icon: "XCircle",
          canDragFrom: false,
          canDropTo: false,
        },
        {
          id: "failed",
          label: "Failed",
          statuses: ["failed"],
          icon: "XOctagon",
          accentColor: "hsl(var(--destructive))",
          canDragFrom: false,
          canDropTo: false,
        },
        {
          id: "stopped",
          label: "Stopped",
          statuses: ["stopped"],
          icon: "StopCircle",
          accentColor: "hsl(var(--destructive))",
          canDragFrom: false, // Requires manual restart
          canDropTo: false,
        },
      ],
    },
  ],
  isDefault: true,
};

/**
 * Jira-compatible workflow with 5 columns
 * Matches familiar Jira board structure with external sync config
 */
export const jiraCompatibleWorkflow: WorkflowSchema = {
  id: "jira-compat",
  name: "Jira Compatible",
  description: "Jira-style workflow with familiar columns",
  columns: [
    { id: "backlog", name: "Backlog", mapsTo: "backlog" },
    { id: "selected", name: "Selected for Dev", mapsTo: "ready" },
    { id: "in_progress", name: "In Progress", mapsTo: "executing" },
    { id: "in_qa", name: "In QA", mapsTo: "pending_review" },
    { id: "done", name: "Done", mapsTo: "approved" },
  ],
  externalSync: {
    provider: "jira",
    mapping: {},
    sync: {
      direction: "bidirectional",
      webhook: true,
    },
    conflictResolution: "external_wins",
  },
  isDefault: false,
};

/**
 * Linear-compatible workflow with state mapping for webhook reconciliation
 */
export const linearCompatibleWorkflow: WorkflowSchema = {
  id: "linear-compat",
  name: "Linear Compatible",
  description: "Linear-style workflow with external state mapping",
  columns: [
    { id: "backlog", name: "Backlog", mapsTo: "backlog" },
    { id: "todo", name: "Todo", mapsTo: "ready" },
    { id: "in_progress", name: "In Progress", mapsTo: "executing" },
    { id: "in_review", name: "In Review", mapsTo: "pending_review" },
    { id: "done", name: "Done", mapsTo: "approved" },
    { id: "canceled", name: "Canceled", mapsTo: "cancelled" },
  ],
  externalSync: {
    provider: "linear",
    mapping: {
      Backlog: {
        externalStatus: "Backlog",
        internalStatus: "backlog",
        columnId: "backlog",
      },
      Triage: {
        externalStatus: "Triage",
        internalStatus: "backlog",
        columnId: "backlog",
      },
      Todo: {
        externalStatus: "Todo",
        internalStatus: "ready",
        columnId: "todo",
      },
      "To Do": {
        externalStatus: "To Do",
        internalStatus: "ready",
        columnId: "todo",
      },
      "In Progress": {
        externalStatus: "In Progress",
        internalStatus: "executing",
        columnId: "in_progress",
      },
      "In Review": {
        externalStatus: "In Review",
        internalStatus: "pending_review",
        columnId: "in_review",
      },
      Review: {
        externalStatus: "Review",
        internalStatus: "pending_review",
        columnId: "in_review",
      },
      Done: {
        externalStatus: "Done",
        internalStatus: "approved",
        columnId: "done",
      },
      Canceled: {
        externalStatus: "Canceled",
        internalStatus: "cancelled",
        columnId: "canceled",
      },
      Cancelled: {
        externalStatus: "Cancelled",
        internalStatus: "cancelled",
        columnId: "canceled",
      },
    },
    sync: {
      direction: "bidirectional",
      webhook: true,
    },
    conflictResolution: "external_wins",
  },
  isDefault: false,
};

/**
 * Built-in workflows array for easy iteration
 */
export const BUILTIN_WORKFLOWS: readonly WorkflowSchema[] = [
  defaultWorkflow,
  jiraCompatibleWorkflow,
  linearCompatibleWorkflow,
] as const;

/**
 * Get a built-in workflow by ID
 */
export function getBuiltinWorkflow(id: string): WorkflowSchema | undefined {
  return BUILTIN_WORKFLOWS.find((w) => w.id === id);
}
