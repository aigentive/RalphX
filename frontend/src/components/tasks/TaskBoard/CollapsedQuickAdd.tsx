/**
 * CollapsedQuickAdd - compact add button for a collapsed column
 *
 * Renders a compact dashed-border add button in collapsed draft/backlog columns.
 * Click expands the column and opens the normal InlineTaskAdd control.
 * e.stopPropagation() prevents triggering column expand.
 */

import { useCallback } from "react";
import { Plus } from "lucide-react";

interface CollapsedQuickAddProps {
  onActivate: () => void;
}

export function CollapsedQuickAdd({ onActivate }: CollapsedQuickAddProps) {
  const handleTriggerClick = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    onActivate();
  }, [onActivate]);

  const handleTriggerKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      e.stopPropagation();
      onActivate();
    }
  }, [onActivate]);

  return (
    <>
      <button
        data-testid="collapsed-quick-add"
        aria-label="Add task"
        onClick={handleTriggerClick}
        onKeyDown={handleTriggerKeyDown}
        className="collapsed-quick-add-btn"
        style={{
          display: "flex",
          alignItems: "center",
          width: "100%",
          marginTop: "8px",
          padding: "9px 12px",
          borderRadius: "6px",
          border: "1px dashed var(--border-strong)",
          backgroundColor: "transparent",
          cursor: "pointer",
          transition: "all 180ms cubic-bezier(0.4, 0, 0.2, 1)",
        }}
      >
        <span
          className="collapsed-quick-add-icon"
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            width: "18px",
            height: "18px",
            borderRadius: "5px",
            color: "var(--text-muted)",
            transition: "background-color 180ms ease, color 180ms ease",
            flexShrink: 0,
          }}
        >
          <Plus
            style={{
              width: "13px",
              height: "13px",
              strokeWidth: 2.5,
            }}
          />
        </span>
        <span
          className="collapsed-quick-add-label"
          style={{
            marginLeft: "6px",
            fontSize: "0.75rem",
            fontWeight: 500,
            color: "var(--text-muted)",
            whiteSpace: "nowrap",
            letterSpacing: 0,
            transition: "color 180ms ease",
          }}
        >
          Add task
        </span>
      </button>

      {/* Hover styles for the trigger button */}
      <style>{`
        .collapsed-quick-add-btn:hover {
          border-color: var(--accent-primary) !important;
          background-color: var(--accent-muted) !important;
        }
        .collapsed-quick-add-btn:hover .collapsed-quick-add-icon {
          color: var(--accent-primary) !important;
          background-color: color-mix(in srgb, var(--accent-primary) 12%, transparent) !important;
        }
        .collapsed-quick-add-btn:hover .collapsed-quick-add-label {
          color: var(--accent-primary) !important;
        }
      `}</style>
    </>
  );
}
