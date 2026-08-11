import { useCallback, useMemo } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { ChevronDown, CircleDot } from "lucide-react";

import { PersonaMenuList } from "@/components/personas/PersonaMenuList";
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
  const { data: personas = [] } = usePersonas(scope);
  const selectedPersona = personas.find(
    (persona) =>
      persona.id === personaId &&
      persona.status === "active" &&
      (persona.projectId === null || persona.projectId === currentProjectId),
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
            <button
              type="button"
              aria-label="Choose persona"
              data-testid="persona-picker-trigger"
              onPointerEnter={warmPersonas}
              onFocus={warmPersonas}
              className={cn(
                "inline-flex h-7 max-w-44 shrink-0 items-center gap-1 rounded-full border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-2 text-xs font-medium text-[var(--text-secondary)] transition-colors hover:border-[var(--accent-primary)] hover:text-[var(--text-primary)]",
                selectedPersona && "text-[var(--text-primary)]",
              )}
            >
              <CircleDot
                className={cn(
                  "h-3.5 w-3.5 shrink-0",
                  selectedPersona && "text-[var(--accent-primary)]",
                )}
                aria-hidden="true"
              />
              <span
                data-testid="persona-picker-label"
                className={cn(
                  "min-w-0 truncate",
                  !selectedPersona && "text-[var(--text-muted)]",
                )}
              >
                {selectedPersona ? selectedPersona.name : "Persona"}
              </span>
              <ChevronDown className="h-3 w-3 shrink-0" aria-hidden="true" />
            </button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent>{tooltipLabel}</TooltipContent>
      </Tooltip>
      <PopoverContent
        data-testid="persona-picker-popover"
        align="end"
        className="w-72 p-1.5"
      >
        <PersonaMenuList
          projectId={currentProjectId}
          projectName={currentProjectName}
          selectedPersonaId={personaId}
          onSelect={onValueChange}
        />
        <div className="mt-1 border-t border-[var(--border-subtle)] pt-1">
          <button
            type="button"
            role="menuitem"
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
