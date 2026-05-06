/**
 * TaskToolCallCard - Static card for completed Task tool calls in final messages.
 *
 * Renders Task subagent calls as collapsible cards showing:
 * - Header: subagent type badge, description, model badge, stats
 * - Body (collapsed by default): the subagent's final text output
 *
 * Matches the TaskSubagentCard streaming design but for persisted messages.
 */

import React, { useState, useMemo } from "react";
import { ChevronDown, ChevronRight, Bot } from "lucide-react";
import { withAlpha } from "@/lib/theme-colors";
import type { ToolCall } from "./tool-widgets/shared.constants";
import {
  extractDelegationMetadata,
  isDelegationStartToolCall,
} from "./delegation-tool-calls";
import { TaskToolCallDelegatedTranscript } from "./TaskToolCallDelegatedTranscript";
import {
  EMPTY_STATS,
  extractChildToolCalls,
  extractTaskArgs,
  extractTaskStats,
} from "./TaskToolCallCard.utils";
import {
  TaskCardTranscriptView,
} from "./TaskCardTranscript";
import { buildTaskCardTranscriptEntryFromToolCall } from "./TaskCardTranscript.utils";
import {
  formatProviderModelEffortLabel,
} from "./provider-harness";
import {
  TaskCardKindBadge,
  TaskCardModelBadge,
  TaskCardProviderHarnessBadge,
  TaskCardStatusBadge,
  TaskCardSubagentTypeBadge,
  TaskCardSummary,
} from "./TaskCardShared";

// ============================================================================
// Types
// ============================================================================

interface TaskToolCallCardProps {
  toolCall: ToolCall;
  className?: string;
}

// ============================================================================
// Helpers
// ============================================================================


// ============================================================================
// Component
// ============================================================================

