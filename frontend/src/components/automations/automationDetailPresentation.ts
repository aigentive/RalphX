import type { AutomationRun } from "@/api/automations";
import type { AutomationGoalItem } from "./automationGoalItems";
import { isOpenAutomationRun } from "./automationStage";

export interface AutomationPhaseGroup {
  key: string;
  label: string | null;
  items: AutomationGoalItem[];
}

function newestFirst(runs: AutomationRun[]): AutomationRun[] {
  return [...runs].sort((left, right) => right.runIndex - left.runIndex);
}

function isMergedRun(run: AutomationRun): boolean {
  return run.status === "merged" || run.prMergedAt !== null;
}

export function getTrailingFailureStreak(runs: AutomationRun[]): number {
  let failures = 0;
  for (const run of newestFirst(runs)) {
    if (run.status === "agent_failed") {
      failures += 1;
      continue;
    }
    if (isOpenAutomationRun(run)) {
      continue;
    }
    break;
  }
  return failures;
}

export function getLatestMergedRun(runs: AutomationRun[]): AutomationRun | null {
  return newestFirst(runs).find(isMergedRun) ?? null;
}

export function getMergedPrByGoalItem(
  runs: AutomationRun[],
): ReadonlyMap<string, AutomationRun> {
  const mergedByGoalItem = new Map<string, AutomationRun>();
  for (const run of newestFirst(runs)) {
    if (
      isMergedRun(run)
      && run.goalItemId
      && run.prNumber !== null
      && !mergedByGoalItem.has(run.goalItemId)
    ) {
      mergedByGoalItem.set(run.goalItemId, run);
    }
  }
  return mergedByGoalItem;
}

export function getPlanArtifactByGoalItem(
  runs: AutomationRun[],
): ReadonlyMap<string, string> {
  const planByGoalItem = new Map<string, string>();
  for (const run of newestFirst(runs)) {
    if (
      run.goalItemId
      && run.planArtifactId
      && !planByGoalItem.has(run.goalItemId)
    ) {
      planByGoalItem.set(run.goalItemId, run.planArtifactId);
    }
  }
  return planByGoalItem;
}

export function getAutomationPhaseGroups(
  items: AutomationGoalItem[],
): AutomationPhaseGroup[] {
  const parsed = items.map((item) => ({
    item,
    match: /^([A-Z]+)\d+$/.exec(item.id),
  }));
  const conventionalCount = parsed.filter(({ match }) => match !== null).length;
  if (conventionalCount * 2 <= items.length) {
    return [{ key: "all", label: null, items }];
  }

  const groups = new Map<string, AutomationGoalItem[]>();
  for (const { item, match } of parsed) {
    const key = match?.[1] ?? "other";
    const current = groups.get(key) ?? [];
    current.push(item);
    groups.set(key, current);
  }

  return [...groups.entries()].map(([key, groupedItems]) => ({
    key,
    label: key === "other" ? "Other" : key,
    items: groupedItems,
  }));
}
