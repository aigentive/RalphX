import React, { useMemo } from "react";
import {
  CheckCircle2,
  ClipboardCheck,
  ClipboardList,
  ListChecks,
  PencilLine,
  Plus,
  TriangleAlert,
  UserCheck,
} from "lucide-react";

import { Badge, InlineIndicator, WidgetCard, WidgetHeader, WidgetRow } from "./shared";
import {
  colors,
  getArray,
  getBool,
  getString,
  parseMcpToolResult,
  type BadgeVariant,
  type ToolCallWidgetProps,
} from "./shared.constants";
import { canonicalizeToolName } from "./tool-name";

type AgentTaskToolName =
  | "create_agent_task"
  | "get_agent_task"
  | "list_agent_tasks"
  | "update_agent_task"
  | "claim_agent_task"
  | "complete_agent_task"
  | "get_delegate_assignment"
  | "complete_delegate_assignment"
  | "release_delegate_assignment";

interface AgentTaskView {
  taskId: string | undefined;
  taskNumber: number | undefined;
  title: string;
  details: string | undefined;
  state: string | undefined;
  ownerAgent: string | undefined;
  blockedBy: string[];
  blocks: string[];
  availability: string | undefined;
}

interface AgentTaskResult {
  success: boolean | undefined;
  error: string | undefined;
  task: AgentTaskView | null;
  tasks: AgentTaskView[];
  changedFields: string[];
  stateChange:
    | {
        from: string | undefined;
        to: string | undefined;
      }
    | undefined;
}

const TOOL_LABELS: Record<AgentTaskToolName, string> = {
  create_agent_task: "Create agent task",
  get_agent_task: "Read agent task",
  list_agent_tasks: "List agent tasks",
  update_agent_task: "Update agent task",
  claim_agent_task: "Claim agent task",
  complete_agent_task: "Complete agent task",
  get_delegate_assignment: "Assigned work",
  complete_delegate_assignment: "Request assignment completion",
  release_delegate_assignment: "Request assignment release",
};

const TOOL_DONE_LABELS: Record<AgentTaskToolName, string> = {
  create_agent_task: "Agent task created",
  get_agent_task: "Agent task loaded",
  list_agent_tasks: "Agent tasks listed",
  update_agent_task: "Agent task updated",
  claim_agent_task: "Agent task claimed",
  complete_agent_task: "Agent task completed",
  get_delegate_assignment: "Assigned work",
  complete_delegate_assignment: "Completion requested",
  release_delegate_assignment: "Release requested",
};

const STATE_VARIANT: Record<string, BadgeVariant> = {
  open: "muted",
  pending: "muted",
  active: "blue",
  in_progress: "blue",
  done: "success",
  completed: "success",
  dropped: "error",
  completion_requested: "blue",
  release_requested: "muted",
  released: "muted",
  failed: "error",
  cancelled: "error",
};

function isAgentTaskToolName(toolName: string): toolName is AgentTaskToolName {
  return toolName in TOOL_LABELS;
}

function iconForTool(toolName: AgentTaskToolName) {
  switch (toolName) {
    case "create_agent_task":
      return <Plus size={14} style={{ color: colors.accent }} />;
    case "get_agent_task":
      return <ClipboardCheck size={14} style={{ color: colors.textMuted }} />;
    case "list_agent_tasks":
      return <ListChecks size={14} style={{ color: colors.textMuted }} />;
    case "update_agent_task":
      return <PencilLine size={14} style={{ color: colors.accent }} />;
    case "claim_agent_task":
      return <UserCheck size={14} style={{ color: colors.accent }} />;
    case "complete_agent_task":
      return <CheckCircle2 size={14} style={{ color: colors.success }} />;
    case "get_delegate_assignment":
      return <ClipboardCheck size={14} style={{ color: colors.accent }} />;
    case "complete_delegate_assignment":
      return <CheckCircle2 size={14} style={{ color: colors.accent }} />;
    case "release_delegate_assignment":
      return <TriangleAlert size={14} style={{ color: colors.textMuted }} />;
  }
}

function stateLabel(state: string): string {
  switch (state) {
    case "active":
    case "in_progress":
      return "in progress";
    case "done":
    case "completed":
      return "done";
    case "dropped":
      return "dropped";
    case "completion_requested":
      return "completion requested";
    case "release_requested":
      return "release requested";
    case "released":
      return "released";
    case "failed":
      return "failed";
    case "cancelled":
      return "cancelled";
    default:
      return "open";
  }
}