export const TaskToolCallCard = React.memo(function TaskToolCallCard({
  toolCall,
  className = "",
}: TaskToolCallCardProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const hasError = Boolean(toolCall.error);

  const taskArgs = useMemo(() => extractTaskArgs(toolCall.arguments), [toolCall.arguments]);
  const delegation = useMemo(
    () => extractDelegationMetadata(toolCall.arguments, toolCall.result),
    [toolCall.arguments, toolCall.result],
  );
  const isDelegateCall = isDelegationStartToolCall(toolCall.name);
  const taskStats = useMemo(
    () => isDelegateCall
      ? {
          ...EMPTY_STATS,
          statsAvailable: true,
          totalDurationMs: delegation.durationMs,
          totalTokens: delegation.totalTokens,
          model: delegation.effectiveModelId ?? delegation.logicalModel,
          textOutput: delegation.textOutput,
          estimatedUsd: delegation.estimatedUsd,
        }
      : extractTaskStats(toolCall),
    [delegation, isDelegateCall, toolCall],
  );
  const childToolCalls = useMemo(() => extractChildToolCalls(toolCall.result), [toolCall.result]);

  const isAgentCall = !isDelegateCall && toolCall.name.toLowerCase() === "agent";
  const subagentType = isDelegateCall ? "delegated" : taskArgs.subagent_type || "agent";
  const model = isDelegateCall
    ? delegation.effectiveModelId ?? delegation.logicalModel ?? ""
    : taskArgs.model || "";
  const providerModelEffortLabel = isDelegateCall
      ? formatProviderModelEffortLabel({
        providerHarness: delegation.providerHarness,
        providerSessionId: delegation.providerSessionId,
        upstreamProvider: delegation.upstreamProvider,
        providerProfile: delegation.providerProfile,
        logicalModel: delegation.logicalModel,
        effectiveModelId: delegation.effectiveModelId,
        logicalEffort: delegation.logicalEffort,
        effectiveEffort: delegation.effectiveEffort,
        inputTokens: delegation.inputTokens,
        outputTokens: delegation.outputTokens,
        cacheCreationTokens: delegation.cacheCreationTokens,
        cacheReadTokens: delegation.cacheReadTokens,
        estimatedUsd: delegation.estimatedUsd,
      })
    : null;

  // Card title: Agent with name → show name (team mode); otherwise description or fallback
  const cardTitle = isDelegateCall
    ? delegation.agentName || delegation.title || "Delegated specialist"
    : isAgentCall && taskArgs.name
      ? taskArgs.name
      : taskArgs.description || (isAgentCall ? "Agent task" : "Subagent task");

  // Hide subagent_type badge when it's the redundant default "agent" value
  const showSubagentTypeBadge = !isDelegateCall && Boolean(subagentType && subagentType !== "agent");

  // Subtitle: shown below title for named agents (description or prompt preview)
  const subtitle = isDelegateCall
    ? delegation.prompt
      ? `${delegation.prompt.slice(0, 100)}${delegation.prompt.length > 100 ? "..." : ""}`
      : null
    : isAgentCall && taskArgs.name && taskArgs.description
      ? taskArgs.description
      : isAgentCall && taskArgs.name && !taskArgs.description && taskArgs.prompt
        ? taskArgs.prompt.slice(0, 100) + "..."
        : null;

  const bodyText = delegation.textOutput ?? taskStats.textOutput;
  const delegatedConversationId = isDelegateCall ? delegation.delegatedConversationId ?? null : null;
  const statusBadge = isDelegateCall && delegation.status && delegation.status !== "completed"
    ? delegation.status
    : null;

  const transcriptEntry = useMemo(
    () => buildTaskCardTranscriptEntryFromToolCall({
      entryId: toolCall.id,
      bodyText,
      childToolCalls,
    }),
    [bodyText, childToolCalls, toolCall.id],
  );
  const hasTranscriptBody = transcriptEntry.blocks.length > 0;
  const hasBody = hasTranscriptBody || hasError || delegatedConversationId != null;
  const providerMetadata = {
    providerHarness: delegation.providerHarness,
    providerSessionId: delegation.providerSessionId,
    upstreamProvider: delegation.upstreamProvider,
    providerProfile: delegation.providerProfile,
    logicalModel: delegation.logicalModel,
    effectiveModelId: delegation.effectiveModelId,
    logicalEffort: delegation.logicalEffort,
    effectiveEffort: delegation.effectiveEffort,
    inputTokens: delegation.inputTokens,
    outputTokens: delegation.outputTokens,
    cacheCreationTokens: delegation.cacheCreationTokens,
    cacheReadTokens: delegation.cacheReadTokens,
    estimatedUsd: delegation.estimatedUsd,
  };

  return (
    <div
      data-testid="task-tool-call-card"
      className={`rounded-lg overflow-hidden ${className}`}
      style={{
        backgroundColor: hasError ? "var(--status-error-muted)" : "var(--bg-elevated)",
        border: `1px solid ${hasError ? "var(--status-error-border)" : "var(--border-subtle)"}`,
      }}
    >
      {/* Header */}
      <button
        onClick={() => hasBody && setIsExpanded(!isExpanded)}
        className={`w-full flex items-center gap-2 px-3 py-2 text-left transition-opacity ${hasBody ? "hover:opacity-80 cursor-pointer" : "cursor-default"}`}
        aria-expanded={hasBody ? isExpanded : undefined}
        aria-label={`${isDelegateCall ? "delegated" : subagentType} task: ${cardTitle}. ${hasBody ? `Click to ${isExpanded ? "collapse" : "expand"}.` : ""}`}
      >
        {/* Expand/Collapse chevron (only if has body) */}
        {hasBody ? (
          isExpanded ? (
            <ChevronDown size={14} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
          ) : (
            <ChevronRight size={14} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
          )
        ) : (
          <Bot size={14} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
        )}

        {/* Bot icon (when expandable, show alongside chevron) */}
        {hasBody && (
          <Bot size={14} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
        )}

        {/* Agent vs Task label */}
        <TaskCardKindBadge toolName={toolCall.name} />

        {/* Subagent type badge — hidden for redundant "agent" default */}
        {showSubagentTypeBadge && <TaskCardSubagentTypeBadge subagentType={subagentType} />}

        {/* Title text */}
        <span
          className="text-xs truncate flex-1 min-w-0"
          style={{ color: hasError ? "var(--status-error)" : "var(--text-secondary)" }}
        >
          {cardTitle}
        </span>

        {isDelegateCall && (
          <TaskCardProviderHarnessBadge
            providerHarness={delegation.providerHarness}
            providerMetadata={providerMetadata}
          />
        )}

        {/* Model badge */}
        {!providerModelEffortLabel && (
          <TaskCardModelBadge label={model || null} colorKey={model || null} />
        )}

        {providerModelEffortLabel && (
          <TaskCardModelBadge
            label={providerModelEffortLabel}
            colorKey={delegation.effectiveModelId ?? delegation.logicalModel ?? providerModelEffortLabel}
            providerMetadata={providerMetadata}
          />
        )}

        {/* Agent-specific: isolation badge */}
        {isAgentCall && taskArgs.isolation && (
          <span
            className="text-[0.625rem] px-1.5 py-0.5 rounded flex-shrink-0"
            style={{
              backgroundColor: "var(--status-info-muted)",
              color: "var(--status-info)",
            }}
          >
            {taskArgs.isolation}
          </span>
        )}

        {/* Agent-specific: background indicator */}
        {isAgentCall && taskArgs.run_in_background && (
          <span
            className="text-[0.625rem] px-1.5 py-0.5 rounded flex-shrink-0"
            style={{
              backgroundColor: withAlpha("var(--accent-primary)", 12),
              color: "var(--accent-primary)",
            }}
          >
            bg
          </span>
        )}

        {/* Error indicator */}
        <TaskCardStatusBadge label={statusBadge} />

        <TaskCardStatusBadge label={hasError ? "Failed" : null} tone="error" />
      </button>

      {/* Subtitle: description or prompt preview for named agents */}
      {subtitle && (
        <div
          className="px-3 pb-1.5"
          style={{ paddingLeft: "2.25rem" /* align under title, past chevron+bot icons */ }}
        >
          <span
            className="text-[0.6875rem] truncate block"
            style={{ color: "var(--text-muted)" }}
          >
            {subtitle}
          </span>
        </div>
      )}

      {/* Stats summary (shown below header when collapsed) */}
      {(
        taskStats.totalDurationMs != null ||
        taskStats.totalTokens != null ||
        taskStats.totalToolUseCount != null ||
        taskStats.estimatedUsd != null
      ) && (
        <div
          className="px-3 py-1.5"
          style={{
            borderTop: `1px solid ${hasError ? "var(--status-error-muted)" : "var(--border-subtle)"}`,
          }}
        >
          <TaskCardSummary
            metrics={{
              totalDurationMs: taskStats.totalDurationMs,
              totalTokens: taskStats.totalTokens,
              totalToolUseCount: taskStats.totalToolUseCount,
              estimatedUsd: taskStats.estimatedUsd,
            }}
          />
        </div>
      )}

      {/* Expanded body: child tool calls + subagent text output */}
      {isExpanded && hasBody && (
        <div
          className="px-3 pb-3 pt-2"
          style={{
            borderTop: (
              taskStats.totalDurationMs != null ||
              taskStats.totalTokens != null ||
              taskStats.totalToolUseCount != null ||
              taskStats.estimatedUsd != null
            )
              ? `1px solid ${hasError ? "var(--status-error-muted)" : "var(--border-subtle)"}`
              : undefined,
          }}
        >
          {/* Error message */}
          {hasError && toolCall.error && (
            <pre
              className="text-[0.6875rem] px-2 py-1.5 rounded overflow-x-auto max-h-48"
              style={{
                backgroundColor: "var(--status-error-muted)",
                color: "var(--status-error)",
                fontFamily: "var(--font-mono)",
                wordBreak: "break-word",
                whiteSpace: "pre-wrap",
              }}
            >
              {toolCall.error}
            </pre>
          )}

          {delegatedConversationId ? (
            <TaskToolCallDelegatedTranscript
              conversationId={delegatedConversationId}
              fallbackText={bodyText}
            />
          ) : hasTranscriptBody ? (
            <div className="max-h-64 overflow-y-auto">
              <TaskCardTranscriptView entries={[transcriptEntry]} />
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
});
