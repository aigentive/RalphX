/**
 * TaskSubagentCard - Renders a dedicated card for each running/completed Task subagent
 * during streaming.
 *
 * Features:
 * - Header: subagent type badge, description, model badge, running timer / completed duration
 * - Body (running): numbered list of child tool calls
 *   with Edit/Write calls rendered as inline DiffToolCallViews
 * - Body (completed): collapsed summary with duration, token count, tool use count
 * - Styling uses the shared task-card surface (bg-elevated, border-subtle, orange accent)
 */

import React, { useState, useRef, useEffect, useCallback } from "react";
import { ChevronDown, ChevronRight, Loader2, Bot } from "lucide-react";
import type { StreamingTask } from "@/types/streaming-task";
import { formatDuration } from "./tool-call-utils";
import {
  TaskCardKindBadge,
  TaskCardModelBadge,
  TaskCardProviderHarnessBadge,
  TaskCardProviderModelBadge,
  TaskCardRuntimeDetails,
  TaskCardStatusBadge,
  TaskCardSubagentTypeBadge,
  TaskCardSummary,
} from "./TaskCardShared";
import {
  TaskCardTranscriptView,
} from "./TaskCardTranscript";
import { buildTaskCardTranscriptEntryFromStreamingTask } from "./TaskCardTranscript.utils";
import { TaskToolCallDelegatedTranscript } from "./TaskToolCallDelegatedTranscript";
import { canonicalizeToolName } from "./tool-widgets/tool-name";
import { useDelegateCardExpansion } from "./useDelegateCardExpansion";

// ============================================================================
// Constants
// ============================================================================

/** Maximum height for the tool list content area */
const MAX_CONTENT_HEIGHT = 200;

/** Distance from bottom (px) within which we consider the user "near bottom" */
const NEAR_BOTTOM_THRESHOLD = 30;

// ============================================================================
// Types
// ============================================================================

interface TaskSubagentCardProps {
  task: StreamingTask;
}

// ============================================================================
// Running Timer Hook
// ============================================================================

function useElapsedTimer(startedAt: number, isRunning: boolean): string {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!isRunning) return;
    setNow(Date.now());
    const interval = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(interval);
  }, [isRunning]);

  if (!isRunning) return "";
  return formatDuration(now - startedAt);
}

// ============================================================================
// Sub-components
// ============================================================================

/** Completed state summary line */
function CompletedSummary({ task }: { task: StreamingTask }) {
  return (
    <TaskCardSummary
      metrics={{
        totalDurationMs: task.totalDurationMs,
        totalTokens: task.totalTokens,
        totalToolUseCount: task.totalToolUseCount,
        estimatedUsd: task.estimatedUsd,
      }}
      className="text-xs"
    />
  );
}

// ============================================================================
// Main Component
// ============================================================================

