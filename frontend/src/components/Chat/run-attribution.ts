import { formatDuration } from "@/components/tasks/detail-views/shared/DurationDisplay";

export function roleVerb(launchRole: string | null | undefined): "Reviewer" | "Fixer" | "Agent" {
  if (launchRole === "workspace_reviewer") return "Reviewer";
  if (launchRole === "workspace_repair" || launchRole === "pr_fixer") return "Fixer";
  return "Agent";
}

export function workedDurationLabel(
  startedAt: string | number | null | undefined,
  completedAt: string | number | null | undefined,
): string {
  const start = typeof startedAt === "number" ? startedAt : Date.parse(startedAt ?? "");
  const end = typeof completedAt === "number" ? completedAt : Date.parse(completedAt ?? "");
  if (Number.isNaN(start) || Number.isNaN(end)) return formatDuration(0);
  return formatDuration(Math.max(0, Math.floor((end - start) / 1000)));
}

export function isFeedbackRole(launchRole: string | null | undefined): boolean {
  return launchRole === "workspace_reviewer" || launchRole === "workspace_repair" || launchRole === "pr_fixer";
}

export function elapsedLabel(startedAt: number, launchRole: string | null | undefined, now = Date.now()) {
  return `${roleVerb(launchRole)} working for ${formatDuration(Math.max(0, Math.floor((now - startedAt) / 1000)))}`;
}
