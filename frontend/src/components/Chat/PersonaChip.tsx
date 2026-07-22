import { useCallback, useMemo, useState } from "react";
import {
  ChevronDown,
  CircleDot,
  FileSearch,
  Sparkles,
  TriangleAlert,
  X,
} from "lucide-react";

import { PersonaMenuList } from "@/components/personas/PersonaMenuList";
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

import { PersonaInjectedPromptDialog } from "./PersonaInjectedPromptDialog";
import { getPersonaSkippedReasonCopy } from "./personaSkippedReason";

const PERSONA_SCOPE_TOOLTIP =
  "Applies to this conversation only — not to delegated, subagent, or pipeline work in v1.";
const RUNNING_SWITCH_CONFIRMATION =
  "Changing the persona stops the current run. Conversation history is preserved and the next message resumes the same session.";

export interface PersonaChipProps {
  conversationId: string;
  projectId: string;
  projectName: string;
  personaId: string | null | undefined;
  isAgentRunning: boolean;
  lastRunPersonaId?: string | null;
  lastRunPersonaSlug?: string | null;
  lastRunPersonaVersion?: number | null;
  lastRunPersonaInjected?: boolean | null;
  lastRunPersonaSkippedReason?: string | null;
  onBuildPersona?: () => void;
}

export function PersonaChip({
  conversationId,
  projectId,
  projectName,
  personaId,
  isAgentRunning,
  lastRunPersonaId,
  lastRunPersonaSlug,
  lastRunPersonaVersion,
  lastRunPersonaInjected,
  lastRunPersonaSkippedReason,
  onBuildPersona,
}: PersonaChipProps) {
  const scope = useMemo(
    () => ({ type: "globalAndProject", projectId }) as const,
    [projectId],
  );
  const { data: personas = [] } = usePersonas(scope);
  const switchPersona = useSwitchConversationPersona();
  const { confirm, confirmationDialogProps, ConfirmationDialog } =
    useConfirmation();
  const [isOpen, setIsOpen] = useState(false);
  const [injectedPromptOpen, setInjectedPromptOpen] = useState(false);
  const [switchError, setSwitchError] = useState<string | null>(null);
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
    lastRunPersonaInjected === false &&
    (lastRunPersonaSkippedReason?.trim().length ?? 0) > 0;
  const lastRunMatchesBoundPersona = lastRunPersonaId === personaId;
  const displaySlug =
    (lastRunMatchesBoundPersona ? lastRunPersonaSlug : null) ??
    selectedPersona?.slug ??
    archivedPersonaSlug;
  const displayVersion = lastRunMatchesBoundPersona
    ? (lastRunPersonaVersion ?? null)
    : (selectedPersona?.version ?? null);
  const chipTooltip = lastRunDidNotApplyBoundPersona
    ? getPersonaSkippedReasonCopy(lastRunPersonaSkippedReason)
    : archivedPersonaSlug
      ? `${archivedPersonaSlug} is archived. It remains attributed to the last run.`
      : PERSONA_SCOPE_TOOLTIP;
  const appliedLastRun =
    lastRunMatchesBoundPersona &&
    lastRunPersonaInjected === true &&
    lastRunPersonaSlug;

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
                  <span className="max-w-40 truncate">
                    {displaySlug
                      ? `${displaySlug}${
                          displayVersion != null ? ` v${displayVersion}` : ""
                        }${
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
        <PopoverContent align="end" className="w-72 p-1.5">
          <PersonaMenuList
            projectId={projectId}
            projectName={projectName}
            selectedPersonaId={selectedPersona?.id ?? null}
            disabled={switchPersona.isPending}
            showNoPersona={false}
            onSelect={(nextPersonaId) => void selectPersona(nextPersonaId)}
          />
          <div className="mt-1 border-t border-[var(--border-subtle)] pt-1">
            {onBuildPersona && (
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setIsOpen(false);
                  onBuildPersona();
                }}
                className="flex w-full items-center gap-2 rounded px-2.5 py-2 text-left text-sm font-medium text-[var(--accent-primary)] hover:bg-[var(--bg-hover)]"
              >
                <Sparkles className="h-3.5 w-3.5" aria-hidden="true" />
                Create persona for this project
              </button>
            )}
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
          <div className="mt-1 border-t border-[var(--border-subtle)] pt-1">
            {appliedLastRun && (
              <p className="px-2.5 py-1 text-xs text-[var(--text-muted)]">
                Applied last run: {lastRunPersonaSlug}
                {lastRunPersonaVersion != null && ` v${lastRunPersonaVersion}`}
              </p>
            )}
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                // Shell-first: the popover closes and the dialog opens in the
                // same click commit; the preview fetch happens after.
                setIsOpen(false);
                setInjectedPromptOpen(true);
              }}
              className="flex w-full items-center gap-2 rounded px-2.5 py-2 text-left text-sm text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]"
            >
              <FileSearch className="h-3.5 w-3.5" aria-hidden="true" />
              View injected prompt
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
      <PersonaInjectedPromptDialog
        conversationId={conversationId}
        open={injectedPromptOpen}
        onOpenChange={setInjectedPromptOpen}
      />
    </div>
  );
}
