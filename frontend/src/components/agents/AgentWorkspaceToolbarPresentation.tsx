import { GitBranch } from "lucide-react";

import {
  StatusPill,
  type StatusPillTone,
} from "@/components/ui/status-pill";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

const ROUTINE_SYNC_LABELS = new Set(["Pushed", "Refreshed"]);
const WARNING_SYNC_LABELS = new Set([
  "Repair pending",
  "Conflicting",
  "Base unavailable",
  "Behind base",
]);
const ERROR_SYNC_LABELS = new Set(["Failed", "Description failed"]);
const ACTIVE_SYNC_LABELS = new Set([
  "Pushing",
  "Committing",
  "Checking",
  "Refreshing",
]);

function syncTone(label: string): StatusPillTone {
  if (WARNING_SYNC_LABELS.has(label)) return "warning";
  if (ERROR_SYNC_LABELS.has(label)) return "error";
  if (ACTIVE_SYNC_LABELS.has(label)) return "accent";
  return "neutral";
}

export function AgentWorkspaceBranchIdentity({
  branchName,
  baseLabel,
}: {
  branchName: string;
  baseLabel: string;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className="flex min-w-0 flex-[1_1_16rem] items-center gap-1.5 text-[var(--text-secondary)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          aria-label={`${branchName} merges into ${baseLabel}`}
          tabIndex={0}
        >
          <GitBranch
            className="h-3.5 w-3.5 shrink-0 text-[var(--text-muted)]"
            aria-hidden="true"
          />
          <span className="min-w-0 truncate font-medium text-[var(--text-primary)]">
            {branchName}
          </span>
        </span>
      </TooltipTrigger>
      <TooltipContent side="bottom">Merges into {baseLabel}</TooltipContent>
    </Tooltip>
  );
}

export function AgentWorkspaceModeStatus({ label }: { label: string }) {
  const accessibleLabel = `Workspace mode: ${label}`;
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className="ml-auto shrink-0 rounded px-1 py-0.5 text-[0.6875rem] font-medium text-[var(--text-secondary)] focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          data-testid="agents-workspace-mode-status"
          aria-label={accessibleLabel}
          tabIndex={0}
        >
          {label}
        </span>
      </TooltipTrigger>
      <TooltipContent side="bottom">{accessibleLabel}</TooltipContent>
    </Tooltip>
  );
}

export function AgentWorkspaceSyncStatus({
  label,
}: {
  label: string | null;
}) {
  if (!label) return null;

  const accessibleLabel = `Workspace sync: ${label}`;
  if (ROUTINE_SYNC_LABELS.has(label)) {
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            className="inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full focus-visible:outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
            data-testid="agents-workspace-sync-status"
            aria-label={accessibleLabel}
            tabIndex={0}
          >
            <span
              className="h-1.5 w-1.5 rounded-full"
              style={{ backgroundColor: "var(--text-muted, #8e8e93)" }}
              aria-hidden="true"
            />
          </span>
        </TooltipTrigger>
        <TooltipContent side="bottom">{label}</TooltipContent>
      </Tooltip>
    );
  }

  return (
    <StatusPill
      label={label}
      tone={syncTone(label)}
      testId="agents-workspace-sync-status"
      className="shrink-0 font-medium"
    />
  );
}
