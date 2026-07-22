import { Folder, Sparkles, Trash2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Persona, PersonaUsage } from "@/types/persona";

function relativeTime(timestamp: string): string {
  const elapsedMs = Date.now() - new Date(timestamp).getTime();
  const hours = Math.round(elapsedMs / (60 * 60 * 1000));
  if (hours < 1) return "just now";
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  return days === 1 ? "yesterday" : `${days}d ago`;
}

function relativeUpdatedAt(updatedAt: string): string {
  const relative = relativeTime(updatedAt);
  return relative === "just now"
    ? "Updated just now"
    : relative === "yesterday"
      ? "Updated yesterday"
      : `Updated ${relative}`;
}

function usageCaption(usage: PersonaUsage): string {
  const conversations =
    usage.boundConversationCount === 0
      ? null
      : usage.boundConversationCount === 1
        ? "1 conversation"
        : `${usage.boundConversationCount} conversations`;
  const lastRun = usage.lastRunAt ? `last run ${relativeTime(usage.lastRunAt)}` : null;
  if (!conversations && !lastRun) return "never used";
  return [conversations, lastRun].filter(Boolean).join(" · ");
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

export function PersonaRowSkeleton() {
  return (
    <li aria-hidden="true" className="flex items-center gap-3 px-4 py-3">
      <span className="h-2.5 w-2.5 animate-pulse rounded-full bg-[var(--bg-elevated)]" />
      <div className="min-w-0 flex-1 space-y-2">
        <div className="h-3.5 w-1/3 animate-pulse rounded bg-[var(--bg-elevated)]" />
        <div className="h-3 w-1/2 animate-pulse rounded bg-[var(--bg-elevated)]" />
      </div>
      <div className="h-7 w-24 animate-pulse rounded bg-[var(--bg-elevated)]" />
    </li>
  );
}

export function PersonaRow({
  persona,
  projectNames,
  usage,
  usageError = false,
  onEdit,
  onActivate,
  onRemove,
  onRefine,
  onRestore,
  refineDisabledReason,
}: {
  persona: Persona;
  projectNames: Record<string, string>;
  usage?: PersonaUsage | undefined;
  usageError?: boolean;
  onEdit: (persona: Persona) => void;
  onActivate: (persona: Persona) => void;
  onRemove: (persona: Persona) => void;
  onRefine: (persona: Persona) => void;
  onRestore?: (persona: Persona) => void;
  refineDisabledReason?: string;
}) {
  const active = persona.status === "active";
  const archived = persona.status === "archived";
  const actionLabel = active ? "Archive" : "Delete";
  const builtWithAgent = Boolean(persona.sourceSessionId);
  const refineButton = (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      aria-label={`Refine ${persona.name} with Agent`}
      disabled={refineDisabledReason !== undefined}
      onClick={() => onRefine(persona)}
      className="text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
    >
      Refine with Agent
    </Button>
  );
  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3">
      <span
        aria-label={
          archived
            ? "Archived persona"
            : active
              ? "Active persona"
              : "Draft persona"
        }
        className={
          active
            ? "h-2.5 w-2.5 rounded-full bg-[var(--accent-primary)]"
            : archived
              ? "h-2.5 w-2.5 rounded-full border border-dashed border-[var(--text-muted)]"
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
          {builtWithAgent && (
            <span
              data-testid={`persona-lineage-${persona.id}`}
              className="inline-flex items-center gap-1 rounded-sm border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--text-secondary)]"
            >
              <Sparkles className="h-3 w-3" aria-hidden="true" />
              Built with Agent
            </span>
          )}
        </div>
        {persona.description && (
          <p className="mt-1 truncate text-xs text-[var(--text-secondary)]">
            {persona.description}
          </p>
        )}
        <p className="mt-1 text-xs text-[var(--text-muted)]">
          {relativeUpdatedAt(persona.updatedAt)}
          {" · "}
          {usageError ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <span tabIndex={0} data-testid={`persona-usage-error-${persona.id}`}>
                  —
                </span>
              </TooltipTrigger>
              <TooltipContent>Usage information is unavailable.</TooltipContent>
            </Tooltip>
          ) : usage ? (
            <span data-testid={`persona-usage-${persona.id}`}>
              {usageCaption(usage)}
            </span>
          ) : (
            <span aria-hidden="true">…</span>
          )}
        </p>
      </div>
      <div className="flex items-center gap-1">
        {archived ? (
          onRestore && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              aria-label={`Restore ${persona.name}`}
              onClick={() => onRestore(persona)}
              className="text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            >
              Restore
            </Button>
          )
        ) : (
          <>
            {active &&
              (refineDisabledReason ? (
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span className="inline-flex" tabIndex={0}>
                      {refineButton}
                    </span>
                  </TooltipTrigger>
                  <TooltipContent>{refineDisabledReason}</TooltipContent>
                </Tooltip>
              ) : (
                refineButton
              ))}
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
          </>
        )}
      </div>
    </li>
  );
}
