import { ChevronDown, ChevronRight } from "lucide-react";

import type {
  ManualRoleCatalogEntry,
  ManualRoleDefault,
} from "@/api/manual-role-defaults.types";
import { useConfirmation } from "@/hooks/useConfirmation";
import type { AgentModelCatalogEntry } from "@/lib/agent-models";
import type { Persona } from "@/types/persona";

import { AgentRoleDefaultEditor } from "./AgentRoleDefaultEditor";

const SOURCE_LABELS: Record<string, string> = {
  project_ui: "Project UI",
  project_yaml: "Project YAML",
  global_ui: "Global UI",
  global_yaml: "Global YAML",
  legacy_lane: "Legacy lane",
  legacy_workspace_review: "Legacy Workspace Review",
  provider_default: "Provider default",
};

const CAPABILITY_LABELS: Record<string, string> = {
  solo: "Defaults",
  rx_native_team: "Team",
  rx_native_workflow: "Workflow",
  codex_native_ultra: "Codex Ultra",
};

const SPEED_LABELS: Record<string, string> = {
  provider_default: "Provider default",
  standard: "Standard",
  fast: "Fast",
};

export interface AgentRoleDefaultRowProps {
  entry: ManualRoleCatalogEntry;
  expanded: boolean;
  disabled: boolean;
  providers: readonly string[];
  modelsForProvider: (provider: string) => readonly AgentModelCatalogEntry[];
  personas: readonly Persona[];
  onUpdate: (value: ManualRoleDefault) => void;
  onExpandedChange: (expanded: boolean) => void;
  onUseInheritedDefault: () => Promise<unknown>;
  onManagePersonas: () => void;
}

function providerLabel(provider: string): string {
  if (provider === "codex") return "Codex";
  if (provider === "claude") return "Claude";
  return provider;
}

function SummaryPill({ children }: { children: React.ReactNode }) {
  return (
    <span className="inline-flex min-h-6 items-center rounded-full border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-2 text-[11px] text-[var(--text-secondary)]">
      {children}
    </span>
  );
}

export function AgentRoleDefaultRow({
  entry,
  expanded,
  disabled,
  providers,
  modelsForProvider,
  personas,
  onUpdate,
  onExpandedChange,
  onUseInheritedDefault,
  onManagePersonas,
}: AgentRoleDefaultRowProps) {
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const value = entry.configured ?? entry.effective;
  const diagnosticOpen = entry.diagnostics.length > 0;
  const effectiveExpanded = expanded || diagnosticOpen;
  const model = value
    ? modelsForProvider(value.provider).find((candidate) => candidate.id === value.model)
    : null;
  const sourceLabel = entry.source
    ? SOURCE_LABELS[entry.source] ?? entry.source
    : "Invalid";

  const requestInheritedDefault = () => {
    void confirm({
      title: `Use inherited default for ${entry.displayName}?`,
      description:
        "This removes the UI override at this scope. The next configured source will become effective after the backend refreshes this role.",
      confirmText: "Use inherited default",
      pendingText: "Using inherited default...",
      onConfirm: onUseInheritedDefault,
      recoverFromError: () => ({
        description:
          "The override could not be removed. The configured value is unchanged; fix the error and try again.",
        confirmText: "Try again",
      }),
    });
  };

  return (
    <div
      data-testid="manual-role-row"
      className="agents-role-row"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <button
          type="button"
          aria-label={`Edit ${entry.displayName}`}
          aria-expanded={effectiveExpanded}
          aria-disabled={diagnosticOpen}
          aria-controls={`${entry.role}-editor`}
          onClick={() => {
            if (!diagnosticOpen) onExpandedChange(!effectiveExpanded);
          }}
          className="group flex min-w-0 flex-1 items-start gap-2 text-left"
        >
          {effectiveExpanded
            ? <ChevronDown aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0 text-[var(--text-secondary)]" />
            : <ChevronRight aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0 text-[var(--text-secondary)]" />}
          <span className="min-w-0">
            <span className="block text-sm font-semibold text-[var(--text-primary)] group-hover:text-[var(--accent-primary)]">
              {entry.displayName}
            </span>
            <span className="mt-0.5 block text-xs leading-5 text-[var(--text-muted)]">
              {entry.description}
            </span>
          </span>
        </button>
        <div className="flex flex-wrap items-center justify-end gap-2">
          <span className={entry.configured ? "agents-source-badge agents-source-badge--configured" : "agents-source-badge"}>
            {entry.configured ? "Configured here" : `Inherited · ${sourceLabel}`}
          </span>
          {diagnosticOpen && (
            <span className="agents-attention-badge">Needs attention</span>
          )}
          {entry.configured && (
            <button
              type="button"
              disabled={disabled}
              onClick={requestInheritedDefault}
              aria-label={`Use inherited default for ${entry.displayName}`}
              className="rounded-md px-2.5 py-1.5 text-xs font-medium text-[var(--accent-primary)] hover:bg-[var(--bg-hover)] disabled:pointer-events-none disabled:opacity-50"
            >
              Use inherited default
            </button>
          )}
        </div>
      </div>

      {!effectiveExpanded && value && (
        <div className="ml-6 mt-3 flex flex-wrap gap-2">
          <SummaryPill>{providerLabel(value.provider)}</SummaryPill>
          <SummaryPill>{model?.menuLabel ?? value.model ?? "Provider model"}</SummaryPill>
          <SummaryPill>{value.effort ?? "Provider effort"}</SummaryPill>
          {value.serviceTier !== "provider_default" && (
            <SummaryPill>{SPEED_LABELS[value.serviceTier] ?? value.serviceTier}</SummaryPill>
          )}
          {value.coordinationMode && value.coordinationMode !== "solo" && (
            <SummaryPill>
              {CAPABILITY_LABELS[value.coordinationMode] ?? value.coordinationMode}
            </SummaryPill>
          )}
        </div>
      )}

      {effectiveExpanded && (
        <div id={`${entry.role}-editor`} className="ml-6 mt-4 space-y-4">
          {diagnosticOpen && (
            <div className="agents-diagnostic" role="alert">
              {entry.diagnostics.map((diagnostic) => (
                <p key={diagnostic}>{diagnostic}</p>
              ))}
            </div>
          )}
          <AgentRoleDefaultEditor
            entry={entry}
            disabled={disabled}
            providers={providers}
            modelsForProvider={modelsForProvider}
            personas={personas}
            forcePersonaAccessOpen={diagnosticOpen}
            onUpdate={onUpdate}
            onManagePersonas={onManagePersonas}
          />
        </div>
      )}

      <ConfirmationDialog {...confirmationDialogProps} />
    </div>
  );
}
