import { Folder, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Persona } from "@/types/persona";

function relativeUpdatedAt(updatedAt: string): string {
  const elapsedMs = Date.now() - new Date(updatedAt).getTime();
  const hours = Math.round(elapsedMs / (60 * 60 * 1000));
  if (hours < 1) return "Updated just now";
  if (hours < 24) return `Updated ${hours}h ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? "Updated yesterday" : `Updated ${days}d ago`;
}

function StatusChip({ status }: { status: Persona["status"] }) {
  const active = status === "active";
  return (
    <span
      className={
        active
          ? "rounded-sm border border-[var(--accent-border)] bg-[var(--accent-muted)] px-1.5 py-0.5 text-[10px] font-semibold tracking-wide text-[var(--accent-primary)]"
          : "rounded-sm border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-1.5 py-0.5 text-[10px] font-semibold tracking-wide text-[var(--text-muted)]"
      }
    >
      {status.toUpperCase()}
    </span>
  );
}

export function ScopeBadge({
  projectId,
  projectNames,
  testId,
}: {
  projectId: string | null;
  projectNames: Record<string, string>;
  testId?: string;
}) {
  const label = projectId === null ? "Global" : (projectNames[projectId] ?? projectId);
  return (
    <span
      {...(testId !== undefined && { "data-testid": testId })}
      className="inline-flex max-w-48 items-center gap-1 rounded-sm border border-[var(--accent-border)] bg-[var(--accent-muted)] px-1.5 py-0.5 text-[10px] font-semibold text-[var(--accent-primary)]"
    >
      {projectId !== null && <Folder className="h-3 w-3 shrink-0" aria-hidden="true" />}
      <span className="truncate">{label}</span>
    </span>
  );
}
export function PersonaRow({
  persona,
  projectNames,
  onEdit,
  onActivate,
  onRemove,
}: {
  persona: Persona;
  projectNames: Record<string, string>;
  onEdit: (persona: Persona) => void;
  onActivate: (persona: Persona) => void;
  onRemove: (persona: Persona) => void;
}) {
  const active = persona.status === "active";
  const actionLabel = active ? "Archive" : "Delete";
  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3">
      <span
        aria-label={active ? "Active persona" : "Draft persona"}
        className={
          active
            ? "h-2.5 w-2.5 rounded-full bg-[var(--accent-primary)]"
            : "h-2.5 w-2.5 rounded-full border border-[var(--text-muted)]"
        }
      />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
          <span className="font-medium text-[var(--text-primary)]">{persona.name}</span>
          <code className="text-xs text-[var(--text-muted)]">{persona.slug}</code>
          <ScopeBadge
            projectId={persona.projectId}
            projectNames={projectNames}
            testId={`persona-scope-${persona.id}`}
          />
          <StatusChip status={persona.status} />
          <span className="text-xs text-[var(--text-muted)]">v{persona.version}</span>
        </div>
        <p className="mt-1 text-xs text-[var(--text-muted)]">{relativeUpdatedAt(persona.updatedAt)}</p>
      </div>
      <div className="flex items-center gap-1">
        {!active && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-label={`Activate ${persona.name}`}
            onClick={() => onActivate(persona)}
            className="text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
          >
            Activate
          </Button>
        )}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          aria-label={`Edit ${persona.name}`}
          onClick={() => onEdit(persona)}
          className="text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
        >
          Edit
        </Button>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={`${actionLabel} ${persona.name}`}
              onClick={() => onRemove(persona)}
              className="text-[var(--text-muted)] hover:bg-[var(--status-error-muted)] hover:text-[var(--status-error)]"
            >
              <Trash2 aria-hidden="true" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{actionLabel} {persona.name}</TooltipContent>
        </Tooltip>
      </div>
    </li>
  );
}
