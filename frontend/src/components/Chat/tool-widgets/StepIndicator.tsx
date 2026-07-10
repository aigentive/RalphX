/**
 * StepIndicator — Ultra-compact single-line widget for step-related MCP tools.
 *
 * Handles: start_step, complete_step, add_step, skip_step, fail_step, get_step_progress
 *
 * Design reference: mockups/tool-call-widgets.html (Widgets 5 & 7)
 * - NOT cards — inline single-line elements to reduce visual noise
 * - start_step: pulsing orange dot + step title + "started" badge
 * - complete_step: green checkmark + step title + "completed" badge + truncated note
 * - add_step: blue plus + step title + "added" badge
 * - skip_step: gray skip icon + step title + "skipped" badge
 * - fail_step: red X + step title + "failed" badge
 * - get_step_progress: minimal progress indicator (N of M)
 */

import React from "react";
import { getStepProgressDisplay } from "@/types/task-step";
import { StepLine, Badge, InlineIndicator } from "./shared";
import { colors, getString, getNumber } from "./shared.constants";
import type { ToolCallWidgetProps, StepLineVariant } from "./shared.constants";
import { canonicalizeToolName } from "./tool-name";

// ============================================================================
// Argument / result extraction helpers
// ============================================================================

/** Extract step title from arguments or result (tries common field names) */
function extractStepTitle(toolCall: ToolCallWidgetProps["toolCall"]): string {
  const args = toolCall.arguments;
  // Try title from args first (add_step has it), then from result
  const fromArgs = getString(args, "title");
  if (fromArgs) return fromArgs;

  // Result may contain the step data with title
  const result = toolCall.result;
  const fromResult = getString(result, "title");
  if (fromResult) return fromResult;

  // Fallback: step_id if nothing else
  const stepId = getString(args, "step_id");
  if (stepId) return `Step ${stepId.slice(0, 8)}...`;

  return "Step";
}

/** Extract completion note from args or result */
function extractNote(toolCall: ToolCallWidgetProps["toolCall"]): string | undefined {
  const args = toolCall.arguments;
  // complete_step has note in args
  const noteFromArgs = getString(args, "note");
  if (noteFromArgs) return noteFromArgs;

  // Result may also contain a note
  const result = toolCall.result;
  const noteFromResult = getString(result, "completion_note") ?? getString(result, "note");
  if (noteFromResult) return noteFromResult;

  // skip_step has reason
  const reason = getString(args, "reason");
  if (reason) return reason;

  // fail_step has error
  const error = getString(args, "error");
  if (error) return error;

  return undefined;
}

/** Map tool name to StepLine variant */
function toolNameToVariant(toolName: string): StepLineVariant | null {
  switch (canonicalizeToolName(toolName)) {
    case "start_step":
      return "started";
    case "complete_step":
      return "completed";
    case "add_step":
      return "added";
    case "skip_step":
      return "skipped";
    case "fail_step":
      return "failed";
    default:
      return null;
  }
}

// ============================================================================
// Progress indicator for get_step_progress
// ============================================================================

interface StepProgressData {
  total: number;
  completed: number;
  inProgress: number;
  pending: number;
  skipped: number;
  failed: number;
}

/** Extract progress data from get_step_progress result */
function extractProgress(toolCall: ToolCallWidgetProps["toolCall"]): StepProgressData | null {
  const result = toolCall.result;
  if (result == null || typeof result !== "object") return null;

  const total = getNumber(result, "total");
  const completed = getNumber(result, "completed");
  if (total == null || completed == null) return null;

  return {
    total,
    completed,
    inProgress: getNumber(result, "in_progress") ?? 0,
    pending: getNumber(result, "pending") ?? 0,
    skipped: getNumber(result, "skipped") ?? 0,
    failed: getNumber(result, "failed") ?? 0,
  };
}

function StepProgressIndicator({ progress, compact }: { progress: StepProgressData; compact?: boolean }) {
  const display = getStepProgressDisplay(progress);
  const percentComplete = Math.round(display.completedPercent);
  const hasCompletableSteps = display.total > 0;
  const allDone = hasCompletableSteps && display.completed === display.total;
  const progressText = hasCompletableSteps
    ? `${display.completed}/${display.total} steps`
    : "No steps to complete";

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: compact ? "2px 10px" : "4px 10px",
        margin: "2px 0",
      }}
    >
      {/* Progress bar */}
      <div
        aria-label="Step progress"
        aria-valuemax={100}
        aria-valuemin={0}
        aria-valuenow={percentComplete}
        aria-valuetext={progressText}
        role="progressbar"
        style={{
          flex: 1,
          height: 3,
          borderRadius: 2,
          background: colors.border,
          overflow: "hidden",
        }}
      >
        <div
          style={{
            width: `${display.completedPercent}%`,
            height: "100%",
            borderRadius: 2,
            background: allDone ? colors.success : colors.accent,
            transition: "width 300ms ease",
          }}
        />
      </div>

      {/* Step count */}
      <span
        style={{
          fontSize: compact ? 9.5 : 10,
          color: allDone ? colors.success : colors.textMuted,
          fontWeight: 500,
          whiteSpace: "nowrap",
          flexShrink: 0,
        }}
      >
        {progressText}
      </span>

      {/* Percentage badge */}
      {hasCompletableSteps ? (
        <Badge variant={allDone ? "success" : "muted"} compact>
          {percentComplete}%
        </Badge>
      ) : null}
    </div>
  );
}

// ============================================================================
// Main StepIndicator widget
// ============================================================================

export const StepIndicator = React.memo(function StepIndicator({
  toolCall,
  compact = false,
}: ToolCallWidgetProps) {
  const toolName = canonicalizeToolName(toolCall.name);

  // Handle get_step_progress separately
  if (toolName === "get_step_progress") {
    const progress = extractProgress(toolCall);
    if (progress) {
      return <StepProgressIndicator progress={progress} compact={compact} />;
    }
    // Fallback if result not yet available
    return <InlineIndicator text="Checking step progress..." />;
  }

  // Map to variant for step lifecycle tools
  const variant = toolNameToVariant(toolName);
  if (!variant) {
    // Should not happen if registry is correct, but degrade gracefully
    return <InlineIndicator text={`${toolCall.name}`} />;
  }

  const title = extractStepTitle(toolCall);
  const note = extractNote(toolCall);

  return <StepLine variant={variant} title={title} note={note} compact={compact} />;
});
