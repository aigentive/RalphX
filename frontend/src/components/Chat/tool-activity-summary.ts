import type { ToolCall } from "./ToolCallIndicator";
import {
  extractDelegationMetadata,
  isDelegationControlToolCall,
  isDelegationStartToolCall,
} from "./delegation-tool-calls";
import { canonicalizeToolName } from "./tool-widgets/tool-name";

export interface ToolActivityTask {
  toolUseId: string;
  toolName: string;
  delegatedJobId?: string;
}

export interface ToolActivitySummary {
  totalTools: number;
  createdPaths: string[];
  editedPaths: string[];
  changedPaths: string[];
  delegatedJobKeys: string[];
}

interface SummarizeToolActivityArgs {
  toolCalls?: readonly ToolCall[];
  tasks?: readonly ToolActivityTask[];
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value != null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function normalizePath(path: string): string {
  return path.trim().replace(/\\/g, "/").replace(/\/{2,}/g, "/");
}

function mutationPath(toolCall: ToolCall): string | null {
  const canonicalName = canonicalizeToolName(toolCall.name);
  if (canonicalName !== "edit" && canonicalName !== "write") {
    return null;
  }

  const diffPath = toolCall.diffContext?.filePath;
  if (diffPath?.trim()) {
    return normalizePath(diffPath);
  }

  const args = asRecord(toolCall.arguments);
  const candidate = args?.file_path ?? args?.filePath ?? args?.path;
  return typeof candidate === "string" && candidate.trim()
    ? normalizePath(candidate)
    : null;
}

function addMutation(
  toolCall: ToolCall,
  createdPaths: Set<string>,
  editedPaths: Set<string>,
  changedPaths: Set<string>,
): void {
  const path = mutationPath(toolCall);
  if (!path) return;

  const canonicalName = canonicalizeToolName(toolCall.name);
  const isCreated = canonicalName === "write" && toolCall.diffContext?.oldFileExists === false;
  const isEdited = canonicalName === "edit"
    || (canonicalName === "write" && toolCall.diffContext?.oldFileExists === true);

  if (isCreated) {
    createdPaths.add(path);
    editedPaths.delete(path);
    changedPaths.delete(path);
  } else if (isEdited) {
    if (!createdPaths.has(path)) {
      editedPaths.add(path);
      changedPaths.delete(path);
    }
  } else if (!createdPaths.has(path) && !editedPaths.has(path)) {
    changedPaths.add(path);
  }
}

export function summarizeToolActivity({
  toolCalls = [],
  tasks = [],
}: SummarizeToolActivityArgs): ToolActivitySummary {
  const seenToolIds = new Set<string>();
  const createdPaths = new Set<string>();
  const editedPaths = new Set<string>();
  const changedPaths = new Set<string>();
  const delegatedJobKeys = new Set<string>();
  let totalTools = 0;

  toolCalls.forEach((toolCall, index) => {
    if (isDelegationControlToolCall(toolCall.name)) {
      return;
    }
    const rawLogicalKey = toolCall.id.trim() || `tool-index:${index}`;
    const delegationMetadata = isDelegationStartToolCall(toolCall.name)
      ? extractDelegationMetadata(toolCall.arguments, toolCall.result)
      : null;
    const logicalKey = delegationMetadata?.jobId
      ? `delegation-job:${delegationMetadata.jobId}`
      : rawLogicalKey;
    if (seenToolIds.has(logicalKey)) {
      return;
    }
    seenToolIds.add(logicalKey);
    totalTools += 1;
    addMutation(toolCall, createdPaths, editedPaths, changedPaths);

    if (isDelegationStartToolCall(toolCall.name)) {
      delegatedJobKeys.add(delegationMetadata?.jobId ?? rawLogicalKey);
    }
  });

  tasks.forEach((task, index) => {
    const rawLogicalKey = task.toolUseId.trim() || `task-index:${index}`;
    const delegatedJobId = isDelegationStartToolCall(task.toolName)
      ? task.delegatedJobId?.trim()
      : undefined;
    const logicalKey = delegatedJobId
      ? `delegation-job:${delegatedJobId}`
      : rawLogicalKey;
    if (seenToolIds.has(logicalKey)) {
      return;
    }
    seenToolIds.add(logicalKey);
    totalTools += 1;
    if (isDelegationStartToolCall(task.toolName)) {
      delegatedJobKeys.add(delegatedJobId || rawLogicalKey);
    }
  });

  return {
    totalTools,
    createdPaths: [...createdPaths],
    editedPaths: [...editedPaths],
    changedPaths: [...changedPaths],
    delegatedJobKeys: [...delegatedJobKeys],
  };
}

function countLabel(count: number, singular: string, plural: string): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

function joinClauses(clauses: string[]): string {
  if (clauses.length <= 1) return clauses[0] ?? "";
  if (clauses.length === 2) return `${clauses[0]} and ${clauses[1]}`;
  return `${clauses.slice(0, -1).join(", ")}, and ${clauses[clauses.length - 1]}`;
}

export function formatToolActivitySummary(summary: ToolActivitySummary): string {
  const clauses = [
    `Agent called ${countLabel(summary.totalTools, "tool", "tools")}`,
  ];
  if (summary.createdPaths.length > 0) {
    clauses.push(`created ${countLabel(summary.createdPaths.length, "file", "files")}`);
  }
  if (summary.editedPaths.length > 0) {
    clauses.push(`edited ${countLabel(summary.editedPaths.length, "file", "files")}`);
  }
  if (summary.changedPaths.length > 0) {
    clauses.push(`changed ${countLabel(summary.changedPaths.length, "file", "files")}`);
  }
  if (summary.delegatedJobKeys.length > 0) {
    clauses.push(`delegated ${countLabel(summary.delegatedJobKeys.length, "agent", "agents")}`);
  }
  return `${joinClauses(clauses)}.`;
}
