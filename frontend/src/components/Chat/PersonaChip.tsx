import { useCallback, useMemo, useState } from "react";
import { Check, ChevronDown, CircleDot, TriangleAlert, X } from "lucide-react";

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
import { useConfirmation } from "@/hooks/useConfirmation";
import { usePersonas, useSwitchConversationPersona } from "@/hooks/usePersonas";
import { extractErrorMessage } from "@/lib/errors";
import { cn } from "@/lib/utils";

import { getPersonaSkippedReasonCopy } from "./personaSkippedReason";

const PERSONA_SCOPE_TOOLTIP =
  "Applies to this conversation only — not to delegated, subagent, or pipeline work in v1.";
const RUNNING_SWITCH_CONFIRMATION =
  "Changing the persona stops the current run. Conversation history is preserved and the next message resumes the same session.";

export interface PersonaChipProps {
  conversationId: string;
  personaId: string | null | undefined;
  isAgentRunning: boolean;
  lastRunPersonaId?: string | null;
  lastRunPersonaSlug?: string | null;
  lastRunPersonaInjected?: boolean | null;
  lastRunPersonaSkippedReason?: string | null;
}

export function PersonaChip({
  conversationId,
  personaId,
  isAgentRunning,
  lastRunPersonaId,
  lastRunPersonaSlug,
  lastRunPersonaInjected,
  lastRunPersonaSkippedReason,
}: PersonaChipProps) {
  const { data: personas = [], isLoading } = usePersonas();
  const switchPersona = useSwitchConversationPersona();
  const { confirm, confirmationDialogProps, ConfirmationDialog } =
    useConfirmation();
  const [isOpen, setIsOpen] = useState(false);
  const [switchError, setSwitchError] = useState<string | null>(null);
  const activePersonas = useMemo(
    () => personas.filter((persona) => persona.status === "active"),
    [personas],
  );
  const boundPersona = personas.find((persona) => persona.id === personaId);
  const selectedPersona =
    boundPersona?.status === "active" ? boundPersona : undefined;
  const archivedPersonaSlug =
    boundPersona?.status === "archived"
      ? (lastRunPersonaId === personaId ? lastRunPersonaSlug : null) ??
        boundPersona.slug
      : null;
  const lastRunDidNotApplyBoundPersona =
    personaId != null &&
    lastRunPersonaId === personaId &&
    lastRunPersonaInjected === false;
  const displaySlug =
    (lastRunPersonaId === personaId ? lastRunPersonaSlug : null) ??
    selectedPersona?.slug ??
    archivedPersonaSlug;
  const chipTooltip = lastRunDidNotApplyBoundPersona
    ? getPersonaSkippedReasonCopy(lastRunPersonaSkippedReason)
    : archivedPersonaSlug
      ? `${archivedPersonaSlug} is archived. It remains attributed to the last run.`
      : PERSONA_SCOPE_TOOLTIP;

  const selectPersona = useCallback(
    async (nextPersonaId: string | null) => {
      if (nextPersonaId === personaId || switchPersona.isPending) {
        setIsOpen(false);
        return;
      }

      if (isAgentRunning) {
        const confirmed = await confirm({
          title: "Change persona?",
          description: RUNNING_SWITCH_CONFIRMATION,
          confirmText: "Change persona",
        });
        if (!confirmed) return;
      }

      setSwitchError(null);
      try {
        await switchPersona.mutateAsync({
          conversationId,
          personaId: nextPersonaId,
        });
        setIsOpen(false);
      } catch (error) {
        setSwitchError(
          extractErrorMessage(error, "Could not change the conversation persona."),
        );
      }
    },
    [confirm, conversationId, isAgentRunning, personaId, switchPersona],
  );

  return (
    <div className="flex min-w-0 items-center gap-2" data-testid="persona-chip">
      <Popover open={isOpen} onOpenChange={setIsOpen}>
        <TooltipProvider>
          <Tooltip>
            <TooltipTrigger asChild>
              <PopoverTrigger asChild>
                <button
                  type="button"
                  aria-label="Switch conversation persona"
                  className={cn(
                    "inline-flex shrink-0 items-center gap-1 rounded-full border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-2 py-1 text-xs font-medium text-[var(--text-secondary)] transition-colors hover:border-[var(--accent-primary)] hover:text-[var(--text-primary)]",
                    selectedPersona && "text-[var(--text-primary)]",
                    lastRunDidNotApplyBoundPersona &&
                      "border-[var(--status-warning)] text-[var(--status-warning)]",
                  )}
                >
                  {lastRunDidNotApplyBoundPersona ? (
                    <TriangleAlert
                      className="h-3.5 w-3.5"
                      aria-hidden="true"
                    />
                  ) : (
                    <CircleDot className="h-3.5 w-3.5" aria-hidden="true" />
                  )}
                  <span className="max-w-36 truncate">
                    {displaySlug
                      ? `${displaySlug}${
                          lastRunDidNotApplyBoundPersona
                            ? " not applied"
                            : archivedPersonaSlug
                              ? " (archived)"
                              : ""
                        }`
                      : "No persona"}
                  </span>
                  <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
                </button>
              </PopoverTrigger>
            </TooltipTrigger>
            <TooltipContent>{chipTooltip}</TooltipContent>
          </Tooltip>
        </TooltipProvider>
        <PopoverContent align="end" className="w-64 p-1.5">
          <div role="menu" aria-label="Conversation persona" className="space-y-0.5">
            {isLoading && activePersonas.length === 0 ? (
              <div className="mx-2 my-2 h-7 animate-pulse rounded bg-[var(--bg-elevated)]" />
            ) : (
              activePersonas.map((persona) => (
                <button
                  key={persona.id}
                  type="button"
                  role="menuitemradio"
                  aria-checked={persona.id === personaId}
                  disabled={switchPersona.isPending}
                  onClick={() => void selectPersona(persona.id)}
                  className="flex w-full items-center gap-2 rounded px-2.5 py-2 text-left text-sm text-[var(--text-primary)] hover:bg-[var(--bg-hover)] disabled:cursor-not-allowed disabled:opacity-60"
                >
                  <CircleDot
                    className={cn(
                      "h-3.5 w-3.5 text-[var(--text-muted)]",
                      persona.id === personaId && "text-[var(--accent-primary)]",
                    )}
                    aria-hidden="true"
                  />
                  <span className="min-w-0 flex-1 truncate">{persona.name}</span>
                  {persona.id === personaId && (
                    <Check
                      className="h-3.5 w-3.5 text-[var(--accent-primary)]"
                      aria-hidden="true"
                    />
                  )}
                </button>
              ))
            )}
          </div>
          <div className="mt-1 border-t border-[var(--border-subtle)] pt-1">
            <button
              type="button"
              role="menuitem"
              disabled={personaId == null || switchPersona.isPending}
              onClick={() => void selectPersona(null)}
              className="flex w-full items-center gap-2 rounded px-2.5 py-2 text-left text-sm text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] disabled:cursor-not-allowed disabled:opacity-60"
            >
              <X className="h-3.5 w-3.5" aria-hidden="true" />
              Remove persona
            </button>
          </div>
        </PopoverContent>
      </Popover>
      {switchError && (
        <p role="alert" className="max-w-64 text-xs text-[var(--status-error)]">
          {switchError}
        </p>
      )}
      <ConfirmationDialog {...confirmationDialogProps} />
    </div>
  );
}
