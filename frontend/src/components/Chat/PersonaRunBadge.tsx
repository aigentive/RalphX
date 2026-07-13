import { CircleDot, TriangleAlert } from "lucide-react";

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";

import { getPersonaSkippedReasonCopy } from "./personaSkippedReason";

export interface PersonaRunBadgeProps {
  enabled: boolean;
  personaSlug: string | null | undefined;
  personaVersion: number | null | undefined;
  personaInjected: boolean | null | undefined;
  skippedReason: string | null | undefined;
}

export function PersonaRunBadge({
  enabled,
  personaSlug,
  personaVersion,
  personaInjected,
  skippedReason,
}: PersonaRunBadgeProps) {
  if (!enabled || !personaSlug || personaInjected == null) {
    return null;
  }

  const applied = personaInjected;
  const tooltip = applied
    ? `${personaSlug}${personaVersion == null ? "" : ` · v${personaVersion}`} — applied to this run`
    : getPersonaSkippedReasonCopy(skippedReason);

  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            className="ml-auto inline-flex min-w-0 items-center gap-1 rounded-full px-1.5 py-0.5 text-[0.625rem] font-medium"
            style={{
              backgroundColor: "var(--bg-elevated)",
              borderColor: applied
                ? "var(--accent-primary)"
                : "var(--status-warning)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: applied
                ? "var(--accent-primary)"
                : "var(--status-warning)",
            }}
            data-testid="persona-run-badge"
            aria-label={tooltip}
          >
            {applied ? (
              <CircleDot className="h-3 w-3 shrink-0" aria-hidden="true" />
            ) : (
              <TriangleAlert className="h-3 w-3 shrink-0" aria-hidden="true" />
            )}
            <span className="max-w-40 truncate">
              {personaSlug}
              {!applied && " not applied"}
            </span>
          </span>
        </TooltipTrigger>
        <TooltipContent>{tooltip}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
