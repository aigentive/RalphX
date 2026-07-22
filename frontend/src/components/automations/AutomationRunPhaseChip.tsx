import { StatusPill } from "@/components/ui/status-pill";

import type { AutomationGoalItem } from "./automationGoalItems";

/** Accent chip for the active goal item (design-system StatusPill). */
export function AutomationRunPhaseChip({
  item,
  testId,
  className,
}: {
  item: AutomationGoalItem;
  testId?: string;
  className?: string;
}) {
  return (
    <StatusPill
      label={item.title}
      tone="accent"
      live
      {...(className ? { className } : {})}
      {...(testId ? { testId } : {})}
    />
  );
}
