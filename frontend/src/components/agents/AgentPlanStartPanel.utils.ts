import type { AgentComposerPlanReference } from "@/api/agent-composer";

export function planTitle(plan: AgentComposerPlanReference): string {
  return plan.title?.trim() || "Untitled plan";
}

export function samePlanReference(
  a: AgentComposerPlanReference,
  b: AgentComposerPlanReference,
): boolean {
  return a.sessionId === b.sessionId && a.artifactId === b.artifactId;
}

export function previewText(text: string): string {
  return text.length > 4_000 ? `${text.slice(0, 4_000)}\n...` : text;
}
