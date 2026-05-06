/**
 * nodeStyles.ts - Status color mapping for Task Graph nodes
 *
 * Defines border and background colors for all 23 internal status states.
 * Colors are grouped by status category per the visual design spec.
 *
 * Status Groups:
 * - Idle: backlog, ready (muted gray)
 * - Blocked: blocked (amber)
 * - Executing: executing, re_executing (accent orange + glow)
 * - QA: qa_refining, qa_testing, qa_passed, qa_failed (purple)
 * - Review: pending_review, reviewing, review_passed, escalated, revision_needed (blue)
 * - Merge: pending_merge, merging, merge_conflict (cyan)
 * - Complete: approved, merged (green)
 * - Terminal: failed, cancelled (red)
 */

import type { InternalStatus } from "@/types/status";
import { useThemeStore, type FontScale } from "@/stores/themeStore";

// ============================================================================
// Types
// ============================================================================

export interface NodeStyle {
  /** Border color (solid HSL value) */
  borderColor: string;
  /** Background color (HSL with alpha) */
  backgroundColor: string;
  /** Optional box-shadow for glow effect */
  boxShadow?: string;
  /** Optional CSS animation for active states */
  animation?: string;
}

/**
 * Glass morphism surface styles for consistent node styling
 */
export interface GlassSurface {
  /** Glass morphism background */
  background: string;
  /** Backdrop filter for blur effect */
  backdropFilter: string;
  /** Webkit prefix for backdrop filter (Safari support) */
  WebkitBackdropFilter: string;
  /** Subtle divider border */
  border: string;
  /** Consistent soft shadow */
  boxShadow: string;
}

// ============================================================================
// Node Dimensions (Single Source of Truth)
// ============================================================================

/** Standard node width - full-size TaskNode with status badges */
export const NODE_WIDTH = 240;

/** Standard node height to accommodate: title (2-line) + description (2-line) + category + progress bar */
export const NODE_HEIGHT = 140;

/** Compact node width - smaller TaskNodeCompact for large graphs (50+ tasks) */
export const COMPACT_NODE_WIDTH = 220;

/** Compact node height - smaller for dense layouts */
export const COMPACT_NODE_HEIGHT = 60;

/** Maximum title characters for compact nodes */
export const COMPACT_TITLE_MAX_CHARS = 40;

/** Multiplier applied to node dimensions per font-scale toggle. Keeps the
 * fixed-pixel React Flow layout in sync with the rem-based text scale so
 * titles/descriptions don't overflow when the user picks 110% / 125%. */
const FONT_SCALE_NODE_MULTIPLIER: Record<FontScale, number> = {
  default: 1,
  lg: 1.1,
  xl: 1.25,
};

export interface ScaledNodeDimensions {
  nodeWidth: number;
  nodeHeight: number;
  compactNodeWidth: number;
  compactNodeHeight: number;
}

export function getScaledNodeDimensions(scale: FontScale): ScaledNodeDimensions {
  const k = FONT_SCALE_NODE_MULTIPLIER[scale];
  return {
    nodeWidth: Math.round(NODE_WIDTH * k),
    nodeHeight: Math.round(NODE_HEIGHT * k),
    compactNodeWidth: Math.round(COMPACT_NODE_WIDTH * k),
    compactNodeHeight: Math.round(COMPACT_NODE_HEIGHT * k),
  };
}

/** React hook — re-renders when the user changes the font-scale toggle. */
export function useScaledNodeDimensions(): ScaledNodeDimensions {
  const fontScale = useThemeStore((s) => s.fontScale);
  return getScaledNodeDimensions(fontScale);
}

// ============================================================================
// Glass Morphism Constants
// ============================================================================

/**
 * Base glass morphism surface - shared by all nodes
 * Matches Kanban card styling for visual consistency
 */
export const GLASS_SURFACE: GlassSurface = {
  background: "color-mix(in srgb, var(--bg-elevated) 85%, transparent)",
  backdropFilter: "blur(12px) saturate(150%)",
  WebkitBackdropFilter: "blur(12px) saturate(150%)",
  border: "1px solid var(--overlay-weak)",
  boxShadow: "0 2px 8px var(--overlay-scrim)",
};

// ============================================================================
// Priority Stripe Colors
// ============================================================================

/**
 * Priority colors for left border stripe
 * Matches Kanban card styling (from TaskCard.utils.ts)
 */
const PRIORITY_COLORS: Record<number, string> = {
  1: "var(--status-error)",    // P1 Critical
  2: "var(--accent-secondary)", // P2 High — soft amber
  3: "var(--accent-primary)",  // P3 Medium — brand orange
  4: "var(--text-muted)",      // P4 Low
};

