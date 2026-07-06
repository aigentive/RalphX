/**
 * MergeWidget — Specialized renderers for merge-related MCP tools.
 *
 * Handles: complete_merge, report_conflict, report_incomplete, get_merge_target
 *
 * Design:
 * - complete_merge: status-aware card with commit SHA badge and branch info
 * - report_conflict: red card with conflict file list
 * - report_incomplete: amber/orange card with error details
 * - get_merge_target: compact card showing source → target branch arrows
 */

import React from "react";
import { GitMerge, GitBranch, AlertTriangle, ArrowRight, X } from "lucide-react";
import { WidgetCard, WidgetHeader, Badge, InlineIndicator, FilePath } from "./shared";
import { colors, getString, getStringArray, getBool } from "./shared.constants";
import type { ToolCallWidgetProps } from "./shared.constants";
import { canonicalizeToolName } from "./tool-name";

/** Shorten a commit SHA to 7 chars */
function shortSha(sha: string): string {
  return sha.length > 7 ? sha.slice(0, 7) : sha;
}

import { formatBranchDisplay } from "@/lib/branch-utils";

/** Extract branch short name using unified formatter */
function shortBranch(branch: string): string {
  return formatBranchDisplay(branch).short;
}

function isContinuationStatus(status: string | undefined): boolean {
  return status === "executing"
    || status === "re_executing"
    || status === "ready"
    || status === "reviewing"
    || status === "pending_review";
}

function continuationLabel(status: string | undefined): string | null {
  switch (status) {
    case "executing":
    case "re_executing":
      return "Task returned to execution after freshness resolution";
    case "ready":
      return "Task returned to ready state after freshness resolution";
    case "reviewing":
    case "pending_review":
      return "Task returned to review after freshness resolution";
    default:
      return null;
  }
}

// ============================================================================
// complete_merge — Status-aware merge result card
// ============================================================================

function CompleteMergeWidget({ toolCall, compact = false }: ToolCallWidgetProps) {
  const args = toolCall.arguments;
  const result = toolCall.result;

  const commitSha = getString(args, "commit_sha") ?? getString(result, "commit_sha");
  const success = getBool(result, "success");
  const message = getString(result, "message");
  const resultStatus = getString(result, "status");
  const resultReason = getString(result, "reason");
  const newStatus = getString(result, "new_status");
  const wasStopped = resultReason === "stopped";
  const wasAborted = resultStatus === "aborted" || wasStopped;
  const failed = success === false;
  const continuationStatus = isContinuationStatus(newStatus);
  const title = wasAborted
    ? wasStopped
      ? "Merge stopped"
      : "Merge aborted"
    : failed
    ? "Merge failed"
    : continuationStatus
    ? "Branch update applied"
    : newStatus === "already_merged"
    ? "Merge already applied"
    : "Merge completed";
  const stoppedDetail = wasStopped
    ? "Stopped before merge completion settled"
    : resultReason
    ? `Aborted before merge completion settled (${resultReason})`
    : "Aborted before merge completion settled";
  const detail = wasAborted ? message ?? stoppedDetail : continuationLabel(newStatus) ?? message;
  const accentColor = wasAborted
    ? "var(--status-warning)"
    : failed
    ? colors.error
    : continuationStatus
    ? colors.blue
    : colors.success;
  const surfaceTint = wasAborted
    ? "var(--status-warning-muted)"
    : failed
    ? colors.errorDim
    : continuationStatus
    ? colors.blueDim
    : colors.successDim;
  const detailColor = wasAborted
    ? "var(--status-warning)"
    : failed
    ? "var(--status-error)"
    : continuationStatus
    ? "var(--status-info)"
    : "var(--status-success)";
  const statusBadge = wasAborted ? resultStatus ?? resultReason : newStatus;
  const statusVariant = wasAborted
    ? "warning"
    : failed
    ? "error"
    : continuationStatus
    ? "blue"
    : "success";

  // If tool errored, show inline error
  if (toolCall.error) {
    return (
      <InlineIndicator
        icon={<X size={12} style={{ color: colors.error }} />}
        text={`Merge failed: ${toolCall.error}`}
      />
    );
  }

  // Result not available yet
  if (result == null) {
    return (
      <InlineIndicator
        icon={<GitMerge size={12} style={{ color: colors.textMuted }} />}
        text="Completing merge..."
      />
    );
  }

  return (
    <div
      style={{
        background: surfaceTint,
        borderRadius: 10,
        border: `1px solid color-mix(in srgb, ${accentColor} 20%, transparent)`,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: compact ? "6px 10px" : "8px 12px",
        }}
      >
        {/* Merge icon */}
        <GitMerge size={14} style={{ color: accentColor, flexShrink: 0 }} />

        {/* Title */}
        <span
          style={{
            fontSize: compact ? 11 : 11.5,
            fontWeight: 500,
            color: accentColor,
            flex: 1,
          }}
        >
          {title}
        </span>

        {/* Commit SHA badge */}
        {commitSha && (
          <span
            style={{
              fontSize: 9.5,
              fontFamily: "var(--font-mono)",
              padding: "1px 6px",
              borderRadius: 6,
              fontWeight: 500,
              background: surfaceTint,
              color: accentColor,
              flexShrink: 0,
            }}
          >
            {shortSha(commitSha)}
          </span>
        )}

        {/* Status badge */}
        {statusBadge && (
          <Badge variant={statusVariant} compact>{statusBadge}</Badge>
        )}
      </div>

      {/* Message detail */}
      {detail && (
        <div
          style={{
            fontSize: 10,
            color: detailColor,
            padding: compact ? "0 10px 6px" : "0 12px 8px",
            paddingTop: 0,
          }}
        >
          {detail}
        </div>
      )}
    </div>
  );
}

