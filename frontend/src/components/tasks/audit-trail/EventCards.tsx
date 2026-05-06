/**
 * EventCards - Right-pane event card components for the audit trail dialog.
 * Renders different card styles per event source: transition, review, activity.
 * Shared utilities live in EventCardHelpers.tsx.
 */

import { ArrowRight, Bot, ExternalLink, MessageSquare, User } from "lucide-react";
import { navigateToIdeationSession } from "@/lib/navigation";
import {
  ACTIVITY_TYPE_CONFIG,
  CARD_STYLE,
  CONTENT_TRUNCATE_LENGTH,
  DEFAULT_ACTIVITY_CONFIG,
  ERROR_CARD_STYLE,
  ExpandableContent,
  formatTimestamp,
  extractToolName,
  REVIEW_OUTCOME_CONFIG,
  SOURCE_STYLES,
  StatusBadge,
  THINKING_TRUNCATE_LENGTH,
} from "./EventCardHelpers";

// ============================================================================
// Types — canonical source: @/hooks/useAuditTrail
// ============================================================================

import type { AuditEntry } from "@/hooks/useAuditTrail";
export type { AuditEntry };

// ============================================================================
// SourceBadge (exported for use by other files)
// ============================================================================

export function SourceBadge({ source }: { source: AuditEntry["source"] }) {
  const style = SOURCE_STYLES[source];
  return (
    <span
      data-testid="source-badge"
      className="px-1.5 py-0.5 rounded text-[0.625rem] font-medium"
      style={{
        backgroundColor: style.bg,
        border: `1px solid ${style.border}`,
        color: style.color,
      }}
    >
      {style.label}
    </span>
  );
}

// ============================================================================
// TransitionEventCard
// ============================================================================

function TransitionEventCard({ entry }: { entry: AuditEntry }) {
  return (
    <div
      data-testid="transition-card"
      className="flex items-start gap-2 py-2 px-3 rounded"
      style={CARD_STYLE}
    >
      <div
        className="flex items-center justify-center w-7 h-7 rounded-full shrink-0 mt-0.5"
        style={{ backgroundColor: "var(--overlay-faint)" }}
      >
        <ArrowRight className="w-3.5 h-3.5" style={{ color: "var(--status-info)" }} />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 flex-wrap">
          {entry.fromStatus && <StatusBadge status={entry.fromStatus} />}
          <ArrowRight className="w-3 h-3 text-text-primary/30" />
          {entry.toStatus && <StatusBadge status={entry.toStatus} />}
          <span className="text-[0.6875rem] text-text-primary/40 ml-auto shrink-0">
            {formatTimestamp(entry.timestamp)}
          </span>
        </div>
        <div className="text-[0.6875rem] text-text-primary/50 mt-0.5">by {entry.actor}</div>
        {entry.description && <ExpandableContent text={entry.description} />}
      </div>
    </div>
  );
}

// ============================================================================
// ActivityEventCard
// ============================================================================

