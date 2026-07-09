import { ExternalLink } from "lucide-react";

import type { AutomationRun } from "@/api/automations";
import { openExternalUrl } from "@/lib/open-external";
import { cn } from "@/lib/utils";

export function AutomationRunPrLink({
  run,
  className,
  testId,
}: {
  run: AutomationRun;
  className?: string;
  testId?: string;
}) {
  if (!run.prUrl) {
    return null;
  }
  const prUrl = run.prUrl;
  const label = run.prNumber ? `PR #${run.prNumber}` : "PR";
  return (
    <button
      type="button"
      aria-label={`Open ${label} in browser`}
      className={cn(
        "inline-flex max-w-full items-center gap-1 rounded-full px-2 py-0.5 text-[0.6875rem] font-semibold text-[var(--accent-primary)] outline-none hover:bg-[var(--bg-hover)] focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]",
        className,
      )}
      style={{
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      onClick={() => void openExternalUrl(prUrl)}
      {...(testId ? { "data-testid": testId } : {})}
    >
      <span className="truncate">{label}</span>
      <ExternalLink className="h-3 w-3 shrink-0" aria-hidden="true" />
    </button>
  );
}