// ============================================================================
// report_conflict — Red card with conflict file list
// ============================================================================

function ReportConflictWidget({ toolCall, compact = false }: ToolCallWidgetProps) {
  const args = toolCall.arguments;

  const conflictFiles = getStringArray(args, "conflict_files") ?? [];
  const reason = getString(args, "reason");

  if (toolCall.error) {
    return (
      <InlineIndicator
        icon={<X size={12} style={{ color: colors.error }} />}
        text={`Report conflict failed: ${toolCall.error}`}
      />
    );
  }

  const fileCount = conflictFiles.length;

  return (
    <WidgetCard
      compact={compact}
      header={
        <WidgetHeader
          icon={<AlertTriangle size={14} style={{ color: colors.error }} />}
          title={reason ? `Conflict: ${reason}` : "Merge conflict"}
          badge={
            <Badge variant="error" compact>
              {fileCount} {fileCount === 1 ? "file" : "files"}
            </Badge>
          }
          compact={compact}
        />
      }
    >
      {/* Conflict file list */}
      <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        {conflictFiles.map((file, i) => (
          <div
            key={i}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "2px 0",
            }}
          >
            <X size={10} style={{ color: colors.error, flexShrink: 0 }} />
            <FilePath path={file} />
          </div>
        ))}
        {fileCount === 0 && (
          <span style={{ fontSize: 10.5, color: colors.textMuted }}>
            No conflict files listed
          </span>
        )}
      </div>
    </WidgetCard>
  );
}

// ============================================================================
// report_incomplete — Amber/orange card with error details
// ============================================================================