function ActivityEventCard({ entry }: { entry: AuditEntry }) {
  const config = ACTIVITY_TYPE_CONFIG[entry.type] ?? DEFAULT_ACTIVITY_CONFIG;
  const Icon = config.icon;
  const isError = entry.type === "error";
  const isThinking = entry.type === "thinking";
  const isToolCall = entry.type === "tool_call";
  const cardStyle = isError ? ERROR_CARD_STYLE : CARD_STYLE;
  const truncateLength = isThinking ? THINKING_TRUNCATE_LENGTH : CONTENT_TRUNCATE_LENGTH;
  const toolName = isToolCall ? extractToolName(entry.description) : null;

  return (
    <div
      data-testid="activity-card"
      data-variant={isError ? "error" : undefined}
      className="flex items-start gap-2 py-2 px-3 rounded"
      style={cardStyle}
    >
      <div
        className="flex items-center justify-center w-7 h-7 rounded-full shrink-0 mt-0.5"
        style={{ backgroundColor: "var(--overlay-faint)" }}
      >
        <Icon className="w-3.5 h-3.5" style={{ color: config.color }} />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5 flex-wrap">
          <span className="text-[0.6875rem] font-medium" style={{ color: config.color }}>
            {entry.type}
          </span>
          {isToolCall && toolName && (
            <span
              className="font-mono text-[0.625rem] px-1.5 py-0.5 rounded text-text-primary/70"
              style={{
                backgroundColor: "var(--overlay-weak)",
              }}
            >
              {toolName}
            </span>
          )}
          <span className="text-[0.6875rem] text-text-primary/40 ml-auto shrink-0">
            {formatTimestamp(entry.timestamp)}
          </span>
        </div>
        <div className="text-[0.6875rem] text-text-primary/50 mt-0.5">by {entry.actor}</div>
        {entry.description && (
          <ExpandableContent
            text={entry.description}
            maxLength={truncateLength}
            italic={isThinking}
          />
        )}
        {entry.followupSessionId && (
          <div
            className="mt-2 flex items-center justify-between gap-2 rounded px-2 py-1.5"
            style={{
              backgroundColor: "var(--accent-muted)",
              border: "1px solid var(--overlay-weak)",
            }}
          >
            <span className="text-[0.625rem] text-text-primary/45 break-all min-w-0">
              Follow-up: {entry.followupSessionId}
            </span>
            <button
              type="button"
              onClick={() => navigateToIdeationSession(entry.followupSessionId!)}
              className="shrink-0 inline-flex items-center gap-1 text-[0.625rem] font-medium transition-opacity hover:opacity-80"
              style={{ color: "#ff8a5b" }}
            >
              <ExternalLink className="w-3 h-3" />
              Open
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

// ============================================================================
// ReviewEventCard
// ============================================================================

function ReviewEventCard({ entry }: { entry: AuditEntry }) {
  const outcomeConfig = REVIEW_OUTCOME_CONFIG[entry.type];
  const isAI = entry.actor.toLowerCase().includes("ai");
  const ReviewerIcon = isAI ? Bot : User;

  return (
    <div
      data-testid="review-card"
      className="flex items-start gap-2 py-2 px-3 rounded"
      style={CARD_STYLE}
    >
      <div
        className="flex items-center justify-center w-7 h-7 rounded-full shrink-0 mt-0.5"
        style={{ backgroundColor: "var(--overlay-faint)" }}
      >
        <ReviewerIcon
          className="w-3.5 h-3.5"
          style={{ color: outcomeConfig?.color ?? "var(--text-muted)" }}
        />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5 flex-wrap">
          {outcomeConfig ? (
            <span
              className="rounded-full px-2 py-0.5 text-[0.625rem] font-semibold"
              style={{
                backgroundColor: outcomeConfig.bgColor,
                color: outcomeConfig.color,
                border: `1px solid ${outcomeConfig.color}40`,
              }}
            >
              {entry.type}
            </span>
          ) : (
            <span className="text-[0.6875rem] font-medium text-text-primary/70">{entry.type}</span>
          )}
          <span className="text-[0.6875rem] text-text-primary/40 ml-auto shrink-0">
            {formatTimestamp(entry.timestamp)}
          </span>
        </div>
        <div className="text-[0.6875rem] text-text-primary/50 mt-0.5">by {entry.actor}</div>
        {entry.description && <ExpandableContent text={entry.description} />}
        {entry.metadata && (
          <p
            className="text-[0.625rem] mt-1 italic"
            style={{ color: "var(--text-muted)" }}
          >
            {entry.metadata}
          </p>
        )}
        {entry.followupSessionId && (
          <div
            className="mt-2 flex items-center justify-between gap-2 rounded px-2 py-1.5"
            style={{
              backgroundColor: "var(--accent-muted)",
              border: "1px solid var(--overlay-weak)",
            }}
          >
            <span className="text-[0.625rem] text-text-primary/45 break-all min-w-0">
              Follow-up: {entry.followupSessionId}
            </span>
            <button
              type="button"
              onClick={() => navigateToIdeationSession(entry.followupSessionId!)}
              className="shrink-0 inline-flex items-center gap-1 text-[0.625rem] font-medium transition-opacity hover:opacity-80"
              style={{ color: "#ff8a5b" }}
            >
              <ExternalLink className="w-3 h-3" />
              Open
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

// ============================================================================
// DefaultEventCard
// ============================================================================

function DefaultEventCard({ entry }: { entry: AuditEntry }) {
  return (
    <div
      data-testid="default-card"
      className="flex items-start gap-2 py-2 px-3 rounded"
      style={CARD_STYLE}
    >
      <div
        className="flex items-center justify-center w-7 h-7 rounded-full shrink-0 mt-0.5"
        style={{ backgroundColor: "var(--overlay-faint)" }}
      >
        <MessageSquare className="w-3.5 h-3.5" style={{ color: "var(--text-muted)" }} />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="text-[0.6875rem] font-medium text-text-primary/70">{entry.type}</span>
          <span className="text-[0.6875rem] text-text-primary/40 ml-auto shrink-0">
            {formatTimestamp(entry.timestamp)}
          </span>
        </div>
        <div className="text-[0.6875rem] text-text-primary/50 mt-0.5">by {entry.actor}</div>
        {entry.description && <ExpandableContent text={entry.description} />}
      </div>
    </div>
  );
}

// ============================================================================
// EventCard dispatcher
// ============================================================================

export function EventCard({ entry }: { entry: AuditEntry }) {
  switch (entry.source) {
    case "transition":
      return <TransitionEventCard entry={entry} />;
    case "review":
      return <ReviewEventCard entry={entry} />;
    case "activity":
      return <ActivityEventCard entry={entry} />;
    default:
      return <DefaultEventCard entry={entry} />;
  }
}