function variantForState(state: string | undefined): BadgeVariant {
  return state ? STATE_VARIANT[state] ?? "muted" : "muted";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === "object" && !Array.isArray(value);
}

function pickString(record: Record<string, unknown>, keys: string[]): string | undefined {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim().length > 0) {
      return value;
    }
  }
  return undefined;
}

function pickNumber(record: Record<string, unknown>, keys: string[]): number | undefined {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "number" && Number.isFinite(value)) {
      return value;
    }
  }
  return undefined;
}

function pickStringArray(record: Record<string, unknown>, keys: string[]): string[] {
  for (const key of keys) {
    const value = record[key];
    if (Array.isArray(value)) {
      return value.filter((item): item is string => typeof item === "string");
    }
  }
  return [];
}

function parseTask(value: unknown): AgentTaskView | null {
  if (!isRecord(value)) {
    return null;
  }

  const title = pickString(value, ["title", "subject"]);
  if (!title) {
    return null;
  }

  return {
    taskId: pickString(value, ["task_id", "taskId", "id"]),
    taskNumber: pickNumber(value, ["task_number", "taskNumber"]),
    title,
    details: pickString(value, ["details", "description"]),
    state: pickString(value, ["assignment_state", "assignmentState", "state", "status", "task_state"]),
    ownerAgent: pickString(value, [
      "delegate_agent_name",
      "delegateAgentName",
      "owner_agent",
      "ownerAgent",
      "owner",
    ]),
    blockedBy: pickStringArray(value, ["unresolved_blocked_by", "blocked_by", "blockedBy"]),
    blocks: pickStringArray(value, ["blocks"]),
    availability: pickString(value, ["availability"]),
  };
}

function parseTasks(result: Record<string, unknown>): AgentTaskView[] {
  const tasks = getArray(result, "tasks") ?? [];
  return tasks.map(parseTask).filter((task): task is AgentTaskView => task != null);
}

function parsePreviewJsonString(value: string): string {
  try {
    return JSON.parse(`"${value}"`) as string;
  } catch {
    return value.replace(/\\"/g, "\"");
  }
}

function previewTextFromResult(result: unknown): string | null {
  if (typeof result === "string") {
    return result;
  }
  if (!isRecord(result)) {
    return null;
  }
  const text = result.text;
  if (typeof text === "string") {
    return text;
  }
  const content = result.content;
  if (Array.isArray(content)) {
    const firstText = content
      .map((item) => (isRecord(item) && typeof item.text === "string" ? item.text : null))
      .find((item): item is string => item != null);
    if (firstText) {
      return firstText;
    }
  }
  return null;
}

function parsePreviewTask(result: unknown): AgentTaskView | null {
  const previewText = previewTextFromResult(result);
  if (!previewText) {
    return null;
  }
  const taskNumberMatch = previewText.match(/"task_number"\s*:\s*(\d+)/);
  const titleMatch = previewText.match(/"title"\s*:\s*"((?:\\.|[^"\\])*)"/);
  if (!taskNumberMatch && !titleMatch) {
    return null;
  }
  const stateMatch = previewText.match(/"state"\s*:\s*"((?:\\.|[^"\\])*)"/);
  const taskIdMatch = previewText.match(/"task_id"\s*:\s*"((?:\\.|[^"\\])*)"/);
  const ownerMatch = previewText.match(/"owner_agent"\s*:\s*"((?:\\.|[^"\\])*)"/);
  return {
    taskId: taskIdMatch ? parsePreviewJsonString(taskIdMatch[1] ?? "") : undefined,
    taskNumber: taskNumberMatch ? Number(taskNumberMatch[1]) : undefined,
    title: titleMatch ? parsePreviewJsonString(titleMatch[1] ?? "") : "Agent task",
    details: undefined,
    state: stateMatch ? parsePreviewJsonString(stateMatch[1] ?? "") : undefined,
    ownerAgent: ownerMatch ? parsePreviewJsonString(ownerMatch[1] ?? "") : undefined,
    blockedBy: [],
    blocks: [],
    availability: undefined,
  };
}

function parseStateChange(value: unknown): AgentTaskResult["stateChange"] {
  if (!isRecord(value)) {
    return undefined;
  }
  return {
    from: pickString(value, ["from"]),
    to: pickString(value, ["to"]),
  };
}