export const TaskSubagentCard = React.memo(function TaskSubagentCard({
  task,
}: TaskSubagentCardProps) {
  const isRunning = task.status === "running";
  const isCompleted = task.status === "completed";
  const isFailed = task.status === "failed";
  const isCancelled = task.status === "cancelled";
  const isDelegateCall = canonicalizeToolName(task.toolName) === "delegate_start";
  const { isExpanded, setIsExpanded } = useDelegateCardExpansion(
    task.delegatedJobId || task.toolUseId || "delegate",
    isDelegateCall,
  );
  const contentRef = useRef<HTMLDivElement>(null);
  const isNearBottomRef = useRef(true);

  const elapsed = useElapsedTimer(task.startedAt, isRunning);

  // Track scroll position to determine if user is near bottom
  const handleScroll = useCallback(() => {
    const el = contentRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    isNearBottomRef.current = distanceFromBottom <= NEAR_BOTTOM_THRESHOLD;
  }, []);

  const transcriptEntry = buildTaskCardTranscriptEntryFromStreamingTask(task);
  const hasTranscriptBody = transcriptEntry.blocks.length > 0;
  const hasChildCalls = transcriptEntry.blocks.some((block) => block.type === "tool_call");

  // Auto-scroll content when new tool calls arrive (only if user is near bottom)
  useEffect(() => {
    if (contentRef.current && isRunning && isNearBottomRef.current) {
      contentRef.current.scrollTop = contentRef.current.scrollHeight;
    }
  }, [task.childToolCalls.length, isRunning]);

  const delegatedConversationId = isDelegateCall ? task.delegatedConversationId ?? null : null;
  const hasBody = hasTranscriptBody || delegatedConversationId != null || isDelegateCall;
  const providerMetadata = {
    providerHarness: task.providerHarness,
    providerSessionId: task.providerSessionId,
    upstreamProvider: task.upstreamProvider,
    providerProfile: task.providerProfile,
    logicalModel: task.logicalModel,
    effectiveModelId: task.effectiveModelId,
    logicalEffort: task.logicalEffort,
    effectiveEffort: task.effectiveEffort,
    approvalPolicy: task.approvalPolicy,
    sandboxMode: task.sandboxMode,
    inputTokens: task.inputTokens,
    outputTokens: task.outputTokens,
    cacheCreationTokens: task.cacheCreationTokens,
    cacheReadTokens: task.cacheReadTokens,
    estimatedUsd: task.estimatedUsd,
  };

  return (
    <div
      data-testid="task-subagent-card"
      className="rounded-lg overflow-hidden mb-2"
      style={{
        backgroundColor: "var(--bg-elevated)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      {/* Header */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center gap-2 px-3 py-2 text-left hover:opacity-80 transition-opacity border-b"
        style={{ borderColor: "var(--border-subtle)" }}
        aria-expanded={isExpanded}
        aria-label={`${task.subagentType} subagent: ${task.description}. Click to ${isExpanded ? "collapse" : "expand"}.`}
      >
        {/* Expand/Collapse chevron */}
        {isExpanded ? (
          <ChevronDown size={14} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
        ) : (
          <ChevronRight size={14} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
        )}

        {/* Running indicator or bot icon */}
        {isRunning ? (
          <Loader2
            size={14}
            className="animate-spin flex-shrink-0"
            style={{ color: "var(--accent-primary)" }}
          />
        ) : (
          <Bot
            size={14}
            className="flex-shrink-0"
            style={{ color: "var(--text-muted)" }}
          />
        )}

        {/* Agent vs Task label */}
        <TaskCardKindBadge toolName={task.toolName} />

        {/* Subagent type badge */}
        {!isDelegateCall && <TaskCardSubagentTypeBadge subagentType={task.subagentType} />}

        {/* Description text */}
        <span
          className="text-xs truncate flex-1 min-w-0"
          style={{ color: "var(--text-secondary)" }}
        >
          {task.description}
        </span>

        <TaskCardProviderHarnessBadge
          providerHarness={task.providerHarness}
          providerMetadata={providerMetadata}
        />

        {/* Model badge */}
        {isDelegateCall ? (
          <TaskCardProviderModelBadge providerMetadata={providerMetadata} />
        ) : (
          <TaskCardModelBadge
            label={task.effectiveModelId ?? task.logicalModel ?? task.model}
            colorKey={task.effectiveModelId ?? task.logicalModel ?? task.model}
            providerMetadata={providerMetadata}
          />
        )}

        <TaskCardStatusBadge
          label={(isFailed || isCancelled) ? task.status : null}
          tone="error"
        />

        {/* Timer / Duration */}
        <span
          className="text-[0.625rem] tabular-nums flex-shrink-0"
          style={{ color: "var(--text-muted)" }}
        >
          {isRunning ? elapsed : (task.totalDurationMs != null ? formatDuration(task.totalDurationMs) : "")}
        </span>
      </button>

      {/* Body */}
      {isExpanded && (
        <div className="px-3 py-2">
          {hasBody && (
            <div
              ref={contentRef}
              onScroll={handleScroll}
              className="overflow-y-auto"
              style={{ maxHeight: `${MAX_CONTENT_HEIGHT}px`, overscrollBehavior: "contain" }}
            >
              {isDelegateCall && (
                <TaskCardRuntimeDetails providerMetadata={providerMetadata} />
              )}
              {delegatedConversationId ? (
                <TaskToolCallDelegatedTranscript
                  conversationId={delegatedConversationId}
                  delegatedAgentRunId={task.delegatedAgentRunId}
                  fallbackText={task.textOutput}
                />
              ) : hasTranscriptBody ? (
                <TaskCardTranscriptView entries={[transcriptEntry]} />
              ) : (
                <div
                  className="text-[0.6875rem] px-2 py-1.5 rounded"
                  style={{
                    backgroundColor: "var(--bg-surface)",
                    color: "var(--text-muted)",
                  }}
                >
                  {isRunning ? "Waiting for delegated output..." : "No delegated output available."}
                </div>
              )}
            </div>
          )}

          {/* Completed state: summary */}
          {isCompleted && !hasChildCalls && (
            <CompletedSummary task={task} />
          )}
        </div>
      )}

      {/* Collapsed completed summary (shown in header area when collapsed) */}
      {!isExpanded && isCompleted && (
        <div className="px-3 py-1.5 border-t" style={{ borderColor: "var(--border-subtle)" }}>
          <CompletedSummary task={task} />
        </div>
      )}
    </div>
  );
});
