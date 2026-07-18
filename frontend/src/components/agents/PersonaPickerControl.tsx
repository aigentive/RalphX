import { useCallback, useMemo } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Check, CircleDot } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { fetchPersonas, personaKeys, usePersonas } from "@/hooks/usePersonas";
import { cn } from "@/lib/utils";
import type { Persona } from "@/types/persona";

export interface PersonaPickerControlProps {
  currentProjectId: string;
  currentProjectName: string;
  personaId: string | null;
  onValueChange: (personaId: string | null) => void;
  onOpenPersonas: () => void;
}

export function PersonaPickerControl({
  currentProjectId,
  currentProjectName,
  personaId,
  onValueChange,
  onOpenPersonas,
}: PersonaPickerControlProps) {
  const queryClient = useQueryClient();
  const scope = useMemo(
    () => ({ type: "globalAndProject", projectId: currentProjectId }) as const,
    [currentProjectId],
  );
  const { data: personas = [], isLoading, isError, refetch } = usePersonas(scope);
  const activePersonas = useMemo(
    () =>
      personas.filter(
        (persona) =>
          persona.status === "active" &&
          (persona.projectId === null || persona.projectId === currentProjectId),
      ),
    [currentProjectId, personas],
  );
  const globalPersonas = activePersonas.filter((persona) => persona.projectId === null);
  const projectPersonas = activePersonas.filter(
    (persona) => persona.projectId === currentProjectId,
  );
  const selectedPersona = activePersonas.find(
    (persona) => persona.id === personaId,
  );
  const tooltipLabel = selectedPersona
    ? `Persona: ${selectedPersona.name}`
    : "Choose persona";
  const warmPersonas = useCallback(() => {
    void queryClient.prefetchQuery({
      queryKey: personaKeys.list(scope),
      queryFn: () => fetchPersonas(scope),
    });
  }, [queryClient, scope]);

  return (
    <Popover>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label="Choose persona"
              data-testid="persona-picker-trigger"
              onPointerEnter={warmPersonas}
              onFocus={warmPersonas}
              className={cn(
                "relative shrink-0 text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]",
                selectedPersona && "text-[var(--accent-primary)]",
              )}
            >
              <CircleDot className="h-4 w-4" aria-hidden="true" />
              {selectedPersona && (
                <span
                  aria-hidden="true"
                  className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-[var(--accent-primary)]"
                />
              )}
            </Button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent>{tooltipLabel}</TooltipContent>
      </Tooltip>
      <PopoverContent
        data-testid="persona-picker-popover"
        align="end"
        className="w-64 p-1.5"
      >
        <div role="menu" aria-label="Choose persona" className="space-y-0.5">
          {isError && (
            <div className="mx-1 my-1 flex items-center justify-between gap-2 rounded px-2 py-2 text-xs text-[var(--status-error)]">
              <span>Couldn't load personas.</span>
              <button
                type="button"
                className="font-medium underline underline-offset-2"
                aria-label="Retry personas"
                onClick={() => void refetch()}
              >
                Retry
              </button>
            </div>
          )}
          {isLoading && activePersonas.length === 0 ? (
            <div
              data-testid="persona-picker-loading"
              className="mx-2 my-2 h-7 animate-pulse rounded bg-[var(--bg-elevated)]"
            />
          ) : (
            <>
              {!isError && (
                <PersonaOption
                  label="No persona"
                  selected={personaId === null}
                  onSelect={() => onValueChange(null)}
                />
              )}
              <PersonaOptionGroup
                label="Global"
                personas={globalPersonas}
                personaId={personaId}
                onValueChange={onValueChange}
              />
              <PersonaOptionGroup
                label={currentProjectName}
                personas={projectPersonas}
                personaId={personaId}
                onValueChange={onValueChange}
              />
            </>
          )}
        </div>
        <div className="mt-1 border-t border-[var(--border-subtle)] pt-1">
          <button
            type="button"
            className="flex w-full items-center rounded px-2.5 py-2 text-left text-xs font-medium text-[var(--accent-primary)] hover:bg-[var(--bg-hover)]"
            onClick={onOpenPersonas}
          >
            Manage personas <span aria-hidden="true" className="ml-1">→</span>
          </button>
        </div>
      </PopoverContent>
    </Popover>
  );
}

function PersonaOptionGroup({
  label,
  personas,
  personaId,
  onValueChange,
}: {
  label: string;
  personas: Persona[];
  personaId: string | null;
  onValueChange: (personaId: string | null) => void;
}) {
  if (!personas || personas.length === 0) return null;
  return (
    <div role="group" aria-label={label} className="pt-1">
      <div className="px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--text-muted)]">
        {label}
      </div>
      {personas.map((persona) => (
        <PersonaOption
          key={persona.id}
          label={persona.name}
          selected={persona.id === personaId}
          onSelect={() => onValueChange(persona.id)}
        />
      ))}
    </div>
  );
}

function PersonaOption({
  label,
  selected,
  onSelect,
}: {
  label: string;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={selected}
      onClick={onSelect}
      className="flex w-full items-center gap-2 rounded px-2.5 py-2 text-left text-sm text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
    >
      <span
        aria-hidden="true"
        className={cn(
          "h-2.5 w-2.5 rounded-full border border-[var(--text-muted)]",
          selected && "border-[var(--accent-primary)] bg-[var(--accent-primary)]",
        )}
      />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {selected && <Check className="h-3.5 w-3.5 text-[var(--accent-primary)]" aria-hidden="true" />}
    </button>
  );
}