function parseAgentTaskResult(result: unknown): AgentTaskResult {
  const parsed = parseMcpToolResult(result);
  return {
    success: typeof parsed.success === "boolean" ? parsed.success : undefined,
    error: typeof parsed.error === "string" ? parsed.error : undefined,
    task: parseTask(parsed.task) ?? parseTask(parsed.assignment) ?? parsePreviewTask(result),
    tasks: parseTasks(parsed),
    changedFields: getArray(parsed, "changed_fields")?.filter(
      (field): field is string => typeof field === "string"
    ) ?? [],
    stateChange: parseStateChange(parsed.state_change),
  };
}

function includeResolvedTasks(args: unknown): boolean {
  return getBool(args, "include_done") ?? getBool(args, "includeDone") ?? false;
}

function formatTaskRef(task: AgentTaskView | null | undefined, fallback?: string): string {
  if (task?.taskNumber != null) {
    return `#${task.taskNumber}`;
  }
  if (fallback) {
    return `#${fallback}`;
  }
  if (task?.taskId) {
    return `#${task.taskId.slice(0, 8)}`;
  }
  return "";
}

function taskSummaryTitle(
  toolName: AgentTaskToolName,
  task: AgentTaskView | null | undefined,
  args: unknown
): string {
  const title = task?.title ?? getString(args, "title");
  const taskRef = formatTaskRef(task, getString(args, "task_ref") ?? getString(args, "task_id"));
  if (taskRef && title) {
    return `${TOOL_DONE_LABELS[toolName]} ${taskRef} ${title}`;
  }
  if (title) {
    return `${TOOL_LABELS[toolName]} ${title}`;
  }
  if (taskRef) {
    return `${TOOL_LABELS[toolName]} ${taskRef}`;
  }
  return TOOL_LABELS[toolName];
}

