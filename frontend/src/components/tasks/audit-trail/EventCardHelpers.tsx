/* eslint-disable react-refresh/only-export-components -- pure constants/helpers file, no HMR components */
/**
 * EventCardHelpers - Shared constants, helpers, and sub-components for EventCards.
 */

import { useState, useCallback } from "react";
import {
  MessageSquare,
  Terminal,
  Brain,
  AlertCircle,
  ChevronDown,
  ChevronRight,
  Settings,
} from "lucide-react";

import type { AuditEntry } from "./EventCards";
import {
  STATUS_TOKEN_REFS,
  statusTint,
  withAlpha,
  type StatusTokenKey,
} from "@/lib/theme-colors";

// ============================================================================
// Style constants
// ============================================================================

type AuditStatusColor = StatusTokenKey | "muted";

const STATUS_COLOR_KEYS: Record<string, { label: string; color: AuditStatusColor }> = {
  backlog: { label: "Backlog", color: "muted" },
  ready: { label: "Ready", color: "info" },
  blocked: { label: "Blocked", color: "warning" },
  executing: { label: "Executing", color: "accent" },
  qa_refining: { label: "QA Refining", color: "accent" },
  qa_testing: { label: "QA Testing", color: "accent" },
  qa_passed: { label: "QA Passed", color: "success" },
  qa_failed: { label: "QA Failed", color: "error" },
  pending_review: { label: "Pending Review", color: "muted" },
  revision_needed: { label: "Revision Needed", color: "warning" },
  approved: { label: "Approved", color: "success" },
  failed: { label: "Failed", color: "error" },
  cancelled: { label: "Cancelled", color: "muted" },
  reviewing: { label: "Reviewing", color: "info" },
  review_passed: { label: "Review Passed", color: "success" },
  escalated: { label: "Escalated", color: "warning" },
  re_executing: { label: "Re-executing", color: "warning" },
  pending_merge: { label: "Pending Merge", color: "accent" },
  merging: { label: "Merging", color: "accent" },
  waiting_on_pr: { label: "Waiting on PR", color: "info" },
  merge_incomplete: { label: "Merge Incomplete", color: "warning" },
  merge_conflict: { label: "Merge Conflict", color: "warning" },
  merged: { label: "Merged", color: "success" },
  paused: { label: "Paused", color: "warning" },
  stopped: { label: "Stopped", color: "error" },
};

function resolveAuditStatusColor(color: AuditStatusColor): string {
  return color === "muted" ? "var(--text-muted)" : STATUS_TOKEN_REFS[color];
}

function resolveAuditStatusTint(color: AuditStatusColor, alpha: number): string {
  if (color === "muted") {
    return `color-mix(in srgb, var(--text-muted) ${alpha}%, transparent)`;
  }
  return statusTint(color, alpha);
}

export const STATUS_COLORS: Record<
  string,
  { label: string; color: string; bgColor: string }
> = Object.fromEntries(
  Object.entries(STATUS_COLOR_KEYS).map(([key, { label, color }]) => [
    key,
    {
      label,
      color: resolveAuditStatusColor(color),
      bgColor: resolveAuditStatusTint(color, 15),
    },
  ]),
);

/**
 * Per-status helper used by <StatusBadge/> to compose a matching border alpha
 * against the status color. Keeps border in-token for theme flippability.
 */
function statusBorderColor(statusKey: string): string {
  const entry = STATUS_COLOR_KEYS[statusKey];
  return resolveAuditStatusTint(entry?.color ?? "muted", 25);
}

export const SOURCE_STYLES: Record<
  AuditEntry["source"],
  { label: string; bg: string; border: string; color: string }
> = {
  transition: {
    label: "Transition",
    bg: statusTint("info", 12),
    border: statusTint("info", 25),
    color: STATUS_TOKEN_REFS.info,
  },
  review: {
    label: "Review",
    bg: statusTint("success", 12),
    border: statusTint("success", 25),
    color: STATUS_TOKEN_REFS.success,
  },
  activity: {
    label: "Activity",
    bg: statusTint("accent", 12),
    border: statusTint("accent", 25),
    color: "var(--accent-primary)",
  },
};

