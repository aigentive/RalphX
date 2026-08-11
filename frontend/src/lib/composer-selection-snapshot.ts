import type { ComposerSelectionSnapshot } from "@/api/chat";

const SOURCE_FALLBACK_LABELS: Record<
  ComposerSelectionSnapshot["sourceKind"],
  string
> = {
  plan: "Plan",
  jira: "Jira",
  linear: "Linear",
  clickup: "ClickUp",
  granola: "Granola",
};

export function getComposerSelectionSourceLabel(
  snapshot: ComposerSelectionSnapshot,
): string {
  return snapshot.sourceKey ?? snapshot.sourceTitle ?? SOURCE_FALLBACK_LABELS[snapshot.sourceKind];
}
