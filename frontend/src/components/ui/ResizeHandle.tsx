/**
 * ResizeHandle - Shared resize handle component for split layouts
 *
 * Features:
 * - 12px wide hit area for easy grabbing
 * - 2px visible line (subtle gray default)
 * - Orange highlight on hover
 * - Orange highlight while dragging
 */

// ============================================================================
// Shared Layout Constants
// ============================================================================

/** Default chat panel width in pixels */
export const CHAT_PANEL_DEFAULT_WIDTH = 600;

/** Minimum chat panel width in pixels */
export const CHAT_PANEL_MIN_WIDTH = 320;

import { memo } from "react";

interface ResizeHandleProps {
  /** Whether currently resizing */
  isResizing: boolean;
  /** Mouse down handler to start resizing */
  onMouseDown: (e: React.MouseEvent) => void;
  /** Optional double click handler, typically used to reset the split */
  onDoubleClick?: (e: React.MouseEvent) => void;
  /** Test ID for testing */
  testId?: string;
}

export const ResizeHandle = memo(function ResizeHandle({
  isResizing,
  onMouseDown,
  onDoubleClick,
  testId = "resize-handle",
}: ResizeHandleProps) {
  return (
    <div
      data-testid={testId}
      className="h-full shrink-0 cursor-ew-resize flex items-center justify-center group relative z-10"
      style={{ width: "12px", marginLeft: "-6px", marginRight: "-6px" }}
      onMouseDown={onMouseDown}
      onDoubleClick={onDoubleClick}
    >
      <div
        className={`h-full transition-all duration-150 ${
          isResizing
            ? "w-[3px] bg-[var(--accent-primary)]"
            : "w-[2px] bg-[var(--overlay-weak)] group-hover:w-[3px] group-hover:bg-[var(--accent-primary)]"
        }`}
      />
    </div>
  );
});

/**
 * Static separator line (non-interactive)
 * Used when resize is not available (e.g., timeline in graph view)
 */
export const SeparatorLine = memo(function SeparatorLine() {
  return (
    <div className="shrink-0 w-px bg-[var(--overlay-faint)]" />
  );
});
