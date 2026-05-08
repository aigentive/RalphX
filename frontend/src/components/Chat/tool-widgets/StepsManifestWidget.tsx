/**
 * StepsManifestWidget - Renders get_task_steps results as a collapsible numbered checklist
 *
 * Follows the Widget 2 mockup from mockups/tool-call-widgets.html:
 * - Collapsed: header with "Implementation Steps" title + "N/M completed" badge
 * - Expanded: numbered list with status icons (green check, orange dot, gray circle)
 * - Gradient fade overlay when collapsed
 */

import React, { useState, useMemo } from "react";
import type { ToolCallWidgetProps } from "./shared.constants";

// ============================================================================
// Constants
// ============================================================================

/** Height for ~3.65 lines at 20px line-height */
const COLLAPSED_HEIGHT = 73;
const COLLAPSED_HEIGHT_COMPACT = 52;
const GRADIENT_HEIGHT = 36;

// ============================================================================
// Types
// ============================================================================

interface StepData {
  title: string;
  status: string;
  sort_order: number;
}

interface StepCounts {
  completed: number;
  inProgress: number;
  pending: number;
  total: number;
}

type StepsManifestWidgetProps = ToolCallWidgetProps;

// ============================================================================
// Helpers
// ============================================================================

function parseSteps(result: unknown): StepData[] {
  if (!result) return [];

  // Handle MCP result wrapper: [{text: "..."}] or direct array
  let data = result;
  if (Array.isArray(result) && result.length === 1 && typeof result[0] === "object" && result[0] !== null && "text" in result[0]) {
    try {
      data = JSON.parse((result[0] as { text: string }).text);
    } catch {
      return [];
    }
  }

  if (!Array.isArray(data)) return [];

  return data
    .filter((item): item is StepData =>
      item != null &&
      typeof item === "object" &&
      typeof (item as Record<string, unknown>).title === "string" &&
      typeof (item as Record<string, unknown>).status === "string"
    )
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
}

function countSteps(steps: StepData[]): StepCounts {
  let completed = 0;
  let inProgress = 0;
  let pending = 0;

  for (const step of steps) {
    switch (step.status) {
      case "completed":
      case "skipped":
        completed++;
        break;
      case "in_progress":
        inProgress++;
        break;
      default:
        pending++;
        break;
    }
  }

  return { completed, inProgress, pending, total: steps.length };
}

// ============================================================================
// Sub-components
// ============================================================================

/** SVG icons matching the mockup exactly */
function CheckIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

function InProgressIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ animation: "pulse-dot 1.5s ease-in-out infinite" }}>
      <circle cx="12" cy="12" r="8" />
      <circle cx="12" cy="12" r="3" fill="currentColor" />
    </svg>
  );
}

function PendingIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
      <circle cx="12" cy="12" r="8" />
    </svg>
  );
}

function FailedIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="8" />
      <line x1="15" y1="9" x2="9" y2="15" />
      <line x1="9" y1="9" x2="15" y2="15" />
    </svg>
  );
}

function StepStatusIcon({ status }: { status: string }) {
  switch (status) {
    case "completed":
    case "skipped":
      return <span style={{ color: "var(--status-success)" }}><CheckIcon /></span>;
    case "in_progress":
      return <span style={{ color: "var(--accent-primary)" }}><InProgressIcon /></span>;
    case "failed":
    case "cancelled":
      return <span style={{ color: "var(--status-error)" }}><FailedIcon /></span>;
    default:
      return <span style={{ color: "var(--text-muted)" }}><PendingIcon /></span>;
  }
}

function getStepTextClass(status: string): string {
  switch (status) {
    case "completed":
    case "skipped":
      return "done-text";
    case "in_progress":
      return "active-text";
    default:
      return "";
  }
}

function getStepTextStyle(status: string): React.CSSProperties {
  switch (status) {
    case "completed":
    case "skipped":
      return { color: "var(--text-muted)" };
    case "in_progress":
      return { color: "var(--text-primary)", fontWeight: 500 };
    default:
      return { color: "var(--text-secondary)" };
  }
}

/** Chevron SVG (right-pointing, rotates 90deg when open) */
function ChevronIcon({ isOpen, compact }: { isOpen: boolean; compact?: boolean }) {
  const size = compact ? 8 : 10;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      style={{
        color: "var(--text-muted)",
        flexShrink: 0,
        transition: "transform 200ms",
        transform: isOpen ? "rotate(90deg)" : "rotate(0deg)",
      }}
    >
      <polyline points="9 18 15 12 9 6" />
    </svg>
  );
}

/** List icon matching mockup */
function ListIcon({ compact }: { compact?: boolean }) {
  const size = compact ? 12 : 14;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      style={{ color: "var(--text-muted)", flexShrink: 0 }}
    >
      <line x1="8" y1="6" x2="21" y2="6" />
      <line x1="8" y1="12" x2="21" y2="12" />
      <line x1="8" y1="18" x2="21" y2="18" />
      <line x1="3" y1="6" x2="3.01" y2="6" />
      <line x1="3" y1="12" x2="3.01" y2="12" />
      <line x1="3" y1="18" x2="3.01" y2="18" />
    </svg>
  );
}

// ============================================================================
// Component
// ============================================================================

