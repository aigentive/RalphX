import { useMemo, useState } from "react";
import { Check, Eye } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { usePersonas } from "@/hooks/usePersonas";
import { cn } from "@/lib/utils";
import type { Persona } from "@/types/persona";

export interface PersonaMenuListProps {
  /** Scoping is authoritative: only global + this project's personas are offered. */
  projectId: string;
  projectName: string;
  selectedPersonaId: string | null;
  onSelect: (personaId: string | null) => void;
  disabled?: boolean;
  showNoPersona?: boolean;
}

/**
 * Single shared persona menu for the start-composer picker and the
 * conversation chip: scoped query, Global/Project grouping, description
 * line, and per-row read-only inspection.
 */
export function PersonaMenuList({
  projectId,
  projectName,
  selectedPersonaId,
  onSelect,
  disabled = false,
  showNoPersona = true,
}: PersonaMenuListProps) {
  const scope = useMemo(
    () => ({ type: "globalAndProject", projectId }) as const,
    [projectId],
  );
  const { data: personas = [], isLoading, isError, refetch } = usePersonas(scope);
  const [inspectedPersona, setInspectedPersona] = useState<Persona | null>(null);
  const [inspectOpen, setInspectOpen] = useState(false);

  const activePersonas = useMemo(
    () =>
      personas.filter(
        (persona) =>
          persona.status === "active" &&
          (persona.projectId === null || persona.projectId === projectId),
      ),
    [personas, projectId],
  );
  const globalPersonas = activePersonas.filter(
    (persona) => persona.projectId === null,
  );
  const projectPersonas = activePersonas.filter(
    (persona) => persona.projectId === projectId,
  );

  const inspect = (persona: Persona) => {
    // Shell-first: open the dialog in the same click commit; content is
    // already client-side so no fetch gates the transition.
    setInspectedPersona(persona);
    setInspectOpen(true);
  };

  return (
    <>
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
            data-testid="persona-menu-loading"
            className="mx-2 my-2 h-7 animate-pulse rounded bg-[var(--bg-elevated)]"
          />
        ) : (
          <>
            {!isError && showNoPersona && (
              <PersonaMenuRow
                label="No persona"
                selected={selectedPersonaId === null}
                disabled={disabled}
                onSelect={() => onSelect(null)}
              />
            )}
            <PersonaMenuGroup
              label="Global"
              personas={globalPersonas}
              selectedPersonaId={selectedPersonaId}
              disabled={disabled}
              onSelect={onSelect}
              onInspect={inspect}
            />
            <PersonaMenuGroup
              label={projectName}
              personas={projectPersonas}
              selectedPersonaId={selectedPersonaId}
              disabled={disabled}
              onSelect={onSelect}
              onInspect={inspect}
            />
          </>
        )}
      </div>
      <Dialog open={inspectOpen} onOpenChange={setInspectOpen}>
        <DialogContent
          className="max-w-2xl"
          aria-describedby="persona-inspect-description"
        >
          <DialogHeader>
            <div>
              <DialogTitle>
                {inspectedPersona
                  ? `${inspectedPersona.name} · v${inspectedPersona.version}`
                  : "Persona"}
              </DialogTitle>
              <DialogDescription id="persona-inspect-description">
                {inspectedPersona
                  ? `${inspectedPersona.slug} — read-only preview`
                  : "Read-only preview"}
              </DialogDescription>
            </div>
          </DialogHeader>
          {inspectedPersona && (
            <div className="max-h-[60vh] overflow-y-auto px-6 pb-5">
              {inspectedPersona.description && (
                <p className="mb-3 text-sm text-[var(--text-secondary)]">
                  {inspectedPersona.description}
                </p>
              )}
              <pre
                data-testid="persona-inspect-content"
                className="whitespace-pre-wrap break-words rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 font-mono text-xs leading-relaxed text-[var(--text-primary)]"
              >
                {inspectedPersona.content}
              </pre>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}

function PersonaMenuGroup({
  label,
  personas,
  selectedPersonaId,
  disabled,
  onSelect,
  onInspect,
}: {
  label: string;
  personas: Persona[];
  selectedPersonaId: string | null;
  disabled: boolean;
  onSelect: (personaId: string | null) => void;
  onInspect: (persona: Persona) => void;
}) {
  if (personas.length === 0) return null;
  return (
    <div role="group" aria-label={label} className="pt-1">
      <div className="px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wide text-[var(--text-muted)]">
        {label}
      </div>
      {personas.map((persona) => (
        <PersonaMenuRow
          key={persona.id}
          label={persona.name}
          description={persona.description}
          selected={persona.id === selectedPersonaId}
          disabled={disabled}
          onSelect={() => onSelect(persona.id)}
          onInspect={() => onInspect(persona)}
          inspectLabel={`Inspect ${persona.name}`}
        />
      ))}
    </div>
  );
}

function PersonaMenuRow({
  label,
  description,
  selected,
  disabled,
  onSelect,
  onInspect,
  inspectLabel,
}: {
  label: string;
  description?: string;
  selected: boolean;
  disabled: boolean;
  onSelect: () => void;
  onInspect?: () => void;
  inspectLabel?: string;
}) {
  return (
    <div className="group/persona-row flex items-center gap-1">
      <button
        type="button"
        role="menuitemradio"
        aria-checked={selected}
        disabled={disabled}
        onClick={onSelect}
        className="flex min-w-0 flex-1 items-center gap-2 rounded px-2.5 py-2 text-left text-sm text-[var(--text-primary)] hover:bg-[var(--bg-hover)] disabled:cursor-not-allowed disabled:opacity-60"
      >
        <span
          aria-hidden="true"
          className={cn(
            "h-2.5 w-2.5 shrink-0 rounded-full border border-[var(--text-muted)]",
            selected && "border-[var(--accent-primary)] bg-[var(--accent-primary)]",
          )}
        />
        <span className="min-w-0 flex-1">
          <span className="block truncate">{label}</span>
          {description && (
            <span className="block truncate text-xs text-[var(--text-muted)]">
              {description}
            </span>
          )}
        </span>
        {selected && (
          <Check
            className="h-3.5 w-3.5 shrink-0 text-[var(--accent-primary)]"
            aria-hidden="true"
          />
        )}
      </button>
      {onInspect && (
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={inspectLabel ?? `Inspect ${label}`}
              onClick={onInspect}
              className="shrink-0 text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            >
              <Eye aria-hidden="true" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{inspectLabel ?? `Inspect ${label}`}</TooltipContent>
        </Tooltip>
      )}
    </div>
  );
}
