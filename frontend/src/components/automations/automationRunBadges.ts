import type { Automation, AutomationRun } from "@/api/automations";
import {
  describeAutomationStage,
  getAutomationRunJudgeLabel,
  getAutomationRunStatusLabel,
  getAutomationRunStatusTone,
  isOpenAutomationRun,
  type AutomationRunStatusTone,
} from "./automationRunView";

/**
 * De-duplicated badge list for a run card header. This is the single writer for
 * run-card status language: exactly one status badge, a judge badge only while
 * the judge is actually pending/running/failed, and a stage badge only when it
 * adds information beyond the status/judge badges (kills the historical
 * `Running` + `Run 8 in progress` duplication).
 */

export interface AutomationRunCardBadge {
  key: "status" | "judge" | "stage";
  label: string;
  tone: AutomationRunStatusTone;
  live: boolean;
}

const LIVE_STATUS_SET = new Set<AutomationRun["status"]>([
  "pending",
  "provisioning",
  "running",
]);

function judgeBadge(
  automation: Automation,
  run: AutomationRun,
): AutomationRunCardBadge | null {
  const label = getAutomationRunJudgeLabel(run, automation);
  if (!label) {
    return null;
  }
  const state =
    run.status === "awaiting_plan_approval" ? run.planJudgeState : run.judgeState;
  switch (state) {
    case "in_progress":
      return { key: "judge", label, tone: "accent", live: true };
    case "failed":
      return { key: "judge", label, tone: "error", live: false };
    case "none":
      return { key: "judge", label, tone: "neutral", live: false };
    // A settled judge ("complete"/"skipped") is expanded-facts material, not a
    // header badge: the green settled status already tells the story.
    default:
      return null;
  }
}

function stageBadge(
  automation: Automation,
  run: AutomationRun,
  statusLabel: string,
  judgeLabel: string | null,
): AutomationRunCardBadge | null {
  // Stage is a live concept; settled cards must not echo the automation's
  // current stage (e.g. "Scheduling next run") onto historical runs.
  if (!isOpenAutomationRun(run)) {
    return null;
  }
  if (automation.status === "paused") {
    return { key: "stage", label: "Paused", tone: "warning", live: false };
  }
  let label = describeAutomationStage(automation, run);
  if (label === `Run ${run.runIndex} planning`) {
    label = "Planning";
  }
  if (label === `Run ${run.runIndex} in progress`) {
    // Restates the status badge ("Running"/"Pending"/"Provisioning").
    return null;
  }
  if (label === statusLabel || label === judgeLabel) {
    return null;
  }
  return { key: "stage", label, tone: "neutral", live: false };
}

export function getRunCardBadges(
  automation: Automation,
  run: AutomationRun,
): AutomationRunCardBadge[] {
  const statusLabel = getAutomationRunStatusLabel(run);
  const badges: AutomationRunCardBadge[] = [
    {
      key: "status",
      label: statusLabel,
      tone: getAutomationRunStatusTone(run),
      live: isOpenAutomationRun(run) && LIVE_STATUS_SET.has(run.status),
    },
  ];
  const judge = judgeBadge(automation, run);
  if (judge && judge.label !== statusLabel) {
    badges.push(judge);
  }
  const stage = stageBadge(automation, run, statusLabel, judge?.label ?? null);
  if (stage) {
    badges.push(stage);
  }
  return badges;
}