export const REVIEW_OUTCOME_CONFIG: Record<string, { color: string; bgColor: string }> = {
  Approved: { color: "var(--status-success)", bgColor: statusTint("success", 12) },
  "Changes Requested": { color: "var(--status-warning)", bgColor: statusTint("warning", 12) },
  Rejected: { color: "var(--status-error)", bgColor: statusTint("error", 12) },
};

export const ACTIVITY_TYPE_CONFIG: Record<
  string,
  { icon: typeof MessageSquare; color: string }
> = {
  text: { icon: MessageSquare, color: "var(--text-muted)" },
  tool_call: { icon: Terminal, color: "var(--accent-primary)" },
  tool_result: { icon: Terminal, color: "var(--text-secondary)" },
  thinking: { icon: Brain, color: "var(--text-muted)" },
  error: { icon: AlertCircle, color: "var(--status-error)" },
  system: { icon: Settings, color: "var(--text-muted)" },
};

export const DEFAULT_ACTIVITY_CONFIG = { icon: MessageSquare, color: "var(--text-muted)" };

export const CARD_STYLE = {
  backgroundColor: "var(--overlay-scrim)",
  border: "1px solid var(--overlay-weak)",
};

export const ERROR_CARD_STYLE = {
  backgroundColor: statusTint("error", 6),
  border: `1px solid ${statusTint("error", 15)}`,
  borderLeft: `3px solid ${statusTint("error", 50)}`,
};

export const CONTENT_TRUNCATE_LENGTH = 200;
export const THINKING_TRUNCATE_LENGTH = 100;

// ============================================================================
// Helpers
// ============================================================================

export function formatTimestamp(dateString: string): string {
  try {
    return new Date(dateString).toLocaleString(undefined, {
      year: "numeric",
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    });
  } catch {
    return dateString;
  }
}

export function extractToolName(description: string): string | null {
  try {
    const parsed = JSON.parse(description) as Record<string, unknown>;
    if (typeof parsed.tool === "string") return parsed.tool;
    if (typeof parsed.name === "string") return parsed.name;
  } catch {
    // not JSON — fall through
  }
  return null;
}

// ============================================================================
// Shared sub-components
// ============================================================================

export function ExpandableContent({
  text,
  maxLength = CONTENT_TRUNCATE_LENGTH,
  italic = false,
}: {
  text: string;
  maxLength?: number;
  italic?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const needsTruncation = text.length > maxLength;
  const displayText =
    !expanded && needsTruncation ? text.slice(0, maxLength) + "..." : text;
  const toggle = useCallback(() => setExpanded((p) => !p), []);

  return (
    <div className="mt-1">
      <p
        className={`text-[0.75rem] whitespace-pre-wrap break-words text-text-primary/60${italic ? " italic" : ""}`}
        style={{ lineHeight: "1.5" }}
      >
        {displayText}
      </p>
      {needsTruncation && (
        <button
          onClick={toggle}
          className="flex items-center gap-0.5 mt-0.5 text-[0.6875rem]"
          style={{ color: "var(--accent-primary)" }}
        >
          {expanded ? (
            <>
              <ChevronDown className="w-3 h-3" /> Show less
            </>
          ) : (
            <>
              <ChevronRight className="w-3 h-3" /> Show more
            </>
          )}
        </button>
      )}
    </div>
  );
}

export function StatusBadge({ status }: { status: string }) {
  const config = STATUS_COLORS[status] ?? {
    label: status,
    color: "var(--text-muted)",
    bgColor: resolveAuditStatusTint("muted", 15),
  };
  const borderRef = STATUS_COLOR_KEYS[status]
    ? statusBorderColor(status)
    : withAlpha("var(--text-muted)", 25);
  return (
    <span
      className="rounded-full px-2 py-0.5 text-[0.625rem] font-semibold"
      style={{
        backgroundColor: config.bgColor,
        color: config.color,
        border: `1px solid ${borderRef}`,
      }}
    >
      {config.label}
    </span>
  );
}
