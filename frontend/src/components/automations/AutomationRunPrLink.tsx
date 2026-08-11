import { ExternalLink } from "lucide-react";

import type { AutomationRun } from "@/api/automations";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { openExternalUrl } from "@/lib/open-external";
import { cn } from "@/lib/utils";

function isHttpPrUrl(value: string): boolean {
  try {
    const parsed = new URL(value);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

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
  const classNameValue = cn(
    "inline-flex max-w-full items-center gap-1 rounded-full px-2 py-0.5 text-[0.6875rem] font-semibold text-[var(--accent-primary)] outline-none",
    className,
  );
  if (!isHttpPrUrl(prUrl)) {
    return (
      <TooltipProvider delayDuration={150}>
        <Tooltip>
          <TooltipTrigger asChild>
            <span
              tabIndex={0}
              aria-disabled="true"
              className={cn(
                classNameValue,
                "cursor-not-allowed opacity-70 focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]",
              )}
              style={{
                borderColor: "var(--border-default)",
                borderStyle: "solid",
                borderWidth: "1px",
              }}
              {...(testId ? { "data-testid": testId } : {})}
            >
              <span className="truncate">{label}</span>
              <ExternalLink className="h-3 w-3 shrink-0" aria-hidden="true" />
            </span>
          </TooltipTrigger>
          <TooltipContent side="top" className="text-xs">
            Only HTTP(S) PR links can be opened.
          </TooltipContent>
        </Tooltip>
      </TooltipProvider>
    );
  }
  return (
    <button
      type="button"
      aria-label={`Open ${label} in browser`}
      className={cn(
        classNameValue,
        "hover:bg-[var(--bg-hover)] focus-visible:ring-2 focus-visible:ring-[var(--accent-primary)]",
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
