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

export interface PersonaPickerControlProps {
  personaId: string | null;
  onValueChange: (personaId: string | null) => void;
  onOpenPersonas: () => void;
}

export function PersonaPickerControl({
  personaId,
  onValueChange,
  onOpenPersonas,
}: PersonaPickerControlProps) {
  const queryClient = useQueryClient();
  const { data: personas = [], isLoading } = usePersonas();
  const activePersonas = useMemo(
    () => personas.filter((persona) => persona.status === "active"),
    [personas],
  );
  const selectedPersona = activePersonas.find(
    (persona) => persona.id === personaId,
  );
  const tooltipLabel = selectedPersona
    ? `Persona: ${selectedPersona.name}`
    : "Choose persona";
  const warmPersonas = useCallback(() => {
    void queryClient.prefetchQuery({
      queryKey: personaKeys.list(),
      queryFn: fetchPersonas,
    });
  }, [queryClient]);

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
          <PersonaOption
            label="No persona"
            selected={personaId === null}
            onSelect={() => onValueChange(null)}
          />
          {isLoading && activePersonas.length === 0 ? (
            <div
              data-testid="persona-picker-loading"
              className="mx-2 my-2 h-7 animate-pulse rounded bg-[var(--bg-elevated)]"
            />
          ) : (
            activePersonas.map((persona) => (
              <PersonaOption
                key={persona.id}
                label={persona.name}
                selected={persona.id === personaId}
                onSelect={() => onValueChange(persona.id)}
              />
            ))
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
