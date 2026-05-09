/**
 * HookEventMessage — Renders hook events inline in the chat message stream.
 *
 * Three visual treatments:
 * 1. Started: centered muted label + gear icon + spinner
 * 2. Completed: centered pill + checkmark + collapsible output
 * 3. Block: amber warning card with left border + always-expanded reason
 */

import { useState } from "react";
import { Settings, Check, ChevronRight, AlertTriangle } from "lucide-react";
import type { HookEvent, HookStartedEvent, HookCompletedEvent, HookBlockEvent } from "@/types/hook-event";

// ============================================================================
// Started — muted centered label with spinner
// ============================================================================

function HookStarted({ event }: { event: HookStartedEvent }) {
  return (
    <div className="flex items-center justify-center gap-1.5 py-1">
      <div className="flex items-center gap-1.5" style={{ color: "var(--text-muted)" }}>
        <Settings className="w-[11px] h-[11px]" />
        <span
          className="text-[0.6875rem] leading-none"
          style={{ fontFamily: "var(--font-body)" }}
        >
          Running {event.hookEvent} hook…
        </span>
        <div
          className="w-[10px] h-[10px] rounded-full animate-spin"
          style={{
            border: "1.5px solid var(--text-muted)",
            borderTopColor: "transparent",
          }}
        />
      </div>
    </div>
  );
}

// ============================================================================
// Completed — centered pill with collapsible output
// ============================================================================

function HookCompleted({ event }: { event: HookCompletedEvent }) {
  const [expanded, setExpanded] = useState(false);
  const isSuccess = event.outcome !== "error" && (event.exitCode === null || event.exitCode === 0);

  return (
    <div className="flex flex-col items-center">
      <div className="flex items-center justify-center">
        <button
          type="button"
          onClick={() => setExpanded(!expanded)}
          className="flex items-center gap-1.5 px-2.5 py-[3px] rounded-xl transition-colors"
          style={{ background: expanded ? "var(--bg-elevated)" : "transparent" }}
          onMouseEnter={(e) => {
            if (!expanded) e.currentTarget.style.background = "var(--bg-elevated)";
          }}
          onMouseLeave={(e) => {
            if (!expanded) e.currentTarget.style.background = "transparent";
          }}
        >
          <Check
            className="w-[11px] h-[11px] shrink-0"
            style={{ color: "var(--status-success)" }}
          />
          <span
            className="text-[0.6875rem] leading-none"
            style={{
              color: "var(--text-muted)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {event.hookEvent}: {event.hookName}
          </span>
          <span
            className="text-[0.5938rem] font-medium px-[5px] py-[1px] rounded-md leading-none"
            style={
              isSuccess
                ? {
                    background: "var(--status-success-muted)",
                    color: "var(--status-success)",
                  }
                : {
                    background: "var(--status-error-muted)",
                    color: "var(--status-error)",
                  }
            }
          >
            {isSuccess ? "success" : "error"}
          </span>
          <ChevronRight
            className="w-[9px] h-[9px] shrink-0 transition-transform"
            style={{
              color: "var(--text-muted)",
              transform: expanded ? "rotate(90deg)" : undefined,
            }}
          />
        </button>
      </div>

      {/* Collapsible output block */}
      {expanded && event.output && (
        <div
          className="mt-0.5 mx-8 px-2.5 py-2 rounded-lg overflow-y-auto whitespace-pre-wrap break-words"
          style={{
            background: "var(--bg-surface)",
            fontFamily: "var(--font-mono)",
            fontSize: "0.6875rem",
            lineHeight: 1.5,
            color: "var(--text-secondary)",
            maxHeight: "120px",
          }}
        >
          {event.output}
        </div>
      )}
    </div>
  );
}

// ============================================================================
// Block — amber warning card
// ============================================================================

function HookBlock({ event }: { event: HookBlockEvent }) {
  return (
    <div
      className="my-1.5 rounded-r-lg px-3 py-2"
      style={{
        background: "var(--status-warning-muted)",
        borderLeft: "2px solid var(--status-warning-border)",
      }}
    >
      <div className="flex items-center gap-1.5 mb-1">
        <AlertTriangle
          className="w-[13px] h-[13px] shrink-0"
          style={{ color: "var(--status-warning)" }}
        />
        <span
          className="text-[0.7188rem] font-semibold"
          style={{ color: "var(--status-warning)" }}
        >
          Stop hook blocked{event.hookName ? `: ${event.hookName}` : ""}
        </span>
      </div>
      <div
        className="text-[0.75rem] leading-[1.45]"
        style={{ color: "var(--text-secondary)" }}
        dangerouslySetInnerHTML={{
          __html: escapeAndStyleCode(event.reason),
        }}
      />
    </div>
  );
}

/** Escape HTML and wrap backtick-delimited code in styled <code> elements */
function escapeAndStyleCode(text: string): string {
  const escaped = text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

  return escaped.replace(
    /`([^`]+)`/g,
    '<code style="font-family: var(--font-mono); font-size: 0.6875rem; background: var(--status-warning-muted); padding: 1px 4px; border-radius: 3px; color: var(--status-warning);">$1</code>'
  );
}

// ============================================================================
// Main component — dispatches by event type
// ============================================================================

export interface HookEventMessageProps {
  event: HookEvent;
}

export function HookEventMessage({ event }: HookEventMessageProps) {
  switch (event.type) {
    case "started":
      return <HookStarted event={event} />;
    case "completed":
      return <HookCompleted event={event} />;
    case "block":
      return <HookBlock event={event} />;
  }
}
