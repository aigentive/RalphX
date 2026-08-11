import type { Automation } from "@/api/automations";

/**
 * Shared automation "phase" (goal item) model + parsing used by both the
 * Agents automation panel and the Automations detail view. Centralizes the
 * previously-duplicated status labels, canonical "Phases" heading, and the
 * lenient goal-items parser so the two surfaces stay in sync.
 */

export type AutomationGoalItem = {
  id: string;
  title: string;
  status: string;
};

export const AUTOMATION_STATUS_LABELS: Record<Automation["status"], string> = {
  draft: "Draft",
  active: "Approved",
  paused: "Paused",
  completed: "Completed",
  stopped: "Stopped",
};

/** Canonical UI label for `goal_items_json` across the automation surfaces. */
export const AUTOMATION_PHASES_LABEL = "Phases";

/** Known phase statuses advanced by the judge. */
export type AutomationPhaseStatus =
  | "pending"
  | "in_progress"
  | "done"
  | "skipped";

/** Human labels for each known phase status. */
export const AUTOMATION_PHASE_STATUS_LABELS: Record<
  AutomationPhaseStatus,
  string
> = {
  pending: "Pending",
  in_progress: "In progress",
  done: "Done",
  skipped: "Skipped",
};

/**
 * Coerce a raw phase status string into a known {@link AutomationPhaseStatus}.
 * Unknown / empty values fall back to `pending` so the UI never renders a
 * blank badge.
 */
export function normalizeAutomationPhaseStatus(
  status: string,
): AutomationPhaseStatus {
  switch (status) {
    case "in_progress":
    case "done":
    case "skipped":
      return status;
    default:
      return "pending";
  }
}

/** At-a-glance progress derived from a phase list. */
export interface AutomationPhaseSummary {
  total: number;
  done: number;
  inProgress: number;
  pending: number;
  skipped: number;
  /** Index of the first in-progress phase, or -1 when none is active. */
  currentIndex: number;
  /** `done / total` clamped to 0..1 (0 when there are no phases). */
  progressRatio: number;
}

/**
 * Summarize a parsed phase list into progress counts. Counts are keyed by the
 * normalized status so callers can rely on the four canonical buckets even
 * when the backend emits an unexpected value.
 */
export function summarizeAutomationPhases(
  items: AutomationGoalItem[],
): AutomationPhaseSummary {
  let done = 0;
  let inProgress = 0;
  let pending = 0;
  let skipped = 0;
  let currentIndex = -1;

  items.forEach((item, index) => {
    const status = normalizeAutomationPhaseStatus(item.status);
    switch (status) {
      case "done":
        done += 1;
        break;
      case "in_progress":
        inProgress += 1;
        if (currentIndex === -1) {
          currentIndex = index;
        }
        break;
      case "skipped":
        skipped += 1;
        break;
      default:
        pending += 1;
        break;
    }
  });

  const total = items.length;
  return {
    total,
    done,
    inProgress,
    pending,
    skipped,
    currentIndex,
    progressRatio: total === 0 ? 0 : done / total,
  };
}

/**
 * Parse the raw `goal_items_json` string into normalized phase items.
 *
 * Non-array payloads, invalid JSON, and non-object entries are dropped. Each
 * surviving entry gets a title (`title` || `text` || `Phase N`), an id
 * (`id` || `phase-N`), and a status (defaults to `pending`). An optional
 * `limit` slices the resulting list.
 */
export function parseAutomationGoalItems(
  value: string | null,
  options: { limit?: number } = {},
): AutomationGoalItem[] {
  if (!value?.trim()) {
    return [];
  }
  let items: AutomationGoalItem[];
  try {
    const parsed = JSON.parse(value) as unknown;
    if (!Array.isArray(parsed)) {
      return [];
    }
    items = parsed.flatMap((item, index) => {
      if (!item || typeof item !== "object") {
        return [];
      }
      const record = item as Record<string, unknown>;
      const title =
        typeof record.title === "string" && record.title.trim()
          ? record.title.trim()
          : typeof record.text === "string" && record.text.trim()
            ? record.text.trim()
            : `Phase ${index + 1}`;
      const id =
        typeof record.id === "string" && record.id.trim()
          ? record.id.trim()
          : `phase-${index + 1}`;
      const status =
        typeof record.status === "string" && record.status.trim()
          ? record.status.trim()
          : "pending";
      return [{ id, title, status }];
    });
  } catch {
    return [];
  }
  return options.limit !== undefined ? items.slice(0, options.limit) : items;
}

export function findInProgressAutomationGoalItem(
  value: string | null,
): AutomationGoalItem | null {
  return findInProgressAutomationGoalItemFromItems(parseAutomationGoalItems(value));
}

export function findInProgressAutomationGoalItemFromItems(
  items: AutomationGoalItem[],
): AutomationGoalItem | null {
  return (
    items.find(
      (item) => normalizeAutomationPhaseStatus(item.status) === "in_progress",
    ) ?? null
  );
}
