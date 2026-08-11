import type { StateTransition } from "@/api/tasks";
import { isTerminalStatus } from "@/types/status";
import type { InternalStatus } from "@/types/task";
import type {
  TaskHistoryState,
  TaskRuntimeHistoryContextType,
} from "@/types/task-history";
import type { StatusTokenKey } from "@/lib/theme-colors";

type HistoryStatusColor = StatusTokenKey | "muted";
type StageAttemptFamily = "execution" | "review" | "merge";

export interface TaskHistoryEntry extends TaskHistoryState {
  isCurrent: boolean;
  label: string;
}

const STATUS_CONFIG: Record<InternalStatus, { label: string; color: HistoryStatusColor }> = {
  backlog: { label: "Backlog", color: "muted" },
  ready: { label: "Ready", color: "info" },
  blocked: { label: "Blocked", color: "warning" },
  executing: { label: "Executing", color: "accent" },
  qa_refining: { label: "QA Refining", color: "accent" },
  qa_testing: { label: "QA Testing", color: "accent" },
  qa_passed: { label: "QA Passed", color: "success" },
  qa_failed: { label: "QA Failed", color: "error" },
  pending_review: { label: "Pending Review", color: "muted" },
  revision_needed: { label: "Revision Needed", color: "warning" },
  approved: { label: "Approved", color: "success" },
  failed: { label: "Failed", color: "error" },
  cancelled: { label: "Cancelled", color: "muted" },
  reviewing: { label: "Reviewing", color: "info" },
  review_passed: { label: "Review Passed", color: "success" },
  escalated: { label: "Escalated", color: "warning" },
  re_executing: { label: "Re-executing", color: "warning" },
  pending_merge: { label: "Pending Merge", color: "accent" },
  merging: { label: "Merging", color: "accent" },
  waiting_on_pr: { label: "Waiting on PR", color: "info" },
  merge_incomplete: { label: "Merge Incomplete", color: "warning" },
  merge_conflict: { label: "Merge Conflict", color: "warning" },
  updating_plan_branch: { label: "Updating Plan Branch", color: "info" },
  updating_task_branch: { label: "Updating Task Branch", color: "info" },
  branch_update_blocked: { label: "Branch Update Blocked", color: "warning" },
  merged: { label: "Merged", color: "success" },
  paused: { label: "Paused", color: "warning" },
  stopped: { label: "Stopped", color: "error" },
};

const TRANSIENT_STATUSES = new Set<InternalStatus>(["ready", "pending_review", "pending_merge"]);
const INTERMEDIATE_RETRY_STATUSES = new Set<InternalStatus>([
  "merge_incomplete",
  "merge_conflict",
  "revision_needed",
  "qa_failed",
  "blocked",
  "paused",
]);
const EXECUTION_CONTEXT_STATUSES = new Set<InternalStatus>([
  "executing",
  "re_executing",
  "qa_refining",
  "qa_testing",
  "qa_passed",
  "qa_failed",
]);
const REVIEW_CONTEXT_STATUSES = new Set<InternalStatus>([
  "pending_review",
  "reviewing",
  "review_passed",
  "revision_needed",
  "approved",
  "escalated",
]);
const MERGE_CONTEXT_STATUSES = new Set<InternalStatus>([
  "pending_merge",
  "merging",
  "waiting_on_pr",
  "merge_incomplete",
  "merge_conflict",
  "merged",
]);

export function getTaskHistoryStatusConfig(status: InternalStatus) {
  return STATUS_CONFIG[status];
}

function deriveContextType(
  status: InternalStatus,
  explicitContextType?: TaskRuntimeHistoryContextType,
): TaskRuntimeHistoryContextType | undefined {
  if (explicitContextType) return explicitContextType;
  if (EXECUTION_CONTEXT_STATUSES.has(status)) return "task_execution";
  if (REVIEW_CONTEXT_STATUSES.has(status)) return "review";
  if (MERGE_CONTEXT_STATUSES.has(status)) return "merge";
  return undefined;
}

function getAttemptFamily(status: InternalStatus): StageAttemptFamily | null {
  if (EXECUTION_CONTEXT_STATUSES.has(status)) return "execution";
  if (status === "reviewing") return "review";
  if (MERGE_CONTEXT_STATUSES.has(status) && status !== "pending_merge") return "merge";
  return null;
}

function isAttemptStartStatus(status: InternalStatus): boolean {
  return status === "executing" || status === "re_executing" || status === "reviewing" || status === "merging";
}

function formatAttemptLabel(
  family: StageAttemptFamily | null,
  attemptIndex: number | undefined,
  fallbackLabel: string,
): string {
  if (!family || attemptIndex === undefined) return fallbackLabel;
  const familyLabel = family === "execution" ? "Execution" : family === "review" ? "Review" : "Merge";
  return `${familyLabel} attempt ${attemptIndex}`;
}