function ReportIncompleteWidget({ toolCall, compact = false }: ToolCallWidgetProps) {
  const args = toolCall.arguments;

  const reason = getString(args, "reason");
  const diagnosticInfo = getString(args, "diagnostic_info");

  if (toolCall.error) {
    return (
      <InlineIndicator
        icon={<X size={12} style={{ color: colors.error }} />}
        text={`Report incomplete failed: ${toolCall.error}`}
      />
    );
  }

  return (
    <WidgetCard
      compact={compact}
      header={
        <WidgetHeader
          icon={<AlertTriangle size={14} style={{ color: colors.accent }} />}
          title={reason ?? "Merge incomplete"}
          badge={<Badge variant="accent" compact>incomplete</Badge>}
          compact={compact}
        />
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        {/* Reason */}
        {reason && (
          <div style={{ fontSize: 11, color: colors.textSecondary, lineHeight: 1.4 }}>
            {reason}
          </div>
        )}

        {/* Diagnostic info (code block) */}
        {diagnosticInfo && (
          <pre
            style={{
              fontFamily: "var(--font-mono)",
              fontSize: 10,
              color: colors.textMuted,
              background: colors.bgTerminal,
              padding: "6px 8px",
              borderRadius: 6,
              overflow: "auto",
              maxHeight: compact ? 80 : 120,
              whiteSpace: "pre-wrap",
              wordBreak: "break-word",
              margin: 0,
            }}
          >
            {diagnosticInfo}
          </pre>
        )}
      </div>
    </WidgetCard>
  );
}

// ============================================================================
// get_merge_target — Compact source → target branch card
// ============================================================================

function GetMergeTargetWidget({ toolCall, compact = false }: ToolCallWidgetProps) {
  const result = toolCall.result;

  const sourceBranch = getString(result, "source_branch");
  const targetBranch = getString(result, "target_branch");

  if (toolCall.error) {
    return (
      <InlineIndicator
        icon={<X size={12} style={{ color: colors.error }} />}
        text={`Get merge target failed: ${toolCall.error}`}
      />
    );
  }

  // Not loaded yet
  if (!sourceBranch || !targetBranch) {
    return (
      <InlineIndicator
        icon={<GitBranch size={12} style={{ color: colors.textMuted }} />}
        text="Resolving merge target..."
      />
    );
  }

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: compact ? "4px 10px" : "6px 10px",
        margin: "2px 0",
      }}
    >
      {/* Git branch icon */}
      <GitBranch size={12} style={{ color: colors.blue, flexShrink: 0 }} />

      {/* Source branch */}
      <span
        style={{
          fontSize: compact ? 10 : 10.5,
          fontFamily: "var(--font-mono)",
          color: colors.textSecondary,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
        title={sourceBranch}
      >
        {shortBranch(sourceBranch)}
      </span>

      {/* Arrow */}
      <ArrowRight size={10} style={{ color: colors.textMuted, flexShrink: 0 }} />

      {/* Target branch */}
      <span
        style={{
          fontSize: compact ? 10 : 10.5,
          fontFamily: "var(--font-mono)",
          color: colors.textPrimary,
          fontWeight: 500,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
        title={targetBranch}
      >
        {shortBranch(targetBranch)}
      </span>

      {/* Merge badge */}
      <Badge variant="blue" compact>merge target</Badge>
    </div>
  );
}

// ============================================================================
// Main MergeWidget — Dispatcher
// ============================================================================

export const MergeWidget = React.memo(function MergeWidget(props: ToolCallWidgetProps) {
  const toolName = canonicalizeToolName(props.toolCall.name);

  switch (toolName) {
    case "complete_merge":
      return <div data-testid="merge-widget-complete"><CompleteMergeWidget {...props} /></div>;

    case "report_conflict":
      return <div data-testid="merge-widget-conflict"><ReportConflictWidget {...props} /></div>;

    case "report_incomplete":
      return <div data-testid="merge-widget-incomplete"><ReportIncompleteWidget {...props} /></div>;

    case "get_merge_target":
      return <div data-testid="merge-widget-target"><GetMergeTargetWidget {...props} /></div>;

    default:
      return (
        <div data-testid="merge-widget-fallback">
        <InlineIndicator
          icon={<GitMerge size={12} style={{ color: colors.textMuted }} />}
          text={props.toolCall.name}
        />
        </div>
      );
  }
});