function dependencySummary(task: AgentTaskView): string | null {
  const parts: string[] = [];
  if (task.blockedBy.length > 0) {
    parts.push(`blocked by ${task.blockedBy.length}`);
  }
  if (task.blocks.length > 0) {
    parts.push(`blocks ${task.blocks.length}`);
  }
  return parts.length > 0 ? parts.join(" / ") : null;
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div style={{ display: "flex", gap: 8, minWidth: 0 }}>
      <span
        style={{
          width: 76,
          flexShrink: 0,
          color: colors.textMuted,
          fontSize: 10,
        }}
      >
        {label}
      </span>
      <span
        style={{
          minWidth: 0,
          color: colors.textSecondary,
          fontSize: 10.5,
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {value}
      </span>
    </div>
  );
}

function AgentTaskRows({
  tasks,
  compact,
}: {
  tasks: AgentTaskView[];
  compact: boolean;
}) {
  const visibleTasks = tasks.slice(0, 6);
  const omittedCount = tasks.length - visibleTasks.length;

  return (
    <div style={{ display: "grid", gap: 5 }}>
      {visibleTasks.map((task, index) => {
        const dependencyText = dependencySummary(task);
        return (
          <div
            key={task.taskId ?? task.taskNumber ?? index}
            data-testid="agent-task-widget-row"
            style={{
              display: "flex",
              alignItems: "center",
              gap: 7,
              minWidth: 0,
              fontSize: compact ? 10.5 : 11,
            }}
          >
            <span
              style={{
                width: 30,
                flexShrink: 0,
                textAlign: "right",
                color: colors.textMuted,
                fontFamily: "var(--font-mono)",
                fontSize: 10,
                fontWeight: 600,
              }}
            >
              {formatTaskRef(task)}
            </span>
            <Badge variant={variantForState(task.state)} compact>
              {stateLabel(task.state ?? "open")}
            </Badge>
            <span
              style={{
                flex: 1,
                minWidth: 0,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
                color: colors.textSecondary,
              }}
            >
              {task.title}
            </span>
            {task.ownerAgent && (
              <span
                style={{
                  maxWidth: 110,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  color: colors.textMuted,
                  fontSize: 10,
                }}
              >
                {task.ownerAgent}
              </span>
            )}
            {dependencyText && (
              <span
                style={{
                  color: colors.textMuted,
                  fontSize: 10,
                  whiteSpace: "nowrap",
                }}
              >
                {dependencyText}
              </span>
            )}
          </div>
        );
      })}
      {omittedCount > 0 && (
        <div style={{ color: colors.textMuted, fontSize: 10.5 }}>
          {omittedCount} more {omittedCount === 1 ? "task" : "tasks"}
        </div>
      )}
    </div>
  );
}

function ListAgentTasksWidget({
  result,
  includeDone,
  compact,
  className,
}: {
  result: AgentTaskResult;
  includeDone: boolean;
  compact: boolean;
  className: string | undefined;
}) {
  const taskCount = result.tasks.length;
  const badgeLabel =
    taskCount === 0 && !includeDone
      ? "0 unresolved"
      : `${taskCount} ${taskCount === 1 ? "task" : "tasks"}`;
  const emptyLabel = includeDone
    ? "No agent tasks found in this snapshot"
    : "No unresolved agent tasks found in this snapshot";
  const header = (
    <WidgetHeader
      icon={iconForTool("list_agent_tasks")}
      title="Agent task ledger"
      badge={
        <Badge variant="muted" compact>
          {badgeLabel}
        </Badge>
      }
      compact={compact}
    />
  );

  if (taskCount === 0) {
    return (
      <WidgetCard className={className ?? ""} header={header} compact={compact} alwaysExpanded>
        <span style={{ color: colors.textMuted, fontSize: 10.5 }}>{emptyLabel}</span>
      </WidgetCard>
    );
  }

  return (
    <WidgetCard
      className={className ?? ""}
      header={header}
      compact={compact}
      alwaysExpanded={taskCount <= 4}
    >
      <AgentTaskRows tasks={result.tasks} compact={compact} />
    </WidgetCard>
  );
}

export const AgentTaskWidget = React.memo(function AgentTaskWidget({
  toolCall,
  compact = false,
  className,
}: ToolCallWidgetProps) {
  const canonicalName = canonicalizeToolName(toolCall.name);
  const toolName = isAgentTaskToolName(canonicalName)
    ? canonicalName
    : "list_agent_tasks";
  const result = useMemo(() => parseAgentTaskResult(toolCall.result), [toolCall.result]);
  const isPending = toolCall.result == null && !toolCall.error;
  const error = toolCall.error ?? (result.success === false ? result.error : undefined);

  if (error) {
    return (
      <InlineIndicator
        icon={<TriangleAlert size={12} style={{ color: colors.error }} />}
        text={`${TOOL_LABELS[toolName]} failed: ${error}`}
      />
    );
  }

  if (isPending) {
    return (
      <InlineIndicator
        icon={<ClipboardList size={12} style={{ color: colors.textMuted }} />}
        text={`${TOOL_LABELS[toolName]}...`}
      />
    );
  }

  if (toolName === "list_agent_tasks") {
    return (
      <ListAgentTasksWidget
        result={result}
        includeDone={includeResolvedTasks(toolCall.arguments)}
        compact={compact}
        className={className}
      />
    );
  }

  const task = result.task;
  const title = taskSummaryTitle(toolName, task, toolCall.arguments);
  const state = result.stateChange?.to ?? task?.state;
  const dependencyText = task ? dependencySummary(task) : null;
  const changedFields = result.changedFields.join(", ");
  const bodyRows = [
    task?.details ? <DetailRow key="details" label="Details" value={task.details} /> : null,
    task?.ownerAgent ? <DetailRow key="owner" label="Owner" value={task.ownerAgent} /> : null,
    result.stateChange?.from && result.stateChange.to ? (
      <DetailRow
        key="state"
        label="State"
        value={`${stateLabel(result.stateChange.from)} -> ${stateLabel(result.stateChange.to)}`}
      />
    ) : null,
    changedFields ? <DetailRow key="changed" label="Changed" value={changedFields} /> : null,
    dependencyText ? <DetailRow key="deps" label="Deps" value={dependencyText} /> : null,
  ].filter((row): row is React.ReactElement => row != null);

  if (bodyRows.length === 0) {
    return (
      <div className={className} data-testid="agent-task-widget-inline">
        <WidgetRow compact={compact}>
          <span
            style={{
              width: 14,
              height: 14,
              display: "flex",
              alignItems: "center",
              flexShrink: 0,
            }}
          >
            {iconForTool(toolName)}
          </span>
          <span
            style={{
              flex: 1,
              minWidth: 0,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              color: colors.textSecondary,
              fontSize: compact ? 10.5 : 11,
              fontWeight: 500,
            }}
          >
            {title}
          </span>
          <Badge variant={variantForState(state)} compact>
            {state ? stateLabel(state) : "done"}
          </Badge>
        </WidgetRow>
      </div>
    );
  }

  return (
    <WidgetCard
      className={className ?? ""}
      compact={compact}
      alwaysExpanded
      header={
        <WidgetHeader
          icon={iconForTool(toolName)}
          title={title}
          badge={
            <Badge variant={variantForState(state)} compact>
              {state ? stateLabel(state) : "done"}
            </Badge>
          }
          compact={compact}
        />
      }
    >
      <div style={{ display: "grid", gap: 5 }}>
        {bodyRows}
      </div>
    </WidgetCard>
  );
});
