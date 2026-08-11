/**
 * TaskWidgets — Widgets for TaskCreate, TaskUpdate, and TaskList tool calls.
 *
 * Uses WidgetCard + WidgetHeader + Badge from shared.tsx.
 * Registered in registry.ts under keys: taskcreate, taskupdate, and tasklist.
 */

import React, { useMemo } from "react";
import { CheckSquare, List } from "lucide-react";
import { WidgetCard, WidgetHeader, Badge } from "./shared";
import type { ToolCallWidgetProps, BadgeVariant } from "./shared.constants";
import { getString, parseToolResultAsLines, colors } from "./shared.constants";

const STATUS_VARIANT: Record<string, BadgeVariant> = {
  pending: "muted",
  in_progress: "blue",
  completed: "success",
  deleted: "error",
};

function statusLabel(s: string): string {
  return s.replace(/_/g, " ");
}

// ============================================================================
// TaskCreateWidget
// ============================================================================

export const TaskCreateWidget = React.memo(function TaskCreateWidget({
  toolCall,
  compact = false,
  className,
}: ToolCallWidgetProps) {
  const args = toolCall.arguments;
  const subject = getString(args, "subject") ?? "New Task";
  const description = getString(args, "description");
  const activeForm = getString(args, "activeForm");

  const badge = activeForm ? (
    <Badge variant="accent" compact>{activeForm}</Badge>
  ) : undefined;

  if (!description) {
    return (
      <div className={className} style={{ display: "flex", alignItems: "center", gap: 5, padding: "2px 0", margin: "2px 0" }}>
        <CheckSquare size={12} style={{ color: colors.accent }} />
        <span style={{ fontSize: 10.5, color: colors.textSecondary }}>{subject}</span>
        {badge}
      </div>
    );
  }

  return (
    <WidgetCard
      className={className ?? ""}
      compact={compact}
      header={
        <WidgetHeader
          icon={<CheckSquare size={14} style={{ color: colors.accent }} />}
          title={`Create Task — ${subject}`}
          badge={badge}
          compact={compact}
        />
      }
    >
      <div style={{
        fontSize: compact ? 10.5 : 11,
        color: colors.textSecondary,
        lineHeight: 1.45,
        WebkitLineClamp: 3,
        WebkitBoxOrient: "vertical" as const,
        display: "-webkit-box",
        overflow: "hidden",
      }}>
        {description}
      </div>
    </WidgetCard>
  );
});

// ============================================================================
// TaskUpdateWidget
// ============================================================================

export const TaskUpdateWidget = React.memo(function TaskUpdateWidget({
  toolCall,
  className,
}: ToolCallWidgetProps) {
  const args = toolCall.arguments;
  const taskId = getString(args, "taskId") ?? "?";
  const status = getString(args, "status");
  const owner = getString(args, "owner");
  const subject = getString(args, "subject");

  return (
    <div
      className={className}
      style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0", margin: "2px 0", flexWrap: "wrap" }}
    >
      <CheckSquare size={12} style={{ color: colors.accent }} />
      <span style={{ fontSize: 10.5, color: colors.textSecondary, fontWeight: 500 }}>
        Update Task #{taskId}
      </span>
      {status && (
        <Badge variant={STATUS_VARIANT[status] ?? "muted"} compact>
          {statusLabel(status)}
        </Badge>
      )}
      {owner && (
        <span style={{ fontSize: 10, color: colors.textMuted }}>
          → {owner}
        </span>
      )}
      {subject && (
        <span style={{ fontSize: 10, color: colors.textMuted, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 200 }}>
          {subject}
        </span>
      )}
    </div>
  );
});

// ============================================================================
// TaskListWidget
// ============================================================================

interface TaskEntry {
  id: string;
  subject: string;
  status: string;
}

function parseTaskEntries(result: unknown): TaskEntry[] {
  const lines = parseToolResultAsLines(result);
  const tasks: TaskEntry[] = [];
  for (const line of lines) {
    // Match patterns like "#1: Subject (status: pending)" or "Task #1: Subject [in_progress]"
    const m = line.match(/#(\d+)[:\s]+(.+?)\s*(?:\((?:status:\s*)?|\[)(\w+)[)\]]?/i);
    if (m?.[1] && m[2] && m[3]) {
      tasks.push({ id: m[1], subject: m[2].replace(/["\u2014—–-]\s*$/, "").trim(), status: m[3] });
    }
  }
  return tasks;
}

export const TaskListWidget = React.memo(function TaskListWidget({
  toolCall,
  compact = false,
  className,
}: ToolCallWidgetProps) {
  const tasks = useMemo(() => parseTaskEntries(toolCall.result), [toolCall.result]);

  if (tasks.length === 0) {
    return (
      <div className={className} style={{ display: "flex", alignItems: "center", gap: 5, padding: "2px 0", margin: "2px 0" }}>
        <List size={12} style={{ color: colors.textMuted }} />
        <span style={{ fontSize: 10.5, color: colors.textMuted }}>Task List</span>
      </div>
    );
  }

  return (
    <WidgetCard
      className={className ?? ""}
      compact={compact}
      alwaysExpanded={tasks.length <= 3}
      header={
        <WidgetHeader
          icon={<List size={14} style={{ color: colors.textMuted }} />}
          title="Task List"
          badge={<Badge variant="muted" compact>{tasks.length} tasks</Badge>}
          compact={compact}
        />
      }
    >
      {tasks.map((task) => (
        <div
          key={task.id}
          style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0", fontSize: compact ? 10.5 : 11 }}
        >
          <span style={{ color: colors.textMuted, fontWeight: 600, minWidth: 18, textAlign: "right", fontSize: 9 }}>
            #{task.id}
          </span>
          <span style={{ color: colors.textSecondary, flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {task.subject}
          </span>
          <Badge variant={STATUS_VARIANT[task.status] ?? "muted"} compact>
            {statusLabel(task.status)}
          </Badge>
        </div>
      ))}
    </WidgetCard>
  );
});
