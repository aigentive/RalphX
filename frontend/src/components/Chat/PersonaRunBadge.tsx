import { useState } from "react";
import { CircleDot, TriangleAlert } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useUiStore } from "@/stores/uiStore";

import { getPersonaSkippedReasonCopy } from "./personaSkippedReason";

export interface PersonaRunBadgeProps {
  enabled: boolean;
  personaId?: string | null | undefined;
  personaSlug: string | null | undefined;
  personaVersion: number | null | undefined;
  personaInjected: boolean | null | undefined;
  skippedReason: string | null | undefined;
}

export function PersonaRunBadge({
  enabled,
  personaId,
  personaSlug,
  personaVersion,
  personaInjected,
  skippedReason,
}: PersonaRunBadgeProps) {
  const [detailsOpen, setDetailsOpen] = useState(false);
  const openModal = useUiStore((state) => state.openModal);

  if (!enabled || !personaSlug || personaInjected == null) {
    return null;
  }

  const applied = personaInjected;
  const tooltip = applied
    ? `${personaSlug}${personaVersion == null ? "" : ` · v${personaVersion}`} — applied to this run`
    : getPersonaSkippedReasonCopy(skippedReason);

  const badge = (
    <button
      type="button"
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
    </button>
  );

  return (
    <TooltipProvider>
      <Popover open={detailsOpen} onOpenChange={setDetailsOpen}>
        <Tooltip>
          <TooltipTrigger asChild>
            <PopoverTrigger asChild>{badge}</PopoverTrigger>
          </TooltipTrigger>
          <TooltipContent>{tooltip}</TooltipContent>
        </Tooltip>
        <PopoverContent
          align="end"
          className="w-64 p-3"
          data-testid="persona-run-badge-details"
        >
          <p className="text-sm font-medium text-[var(--text-primary)]">
            {personaSlug}
            {personaVersion != null && (
              <span className="ml-1 text-xs text-[var(--text-muted)]">
                v{personaVersion}
              </span>
            )}
          </p>
          <p className="mt-1 text-xs text-[var(--text-secondary)]">
            {applied
              ? "Applied to this run."
              : getPersonaSkippedReasonCopy(skippedReason)}
          </p>
          {personaId && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="mt-2 w-full"
              onClick={() => {
                setDetailsOpen(false);
                openModal("settings", {
                  section: "personas",
                  personaId,
                });
              }}
            >
              Open persona
            </Button>
          )}
        </PopoverContent>
      </Popover>
    </TooltipProvider>
  );
}
