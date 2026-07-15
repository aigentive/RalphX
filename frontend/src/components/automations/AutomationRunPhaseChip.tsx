import { cn } from "@/lib/utils";

import type { AutomationGoalItem } from "./automationGoalItems";

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
    <span
      className={cn(
        "inline-flex min-w-0 max-w-full items-center rounded-full px-2 py-0.5 text-[0.6875rem] font-semibold",
        className,
      )}
      style={{
        color: "var(--accent-primary, #ff6a35)",
        backgroundColor: "var(--accent-muted)",
        borderColor: "var(--accent-border)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      {...(testId ? { "data-testid": testId } : {})}
    >
      <span className="truncate">{item.title}</span>
    </span>
  );
}
