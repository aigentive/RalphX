import type { AutomationRun } from "@/api/automations";
import { AutomationRunPrLink } from "@/components/automations/AutomationRunPrLink";
import { StatusPill } from "@/components/ui/status-pill";

export function AutomationDetailPrChip({
  run,
  testId,
}: {
  run: AutomationRun;
  testId?: string;
}) {
  if (run.prUrl) {
    return (
      <AutomationRunPrLink
        run={run}
        {...(testId ? { testId } : {})}
      />
    );
  }
  if (run.prNumber === null) {
    return null;
  }
  return (
    <StatusPill
      label={`PR #${run.prNumber}`}
      tone="accent"
      variant="tinted"
      {...(testId ? { testId } : {})}
    />
  );
}
