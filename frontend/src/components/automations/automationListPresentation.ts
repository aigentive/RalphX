import type { Automation } from "@/api/automations";

export type AutomationListFilter = "all" | "attention" | "running" | "finished" | "drafts";
export type AutomationListGroupId = Exclude<AutomationListFilter, "all">;

export interface AutomationListGroupDefinition {
  id: AutomationListGroupId;
  label: string;
  hint: string;
}

export interface AutomationListGroup extends AutomationListGroupDefinition {
  automations: Automation[];
}

export const AUTOMATION_LIST_GROUPS: readonly AutomationListGroupDefinition[] = [
  { id: "attention", label: "Needs attention", hint: "Paused after failures — resume, edit, or stop" },
  { id: "running", label: "Running", hint: "Automation work in progress" },
  { id: "finished", label: "Finished", hint: "Completed or stopped automations" },
  { id: "drafts", label: "Drafts", hint: "No first run yet" },
];

const GROUP_STATUSES: Record<AutomationListGroupId, readonly Automation["status"][]> = {
  attention: ["paused"],
  running: ["active"],
  finished: ["completed", "stopped"],
  drafts: ["draft"],
};

export function automationListFilterCounts(
  automations: Automation[],
): Record<AutomationListFilter, number> {
  return {
    all: automations.length,
    attention: automations.filter((automation) => GROUP_STATUSES.attention.includes(automation.status)).length,
    running: automations.filter((automation) => GROUP_STATUSES.running.includes(automation.status)).length,
    finished: automations.filter((automation) => GROUP_STATUSES.finished.includes(automation.status)).length,
    drafts: automations.filter((automation) => GROUP_STATUSES.drafts.includes(automation.status)).length,
  };
}

export function filterAndGroupAutomations(
  automations: Automation[],
  filter: AutomationListFilter,
  searchText: string,
): AutomationListGroup[] {
  const query = searchText.trim().toLocaleLowerCase();
  return AUTOMATION_LIST_GROUPS.flatMap((group) => {
    if (filter !== "all" && filter !== group.id) {
      return [];
    }
    const grouped = automations.filter((automation) =>
      GROUP_STATUSES[group.id].includes(automation.status),
    );
    const searched = query
      ? grouped.filter((automation) => automation.name.toLocaleLowerCase().includes(query))
      : grouped;
    return searched.length > 0 ? [{ ...group, automations: searched }] : [];
  });
}

export function automationListSummary(projectName: string, automations: Automation[]): string {
  const counts = automationListFilterCounts(automations);
  return `${projectName} · ${counts.all} automations · ${counts.running} running · ${counts.attention} needs attention`;
}
