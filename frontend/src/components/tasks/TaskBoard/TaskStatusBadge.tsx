/**
 * TaskStatusBadge - Icon-only badge for ALL task states
 *
 * Uses shared STATUS_ICON_CONFIG for consistency with Graph view.
 * Displays a compact status icon in the card header.
 */

import type { InternalStatus } from "@/types/status";
import { ARCHIVED_ICON_CONFIG, getStatusIconConfig } from "@/types/status-icons";

export interface TaskStatusBadgeProps {
  /** The internal status of the task */
  status: InternalStatus;
  /** Whether the task is archived */
  isArchived?: boolean;
  /** Number of revision attempts (for re_executing state tooltip) */
  revisionCount?: number;
}

export function TaskStatusBadge({ status, isArchived = false, revisionCount }: TaskStatusBadgeProps) {
  // Archived takes precedence
  const config = isArchived ? ARCHIVED_ICON_CONFIG : getStatusIconConfig(status);
  const IconComponent = config.icon;

  // Custom title for re_executing with revision count
  const title =
    status === "re_executing" && revisionCount !== undefined
      ? `Attempt #${revisionCount + 1}`
      : config.label;

  return (
    <div
      data-testid={`status-badge-${isArchived ? "archived" : status}`}
      className="flex h-4 w-4 items-center justify-center rounded-full"
      style={{
        backgroundColor: "transparent",
        color: config.color,
      }}
      title={title}
    >
      <IconComponent className={`w-3 h-3 ${config.animate ? "animate-spin" : ""}`} />
    </div>
  );
}
