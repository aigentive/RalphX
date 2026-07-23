/**
 * ActivityContext - Displays source/origin badge with contextual link
 *
 * Shows where an activity event originated (task or ideation session)
 * with a clickable link to navigate to the source, plus a role badge.
 */

import { useCallback } from "react";
import { CheckSquare, MessageSquare } from "lucide-react";
import { navigateToIdeationSession, openTaskInAgents } from "@/lib/navigation";

export interface ActivityContextProps {
  taskId?: string | undefined;
  sessionId?: string | undefined;
  role?: string | undefined;
}

/**
 * ActivityContext displays the source of an activity event and the role.
 * - Icon: Task icon (CheckSquare) or Session icon (MessageSquare)
 * - Label: Truncated task/session ID
 * - Link: Click to navigate to the source
 * - Role badge: Agent / System / User indicator
 */
export function ActivityContext({ taskId, sessionId, role }: ActivityContextProps) {
  const handleNavigate = useCallback(() => {
    if (taskId) {
      void openTaskInAgents(taskId, "graph");
    } else if (sessionId) {
      navigateToIdeationSession(sessionId);
    }
  }, [taskId, sessionId]);

  // Don't render if no context available
  if (!taskId && !sessionId) {
    return null;
  }

  const isTask = Boolean(taskId);
  const Icon = isTask ? CheckSquare : MessageSquare;
  const label = isTask
    ? `Task: ${taskId?.slice(0, 8)}...`
    : `Session: ${sessionId?.slice(0, 8)}...`;

  const roleLabel = role === "agent" ? "Agent" : role === "system" ? "System" : role === "user" ? "User" : null;

  return (
    <div className="flex items-center gap-2 text-xs text-[var(--text-muted)]">
      <button
        onClick={(e) => {
          e.stopPropagation();
          handleNavigate();
        }}
        className="flex items-center gap-1 hover:text-[var(--text-secondary)] transition-colors"
      >
        <Icon className="w-3 h-3" />
        <span>{label}</span>
      </button>
      {roleLabel && (
        <span className="px-1.5 py-0.5 rounded bg-[var(--bg-base)] text-[0.625rem]">
          {roleLabel}
        </span>
      )}
    </div>
  );
}