/**
 * Get priority color for the left border stripe
 * Falls back to transparent if priority is undefined or out of range
 */
export function getPriorityStripeColor(priority: number | undefined): string {
  if (priority === undefined || priority < 1 || priority > 4) {
    return "transparent";
  }
  return PRIORITY_COLORS[priority] ?? "transparent";
}

export type StatusCategory =
  | "idle"
  | "blocked"
  | "executing"
  | "qa"
  | "review"
  | "merge"
  | "complete"
  | "terminal";

// ============================================================================
// Color Definitions by Category
// ============================================================================

/** Idle statuses: backlog, ready */
const IDLE_COLORS = {
  border: "var(--text-muted)",
  background: "color-mix(in srgb, var(--bg-elevated) 80%, transparent)",
} as const;

/** Blocked status: blocked */
const BLOCKED_COLORS = {
  border: "var(--status-warning)",
  background: "var(--status-warning-muted)",
} as const;

/** Executing statuses: executing, re_executing */
const EXECUTING_COLORS = {
  border: "var(--accent-primary)",
  background: "var(--accent-muted)",
  boxShadow: "0 0 12px color-mix(in srgb, var(--accent-primary) 30%, transparent)",
  animation: "var(--animation-executing-pulse)",
} as const;

/** QA statuses: qa_refining, qa_testing, qa_passed, qa_failed */
const QA_COLORS = {
  border: "var(--status-info)",
  background: "var(--status-info-muted)",
} as const;

/** Review statuses: pending_review, reviewing, review_passed, escalated, revision_needed */
const REVIEW_COLORS = {
  border: "var(--status-info)",
  background: "var(--status-info-muted)",
} as const;

/** Reviewing status: active review with pulse animation */
const REVIEWING_ANIMATION = "var(--animation-reviewing-pulse)";

/** Merge statuses: pending_merge, merging, waiting_on_pr, merge_conflict */
const MERGE_COLORS = {
  border: "var(--status-info)",
  background: "var(--status-info-muted)",
} as const;

/** Complete statuses: approved, merged */
const COMPLETE_COLORS = {
  border: "var(--status-success)",
  background: "var(--status-success-muted)",
} as const;

/** Terminal statuses: failed, cancelled */
const TERMINAL_COLORS = {
  border: "var(--status-error)",
  background: "var(--status-error-muted)",
} as const;

// ============================================================================
// Status to Category Mapping
// ============================================================================

/**
 * Maps each internal status to its category
 */
export function getStatusCategory(status: InternalStatus): StatusCategory {
  switch (status) {
    case "backlog":
    case "ready":
      return "idle";
    case "blocked":
      return "blocked";
    case "executing":
    case "re_executing":
      return "executing";
    case "qa_refining":
    case "qa_testing":
    case "qa_passed":
    case "qa_failed":
      return "qa";
    case "pending_review":
    case "reviewing":
    case "review_passed":
    case "escalated":
    case "revision_needed":
      return "review";
    case "pending_merge":
    case "merging":
    case "waiting_on_pr":
    case "merge_conflict":
    case "merge_incomplete":
      return "merge";
    case "approved":
    case "merged":
      return "complete";
    case "failed":
    case "cancelled":
    case "stopped":
      return "terminal";
    case "paused":
      return "blocked";
    default: {
      const exhaustiveCheck: never = status;
      return exhaustiveCheck;
    }
  }
}

// ============================================================================
// Style Getters
// ============================================================================

/**
 * Get the complete node style for a given status
 */