function shouldShowTransition(status: InternalStatus, currentStatus: InternalStatus): boolean {
  if (TRANSIENT_STATUSES.has(status) && status !== currentStatus) return false;
  return !(isTerminalStatus(currentStatus) && INTERMEDIATE_RETRY_STATUSES.has(status) && status !== currentStatus);
}

export function getTaskHistoryEntryKey(entry: Pick<TaskHistoryEntry, "transitionId" | "status" | "timestamp">): string {
  return entry.transitionId ?? `${entry.status}-${entry.timestamp}`;
}

export function isSelectedTaskHistoryEntry(
  entry: TaskHistoryEntry,
  selectedState: TaskHistoryState | null | undefined,
): boolean {
  if (!selectedState || entry.isCurrent) return false;
  return Boolean(
    (selectedState.transitionId && entry.transitionId === selectedState.transitionId) ||
      (entry.status === selectedState.status && entry.timestamp === selectedState.timestamp),
  );
}

export function entryToHistoryState(entry: TaskHistoryEntry): TaskHistoryState {
  const { isCurrent: _isCurrent, label: _label, ...state } = entry;
  return state;
}

export function buildTaskHistoryEntries(
  transitions: StateTransition[] | undefined,
  currentStatus: InternalStatus,
): TaskHistoryEntry[] {
  if (!transitions || transitions.length === 0) {
    if (TRANSIENT_STATUSES.has(currentStatus)) return [];
    const family = getAttemptFamily(currentStatus);
    const timestamp = new Date().toISOString();
    const contextType = deriveContextType(currentStatus);
    return [{
      status: currentStatus,
      timestamp,
      isCurrent: true,
      label: formatAttemptLabel(family, family ? 1 : undefined, STATUS_CONFIG[currentStatus].label),
      ...(contextType !== undefined && { contextType }),
      transitionId: `${currentStatus}-${timestamp}`,
      ...(family && { attemptIndex: 1 }),
      hasConversation: false,
    }];
  }

  const attemptCounts: Record<StageAttemptFamily, number> = { execution: 0, review: 0, merge: 0 };
  let activeAttemptFamily: StageAttemptFamily | null = null;
  const entries: TaskHistoryEntry[] = [];

  for (const transition of transitions) {
    if (!shouldShowTransition(transition.toStatus, currentStatus)) continue;
    const family = getAttemptFamily(transition.toStatus);
    if (family) {
      if (activeAttemptFamily !== family || isAttemptStartStatus(transition.toStatus) || attemptCounts[family] === 0) {
        attemptCounts[family] += 1;
      }
      activeAttemptFamily = family;
    } else {
      activeAttemptFamily = null;
    }
    const attemptIndex = family ? attemptCounts[family] : undefined;
    const contextType = deriveContextType(transition.toStatus, transition.contextType);
    entries.push({
      status: transition.toStatus,
      timestamp: transition.timestamp,
      isCurrent: false,
      label: formatAttemptLabel(family, attemptIndex, STATUS_CONFIG[transition.toStatus].label),
      ...(transition.conversationId !== undefined && { conversationId: transition.conversationId }),
      ...(transition.agentRunId !== undefined && { agentRunId: transition.agentRunId }),
      ...(contextType !== undefined && { contextType }),
      transitionId: transition.transitionId ?? `${transition.toStatus}-${transition.timestamp}`,
      ...(attemptIndex !== undefined && { attemptIndex }),
      hasConversation: Boolean(transition.conversationId),
    });
  }

  const currentIndex = entries.map((entry) => entry.status).lastIndexOf(currentStatus);
  if (currentIndex === -1 && !TRANSIENT_STATUSES.has(currentStatus)) {
    const family = getAttemptFamily(currentStatus);
    const timestamp = new Date().toISOString();
    const attemptIndex = family ? (attemptCounts[family] || 0) + 1 : undefined;
    const contextType = deriveContextType(currentStatus);
    entries.push({
      status: currentStatus,
      timestamp,
      isCurrent: true,
      label: formatAttemptLabel(family, attemptIndex, STATUS_CONFIG[currentStatus].label),
      ...(contextType !== undefined && { contextType }),
      transitionId: `${currentStatus}-${timestamp}`,
      ...(attemptIndex !== undefined && { attemptIndex }),
      hasConversation: false,
    });
  } else if (currentIndex >= 0) {
    const currentEntry = entries[currentIndex];
    if (currentEntry) entries[currentIndex] = { ...currentEntry, isCurrent: true };
  }
  return entries;
}
