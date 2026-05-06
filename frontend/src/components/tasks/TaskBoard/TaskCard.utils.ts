/**
 * TaskCard utility functions
 *
 * Contains styling helpers extracted from TaskCard.tsx to reduce component size.
 * Design: v29a Kanban — flat card surfaces with full-card status tints.
 */

import { type InternalStatus, NON_DRAGGABLE_STATUSES } from "@/types/status";
import { KANBAN_CARD_FALLBACKS } from "@/lib/kanban-card-token-fallbacks";

function cssVar(name: string, fallback?: string): string {
  return fallback ? `var(${name}, ${fallback})` : `var(${name})`;
}

function cardSurface(backgroundColor: string, borderColor: string): React.CSSProperties {
  return {
    backgroundColor,
    borderColor,
    borderStyle: "solid",
    borderWidth: "1px",
  };
}

function isWarningStatus(status: InternalStatus): boolean {
  return [
    "blocked",
    "re_executing",
    "escalated",
    "revision_needed",
    "pending_merge",
    "merging",
    "waiting_on_pr",
    "merge_incomplete",
    "paused",
  ].includes(status);
}

function isSuccessStatus(status: InternalStatus): boolean {
  return ["approved", "merged", "review_passed", "qa_passed"].includes(status);
}

function isErrorStatus(status: InternalStatus): boolean {
  return ["failed", "stopped", "merge_conflict", "qa_failed"].includes(status);
}

function getStatusSurface(status: InternalStatus, isArchived: boolean): React.CSSProperties {
  if (isArchived) {
    return cardSurface(
      cssVar("--kanban-card-bg", "#232329"),
      cssVar("--kanban-card-border", "#34343C")
    );
  }

  if (isSuccessStatus(status)) {
    return cardSurface(
      cssVar("--kanban-card-success-bg", KANBAN_CARD_FALLBACKS.successBg),
      cssVar("--kanban-card-success-border", KANBAN_CARD_FALLBACKS.successBorder)
    );
  }

  if (isWarningStatus(status)) {
    return cardSurface(
      cssVar("--kanban-card-warning-bg", KANBAN_CARD_FALLBACKS.warningBg),
      cssVar("--kanban-card-warning-border", KANBAN_CARD_FALLBACKS.warningBorder)
    );
  }

  if (isErrorStatus(status)) {
    return cardSurface(
      cssVar("--status-error-muted", KANBAN_CARD_FALLBACKS.errorBg),
      cssVar("--status-error-border", KANBAN_CARD_FALLBACKS.errorBorder)
    );
  }

  return cardSurface(
    cssVar("--kanban-card-bg", "#232329"),
    cssVar("--kanban-card-border", "#34343C")
  );
}

/**
 * Build base card styles.
 */
export function getBaseCardStyles(
  status: InternalStatus,
  isArchived: boolean,
  isDraggable: boolean
): React.CSSProperties {
  return {
    ...getStatusSurface(status, isArchived),
    cursor: isDraggable ? "grab" : "default",
    transition: "background 150ms ease, border-color 150ms ease, transform 150ms ease",
    borderRadius: "8px",
    boxShadow: "none",
  };
}

/**
 * Get card styles based on state (dragging, selected, default)
 */
export function getCardStyles(
  status: InternalStatus,
  isArchived: boolean,
  isDragging: boolean,
  isDraggable: boolean,
  isSelected: boolean
): React.CSSProperties {
  const baseStyles = getBaseCardStyles(status, isArchived, isDraggable);

  if (isDragging) {
    return {
      ...baseStyles,
      cursor: "grabbing",
      transform: "scale(1.015)",
      boxShadow: "none",
      zIndex: 50,
    };
  }

  if (isSelected) {
    return {
      ...baseStyles,
      boxShadow: `inset 0 0 0 1px ${cssVar("--kanban-card-selected-border", KANBAN_CARD_FALLBACKS.selectedBorder)}`,
    };
  }

  return baseStyles;
}

/**
 * Check if a task with given status is draggable
 * Uses centralized NON_DRAGGABLE_STATUSES from @/types/status
 */
export function isDraggableStatus(status: InternalStatus): boolean {
  return !(NON_DRAGGABLE_STATUSES as readonly string[]).includes(status);
}