export function getNodeStyle(status: InternalStatus | string): NodeStyle {
  // Type guard for unknown status strings (e.g., from API before validation)
  const safeStatus = status as InternalStatus;

  switch (safeStatus) {
    // Idle
    case "backlog":
    case "ready":
      return {
        borderColor: IDLE_COLORS.border,
        backgroundColor: IDLE_COLORS.background,
      };

    // Blocked
    case "blocked":
      return {
        borderColor: BLOCKED_COLORS.border,
        backgroundColor: BLOCKED_COLORS.background,
      };

    // Executing
    case "executing":
    case "re_executing":
      return {
        borderColor: EXECUTING_COLORS.border,
        backgroundColor: EXECUTING_COLORS.background,
        boxShadow: EXECUTING_COLORS.boxShadow,
        animation: EXECUTING_COLORS.animation,
      };

    // QA
    case "qa_refining":
    case "qa_testing":
    case "qa_passed":
    case "qa_failed":
      return {
        borderColor: QA_COLORS.border,
        backgroundColor: QA_COLORS.background,
      };

    // Review
    case "pending_review":
    case "review_passed":
    case "escalated":
    case "revision_needed":
      return {
        borderColor: REVIEW_COLORS.border,
        backgroundColor: REVIEW_COLORS.background,
      };

    // Reviewing (active review with animation)
    case "reviewing":
      return {
        borderColor: REVIEW_COLORS.border,
        backgroundColor: REVIEW_COLORS.background,
        animation: REVIEWING_ANIMATION,
      };

    // Merge
    case "pending_merge":
    case "merging":
    case "waiting_on_pr":
    case "merge_conflict":
      return {
        borderColor: MERGE_COLORS.border,
        backgroundColor: MERGE_COLORS.background,
      };

    // Complete
    case "approved":
    case "merged":
      return {
        borderColor: COMPLETE_COLORS.border,
        backgroundColor: COMPLETE_COLORS.background,
      };

    // Terminal
    case "failed":
    case "cancelled":
    case "stopped":
      return {
        borderColor: TERMINAL_COLORS.border,
        backgroundColor: TERMINAL_COLORS.background,
      };

    // Paused (like blocked, amber)
    case "paused":
      return {
        borderColor: BLOCKED_COLORS.border,
        backgroundColor: BLOCKED_COLORS.background,
      };

    // Fallback for unknown statuses
    default:
      return {
        borderColor: IDLE_COLORS.border,
        backgroundColor: IDLE_COLORS.background,
      };
  }
}

/**
 * Get only the border color for a status
 * Useful for MiniMap and edge styling
 */
export function getStatusBorderColor(status: InternalStatus | string): string {
  return getNodeStyle(status).borderColor;
}

/**
 * Get only the background color for a status
 */
export function getStatusBackground(status: InternalStatus | string): string {
  return getNodeStyle(status).backgroundColor;
}

// ============================================================================
// Legend Data (for GraphLegend component)
// ============================================================================

export interface LegendItem {
  status: InternalStatus;
  label: string;
  category: StatusCategory;
}

/**
 * Status groups for the legend, organized by category
 */
export const STATUS_LEGEND_GROUPS: Record<StatusCategory, LegendItem[]> = {
  idle: [
    { status: "backlog", label: "Backlog", category: "idle" },
    { status: "ready", label: "Ready", category: "idle" },
  ],
  blocked: [
    { status: "blocked", label: "Blocked", category: "blocked" },
    { status: "paused", label: "Paused", category: "blocked" },
  ],
  executing: [
    { status: "executing", label: "Executing", category: "executing" },
    { status: "re_executing", label: "Re-executing", category: "executing" },
  ],
  qa: [
    { status: "qa_refining", label: "QA Refining", category: "qa" },
    { status: "qa_testing", label: "QA Testing", category: "qa" },
    { status: "qa_passed", label: "QA Passed", category: "qa" },
    { status: "qa_failed", label: "QA Failed", category: "qa" },
  ],
  review: [
    { status: "pending_review", label: "Pending Review", category: "review" },
    { status: "reviewing", label: "Reviewing", category: "review" },
    { status: "review_passed", label: "Review Passed", category: "review" },
    { status: "escalated", label: "Escalated", category: "review" },
    { status: "revision_needed", label: "Revision Needed", category: "review" },
  ],
  merge: [
    { status: "pending_merge", label: "Pending Merge", category: "merge" },
    { status: "merging", label: "Merging", category: "merge" },
    { status: "waiting_on_pr", label: "Waiting on PR", category: "merge" },
    { status: "merge_conflict", label: "Merge Conflict", category: "merge" },
  ],
  complete: [
    { status: "approved", label: "Approved", category: "complete" },
    { status: "merged", label: "Merged", category: "complete" },
  ],
  terminal: [
    { status: "failed", label: "Failed", category: "terminal" },
    { status: "cancelled", label: "Cancelled", category: "terminal" },
    { status: "stopped", label: "Stopped", category: "terminal" },
  ],
};

/**
 * Category display names for legend headers
 */
export const CATEGORY_LABELS: Record<StatusCategory, string> = {
  idle: "Idle",
  blocked: "Blocked",
  executing: "Executing",
  qa: "QA",
  review: "Review",
  merge: "Merge",
  complete: "Complete",
  terminal: "Terminal",
};

/**
 * Get the border color for a category (for legend group headers)
 */
export function getCategoryColor(category: StatusCategory): string {
  switch (category) {
    case "idle":
      return IDLE_COLORS.border;
    case "blocked":
      return BLOCKED_COLORS.border;
    case "executing":
      return EXECUTING_COLORS.border;
    case "qa":
      return QA_COLORS.border;
    case "review":
      return REVIEW_COLORS.border;
    case "merge":
      return MERGE_COLORS.border;
    case "complete":
      return COMPLETE_COLORS.border;
    case "terminal":
      return TERMINAL_COLORS.border;
  }
}
