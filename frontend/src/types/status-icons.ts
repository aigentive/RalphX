/**
 * status-icons.ts - Shared status styling configuration
 *
 * Single source of truth for status colors, icons, and borders used by:
 * - TaskCard (Kanban board) - icons and borders
 * - TaskNode (Graph view) - icons
 * - TaskStatusBadge - icons
 *
 * Each status gets: icon, color, background opacity, label, and optional border highlight.
 * Colors reference design-system tokens so they flip with the active theme (Dark/Light/HC).
 */

import {
  Clock,
  Play,
  Ban,
  Loader2,
  RotateCcw,
  CheckCircle,
  AlertTriangle,
  AlertCircle,
  GitMerge,
  GitPullRequest,
  XCircle,
  XOctagon,
  Archive,
  FileText,
} from "lucide-react";
import type { InternalStatus } from "./status";

// ============================================================================
// Types
// ============================================================================

export interface StatusIconConfig {
  /** Lucide icon component */
  icon: React.ComponentType<{ className?: string }>;
  /** Icon/text color — design-system token reference (e.g. `var(--status-info)`) */
  color: string;
  /** Background opacity (0-1 as string, e.g., "0.2") */
  bgOpacity: string;
  /** Short display label */
  label: string;
  /** Whether icon should spin (for active states) */
  animate?: boolean;
}

// ============================================================================
// Status Icon Configuration
// ============================================================================

export const STATUS_ICON_CONFIG: Record<InternalStatus, StatusIconConfig> = {
  // === Idle States ===
  backlog: {
    icon: FileText,
    color: "var(--text-muted)",
    bgOpacity: "0.15",
    label: "Draft",
  },
  ready: {
    icon: Play,
    color: "var(--status-info)",
    bgOpacity: "0.15",
    label: "Ready",
  },

  // === Blocked ===
  blocked: {
    icon: Ban,
    color: "var(--status-warning)",
    bgOpacity: "0.2",
    label: "Blocked",
  },

  // === Execution States ===
  executing: {
    icon: Loader2,
    color: "var(--accent-primary)",
    bgOpacity: "0.2",
    label: "Executing",
    animate: true,
  },
  re_executing: {
    icon: Loader2,
    color: "var(--status-warning)",
    bgOpacity: "0.2",
    label: "Revising",
    animate: true,
  },

  // === QA States (informational / distinct from execution) ===
  qa_refining: {
    icon: Loader2,
    color: "var(--status-info)",
    bgOpacity: "0.2",
    label: "QA",
    animate: true,
  },
  qa_testing: {
    icon: Loader2,
    color: "var(--status-info)",
    bgOpacity: "0.2",
    label: "Testing",
    animate: true,
  },
  qa_passed: {
    icon: CheckCircle,
    color: "var(--status-info)",
    bgOpacity: "0.2",
    label: "QA ✓",
  },
  qa_failed: {
    icon: XCircle,
    color: "var(--status-info)",
    bgOpacity: "0.2",
    label: "QA ✗",
  },

  // === Review States ===
  pending_review: {
    icon: Clock,
    color: "var(--status-info)",
    bgOpacity: "0.2",
    label: "Pending",
  },
  reviewing: {
    icon: Loader2,
    color: "var(--status-info)",
    bgOpacity: "0.2",
    label: "Reviewing",
    animate: true,
  },
  review_passed: {
    icon: CheckCircle,
    color: "var(--status-success)",
    bgOpacity: "0.2",
    label: "Approved",
  },
  escalated: {
    icon: AlertTriangle,
    color: "var(--status-warning)",
    bgOpacity: "0.2",
    label: "Escalated",
  },
  revision_needed: {
    icon: RotateCcw,
    color: "var(--status-warning)",
    bgOpacity: "0.2",
    label: "Revision",
  },

  // === Merge States ===
  pending_merge: {
    icon: GitPullRequest,
    color: "var(--status-info)",
    bgOpacity: "0.2",
    label: "Merge",
  },
  merging: {
    icon: Loader2,
    color: "var(--status-info)",
    bgOpacity: "0.2",
    label: "Merging",
    animate: true,
  },
  waiting_on_pr: {
    icon: GitPullRequest,
    color: "var(--status-info)",
    bgOpacity: "0.2",
    label: "Waiting on PR",
  },
  merge_incomplete: {
    icon: AlertTriangle,
    color: "var(--status-warning)",
    bgOpacity: "0.2",
    label: "Incomplete",
  },
  merge_conflict: {
    icon: AlertCircle,
    color: "var(--status-error)",
    bgOpacity: "0.2",
    label: "Conflict",
  },
  updating_plan_branch: {
    icon: Loader2,
    color: "var(--status-info)",
    bgOpacity: "0.2",
    label: "Updating Plan Branch",
    animate: true,
  },
  updating_task_branch: {
    icon: Loader2,
    color: "var(--status-info)",
    bgOpacity: "0.2",
    label: "Updating Task Branch",
    animate: true,
  },
  branch_update_blocked: {
    icon: AlertTriangle,
    color: "var(--status-warning)",
    bgOpacity: "0.2",
    label: "Branch Update Blocked",
  },

  // === Complete States ===
  approved: {
    icon: CheckCircle,
    color: "var(--status-success)",
    bgOpacity: "0.2",
    label: "Done",
  },
  merged: {
    icon: GitMerge,
    color: "var(--status-success)",
    bgOpacity: "0.2",
    label: "Merged",
  },

  // === Terminal States ===
  failed: {
    icon: XOctagon,
    color: "var(--status-error)",
    bgOpacity: "0.2",
    label: "Failed",
  },
  cancelled: {
    icon: XCircle,
    color: "var(--text-muted)",
    bgOpacity: "0.15",
    label: "Cancelled",
  },
  stopped: {
    icon: XOctagon,
    color: "var(--status-error)",
    bgOpacity: "0.2",
    label: "Stopped",
  },

  // === Suspended States ===
  paused: {
    icon: Clock,
    color: "var(--status-warning)",
    bgOpacity: "0.2",
    label: "Paused",
  },
};

// ============================================================================
// Special Icon Configs (not tied to InternalStatus)
// ============================================================================

/** Archived task icon config */
export const ARCHIVED_ICON_CONFIG: StatusIconConfig = {
  icon: Archive,
  color: "var(--text-muted)",
  bgOpacity: "0.1",
  label: "Archived",
};

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Get icon config for a status with fallback
 */
export function getStatusIconConfig(status: InternalStatus | string): StatusIconConfig {
  const config = STATUS_ICON_CONFIG[status as InternalStatus];
  if (config) return config;

  return {
    icon: Clock,
    color: "var(--text-muted)",
    bgOpacity: "0.15",
    label: status,
  };
}

/**
 * Check if a status icon should animate (spin)
 */
export function shouldAnimateIcon(status: InternalStatus | string): boolean {
  const config = STATUS_ICON_CONFIG[status as InternalStatus];
  return config?.animate ?? false;
}

/**
 * Get status color for left border stripe (50% opacity).
 * Used by Kanban TaskCard and Graph TaskNode for visual consistency.
 * Uses color-mix so it resolves theme tokens at render time.
 */
export function getStatusBorderColor(status: InternalStatus | string, isArchived = false): string {
  const color = isArchived
    ? ARCHIVED_ICON_CONFIG.color
    : getStatusIconConfig(status).color;
  return `color-mix(in srgb, ${color} 50%, transparent)`;
}
