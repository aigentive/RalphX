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