export const StepsManifestWidget = React.memo(function StepsManifestWidget({
  toolCall,
  compact = false,
}: StepsManifestWidgetProps) {
  const [isOpen, setIsOpen] = useState(false);

  const steps = useMemo(() => parseSteps(toolCall.result), [toolCall.result]);
  const counts = useMemo(() => countSteps(steps), [steps]);

  // Edge case: no steps at all
  if (steps.length === 0) {
    return (
      <div
        data-testid="steps-manifest-empty"
        style={{
          display: "flex",
          alignItems: "center",
          gap: "5px",
          padding: "2px 0",
          margin: "2px 0",
        }}
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--text-muted)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <line x1="8" y1="6" x2="21" y2="6" />
          <line x1="8" y1="12" x2="21" y2="12" />
          <line x1="8" y1="18" x2="21" y2="18" />
          <line x1="3" y1="6" x2="3.01" y2="6" />
          <line x1="3" y1="12" x2="3.01" y2="12" />
          <line x1="3" y1="18" x2="3.01" y2="18" />
        </svg>
        <span style={{ fontSize: "0.6562rem", color: "var(--text-muted)" }}>
          No steps defined
        </span>
      </div>
    );
  }

  const collapseHeight = compact ? COLLAPSED_HEIGHT_COMPACT : COLLAPSED_HEIGHT;
  const needsCollapse = steps.length > 3;
  const showFull = isOpen || !needsCollapse;

  // Badge color: all completed = green, otherwise accent orange
  const allCompleted = counts.completed === counts.total;
  const badgeStyle: React.CSSProperties = allCompleted
    ? { background: "var(--status-success-muted)", color: "var(--status-success)" }
    : { background: "var(--accent-muted)", color: "var(--accent-primary)" };

  return (
    <div
      data-testid="steps-manifest-widget"
      style={{
        background: "var(--bg-surface)",
        borderRadius: "10px",
        overflow: "hidden",
        border: "1px solid var(--border-subtle)",
      }}
    >
      {/* Header */}
      <button
        data-testid="steps-manifest-toggle"
        onClick={() => setIsOpen(!isOpen)}
        style={{
          display: "flex",
          alignItems: "center",
          gap: "7px",
          padding: compact ? "5px 8px" : "7px 10px",
          cursor: "pointer",
          userSelect: "none",
          transition: "background 200ms",
          minHeight: compact ? "28px" : "32px",
          width: "100%",
          border: "none",
          background: "transparent",
          textAlign: "left",
        }}
        className="hover:opacity-80"
        aria-expanded={isOpen}
        aria-label={`Implementation Steps. ${counts.completed} of ${counts.total} completed. Click to ${isOpen ? "collapse" : "expand"}.`}
      >
        <ChevronIcon isOpen={isOpen} compact={compact} />
        <ListIcon compact={compact} />
        <span
          style={{
            fontSize: compact ? "11px" : "11.5px",
            fontWeight: 500,
            color: "var(--text-secondary)",
            flex: 1,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          Implementation Steps
        </span>
        <span
          style={{
            fontSize: "0.5938rem",
            padding: "1px 6px",
            borderRadius: "6px",
            fontWeight: 500,
            flexShrink: 0,
            whiteSpace: "nowrap",
            ...badgeStyle,
          }}
        >
          {counts.completed}/{counts.total}
        </span>
      </button>

      {/* Body with gradient fade */}
      <div
        style={{
          maxHeight: showFull ? "2000px" : `${collapseHeight}px`,
          overflow: "hidden",
          position: "relative",
          transition: "max-height 200ms ease",
        }}
      >
        <div
          style={{
            padding: "0 10px 8px",
            borderTop: "1px solid var(--border-subtle)",
            paddingTop: "8px",
          }}
        >
          <ol
            style={{ listStyle: "none", padding: 0, margin: 0 }}
            data-testid="steps-list"
          >
            {steps.map((step, index) => (
              <li
                key={index}
                data-testid={`step-item-${index}`}
                style={{
                  display: "flex",
                  alignItems: "flex-start",
                  gap: "7px",
                  padding: compact ? "2px 0" : "3px 0",
                  fontSize: compact ? "11px" : "11.5px",
                  lineHeight: "1.4",
                }}
              >
                <span
                  style={{
                    fontSize: "0.5625rem",
                    color: "var(--text-muted)",
                    fontWeight: 600,
                    minWidth: "14px",
                    textAlign: "right",
                    marginTop: "2px",
                    flexShrink: 0,
                  }}
                >
                  {index + 1}
                </span>
                <span
                  style={{
                    width: "16px",
                    height: "16px",
                    flexShrink: 0,
                    marginTop: "1px",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                  }}
                >
                  <StepStatusIcon status={step.status} />
                </span>
                <span
                  className={getStepTextClass(step.status)}
                  style={getStepTextStyle(step.status)}
                >
                  {step.title}
                </span>
              </li>
            ))}
          </ol>
        </div>

        {/* Gradient fade overlay (collapsed only) */}
        {!showFull && (
          <div
            style={{
              position: "absolute",
              bottom: 0,
              left: 0,
              right: 0,
              height: `${GRADIENT_HEIGHT}px`,
              background: "linear-gradient(to bottom, transparent, var(--bg-surface))",
              pointerEvents: "none",
              transition: "opacity 200ms",
            }}
          />
        )}
      </div>

      {/* Pulse animation keyframes */}
      <style>{`
        @keyframes pulse-dot {
          0%, 100% { opacity: 1; }
          50% { opacity: 0.4; }
        }
      `}</style>
    </div>
  );
});
