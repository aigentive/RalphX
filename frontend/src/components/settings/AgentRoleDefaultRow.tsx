import type { AgentModelCatalogEntry } from "@/lib/agent-models";
import type { Persona } from "@/types/persona";
import type {
  ManualRoleCatalogEntry,
  ManualRoleDefault,
} from "@/api/manual-role-defaults.types";

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

const SOURCE_LABELS: Record<string, string> = {
  project_ui: "Project UI",
  project_yaml: "Project YAML",
  global_ui: "Global UI",
  global_yaml: "Global YAML",
  legacy_lane: "Legacy lane",
  legacy_workspace_review: "Legacy Workspace Review",
  provider_default: "Provider default",
};

interface AgentRoleDefaultRowProps {
  entry: ManualRoleCatalogEntry;
  disabled: boolean;
  providers: readonly string[];
  modelsForProvider: (provider: string) => readonly AgentModelCatalogEntry[];
  personas: readonly Persona[];
  onUpdate: (value: ManualRoleDefault) => void;
  onFollow: () => void;
  onManagePersonas: () => void;
}

function nullableValue(value: string): string | null {
  return value === "__default__" ? null : value;
}

function labelFor(value: string, labels: Record<string, string>): string {
  return labels[value] ?? value;
}

export function AgentRoleDefaultRow({
  entry,
  disabled,
  providers,
  modelsForProvider,
  personas,
  onUpdate,
  onFollow,
  onManagePersonas,
}: AgentRoleDefaultRowProps) {
  const value = entry.configured ?? entry.effective;
  const blocked = disabled || value === null;
  const models = value ? modelsForProvider(value.provider) : [];
  const providerOptions =
    value && !providers.includes(value.provider)
      ? [value.provider, ...providers]
      : providers;
  const selectedModel = models.find((model) => model.id === value?.model);
  const efforts = selectedModel?.supportedEfforts ?? [];
  const commit = (patch: Partial<ManualRoleDefault>) => {
    if (value) onUpdate({ ...value, ...patch });
  };

  return (
    <div
      data-testid="manual-role-row"
      className="rounded-lg px-4 py-4"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: 1,
      }}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h4 className="text-sm font-semibold text-[var(--text-primary)]">
            {entry.displayName}
          </h4>
          <p className="mt-0.5 text-xs text-[var(--text-muted)]">
            Manual default · {entry.source ? SOURCE_LABELS[entry.source] ?? entry.source : "Invalid"}
          </p>
        </div>
        {entry.configured && (
          <button
            type="button"
            disabled={disabled}
            onClick={onFollow}
            aria-label={`Follow ${entry.displayName} default`}
            className="rounded-md px-2.5 py-1.5 text-xs font-medium text-[var(--accent-primary)] hover:bg-[var(--bg-hover)] disabled:opacity-50"
          >
            Follow
          </button>
        )}
      </div>

      {entry.diagnostics.length > 0 && (
        <div className="mt-3 space-y-1 text-xs text-[var(--status-error)]" role="alert">
          {entry.diagnostics.map((diagnostic) => (
            <p key={diagnostic}>{diagnostic}</p>
          ))}
        </div>
      )}

      <div className="mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <label className="space-y-1 text-xs text-[var(--text-secondary)]">
          <span>Provider</span>
          <select
            aria-label={`${entry.displayName} provider`}
            className="settings-input h-9 w-full"
            value={value?.provider ?? ""}
            disabled={blocked}
            onChange={(event) => {
              const provider = event.target.value;
              const firstModel = modelsForProvider(provider)[0];
              commit({
                provider,
                model: firstModel?.id ?? null,
                effort: firstModel?.defaultEffort ?? null,
              });
            }}
          >
            {providerOptions.map((provider) => (
              <option key={provider} value={provider}>
                {provider === "codex" ? "Codex" : provider === "claude" ? "Claude" : provider}
              </option>
            ))}
          </select>
        </label>

        <label className="space-y-1 text-xs text-[var(--text-secondary)]">
          <span>Model</span>
          <select
            aria-label={`${entry.displayName} model`}
            className="settings-input h-9 w-full"
            value={value?.model ?? "__default__"}
            disabled={blocked}
            onChange={(event) => {
              const model = nullableValue(event.target.value);
              const selected = models.find((candidate) => candidate.id === model);
              commit({ model, effort: selected?.defaultEffort ?? value?.effort ?? null });
            }}
          >
            <option value="__default__">Provider default</option>
            {models.map((model) => (
              <option key={model.id} value={model.id}>{model.menuLabel}</option>
            ))}
            {value?.model && !selectedModel && (
              <option value={value.model}>{value.model}</option>
            )}
          </select>
        </label>

        <label className="space-y-1 text-xs text-[var(--text-secondary)]">
          <span>Effort</span>
          <select
            aria-label={`${entry.displayName} effort`}
            className="settings-input h-9 w-full"
            value={value?.effort ?? "__default__"}
            disabled={blocked}
            onChange={(event) => commit({ effort: nullableValue(event.target.value) })}
          >
            <option value="__default__">Provider default</option>
            {efforts.map((effort) => <option key={effort} value={effort}>{effort}</option>)}
            {value?.effort && !efforts.some((effort) => effort === value.effort) && (
              <option value={value.effort}>{value.effort}</option>
            )}
          </select>
        </label>

        <label className="space-y-1 text-xs text-[var(--text-secondary)]">
          <span>Capability</span>
          <select
            aria-label={`${entry.displayName} capability`}
            className="settings-input h-9 w-full"
            value={value?.coordinationMode ?? "solo"}
            disabled={blocked}
            onChange={(event) => commit({ coordinationMode: event.target.value })}
          >
            {entry.controls.capabilities.map((option) => (
              <option key={option.value} value={option.value} disabled={!option.enabled}>
                {labelFor(option.value, CAPABILITY_LABELS)}
                {!option.enabled && option.disabledReason ? ` — ${option.disabledReason}` : ""}
              </option>
            ))}
          </select>
        </label>

        <label className="space-y-1 text-xs text-[var(--text-secondary)]">
          <span>Speed</span>
          <select
            aria-label={`${entry.displayName} speed`}
            className="settings-input h-9 w-full"
            value={value?.serviceTier ?? "provider_default"}
            disabled={blocked}
            onChange={(event) =>
              commit({ serviceTier: event.target.value as ManualRoleDefault["serviceTier"] })
            }
          >
            {entry.controls.speeds.map((option) => (
              <option key={option.value} value={option.value} disabled={!option.enabled}>
                {labelFor(option.value, SPEED_LABELS)}
                {!option.enabled && option.disabledReason ? ` — ${option.disabledReason}` : ""}
              </option>
            ))}
          </select>
        </label>

        <label className="space-y-1 text-xs text-[var(--text-secondary)]">
          <span>Persona</span>
          <select
            aria-label={`${entry.displayName} persona`}
            className="settings-input h-9 w-full"
            value={value?.personaId ?? "__default__"}
            disabled={blocked || !entry.controls.persona.enabled}
            onChange={(event) => commit({ personaId: nullableValue(event.target.value) })}
          >
            <option value="__default__">No role persona</option>
            {personas.map((persona) => (
              <option key={persona.id} value={persona.id}>{persona.name}</option>
            ))}
          </select>
        </label>

        <label className="space-y-1 text-xs text-[var(--text-secondary)]">
          <span>Approval</span>
          <select
            aria-label={`${entry.displayName} approval policy`}
            className="settings-input h-9 w-full"
            value={value?.approvalPolicy ?? "__default__"}
            disabled={blocked}
            onChange={(event) => commit({ approvalPolicy: nullableValue(event.target.value) })}
          >
            <option value="__default__">Provider default</option>
            <option value="untrusted">Untrusted</option>
            <option value="on-request">On request</option>
            <option value="never">Never</option>
          </select>
        </label>

        <label className="space-y-1 text-xs text-[var(--text-secondary)]">
          <span>Sandbox</span>
          <select
            aria-label={`${entry.displayName} sandbox mode`}
            className="settings-input h-9 w-full"
            value={value?.sandboxMode ?? "__default__"}
            disabled={blocked}
            onChange={(event) => commit({ sandboxMode: nullableValue(event.target.value) })}
          >
            <option value="__default__">Provider default</option>
            <option value="read-only">Read only</option>
            <option value="workspace-write">Workspace write</option>
            <option value="danger-full-access">Danger full access</option>
          </select>
        </label>
      </div>

      <div className="mt-3 space-y-1 text-[11px] text-[var(--text-muted)]">
        {entry.controls.capabilities.filter((option) => !option.enabled).map((option) => (
          <p key={`capability-${option.value}`}>{option.disabledReason}</p>
        ))}
        {entry.controls.speeds.filter((option) => !option.enabled).map((option) => (
          <p key={`speed-${option.value}`}>{option.disabledReason}</p>
        ))}
        {!entry.controls.persona.enabled && entry.controls.persona.disabledReason && (
          <p>
            {entry.controls.persona.disabledReason}{" "}
            <button type="button" className="text-[var(--accent-primary)] underline" onClick={onManagePersonas}>
              Manage personas
            </button>
          </p>
        )}
      </div>
    </div>
  );
}
