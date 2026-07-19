import { useId, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";

import type {
  ManualRoleCatalogEntry,
  ManualRoleDefault,
} from "@/api/manual-role-defaults.types";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { AgentModelCatalogEntry } from "@/lib/agent-models";
import type { Persona } from "@/types/persona";

const DEFAULT_VALUE = "__default__";

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

interface EditorOption {
  value: string;
  label: string;
  enabled?: boolean;
  description?: string | null;
}

interface EditorSelectProps {
  label: string;
  ariaLabel: string;
  value: string;
  options: readonly EditorOption[];
  disabled: boolean;
  helpContent?: ReactNode;
  onValueChange: (value: string) => void;
}

function labelFor(value: string, labels: Record<string, string>): string {
  return labels[value] ?? value;
}

function nullableValue(value: string): string | null {
  return value === DEFAULT_VALUE ? null : value;
}

function providerLabel(provider: string): string {
  if (provider === "codex") return "Codex";
  if (provider === "claude") return "Claude";
  return provider;
}

function EditorSelect({
  label,
  ariaLabel,
  value,
  options,
  disabled,
  helpContent,
  onValueChange,
}: EditorSelectProps) {
  const helpId = useId();
  const selected = options.find((option) => option.value === value);
  const disabledReasons = options
    .filter((option) => option.enabled === false && option.description)
    .map((option) => option.description);
  const hasHelp = disabledReasons.length > 0 || Boolean(helpContent);

  return (
    <div className="space-y-1.5">
      <p className="text-xs font-medium text-[var(--text-secondary)]">{label}</p>
      <Select value={value} onValueChange={onValueChange} disabled={disabled}>
        <SelectTrigger
          aria-label={ariaLabel}
          aria-describedby={hasHelp ? helpId : undefined}
          className="h-9 w-full border-[var(--border-default)] bg-[var(--bg-elevated)] text-left font-[var(--font-body)]"
        >
          <SelectValue>
            <span className="truncate">{selected?.label ?? value}</span>
          </SelectValue>
        </SelectTrigger>
        <SelectContent className="border-[var(--border-default)] bg-[var(--bg-elevated)]">
          {options.map((option) => (
            <SelectItem
              key={option.value}
              value={option.value}
              textValue={option.label}
              disabled={option.enabled === false}
            >
              <div className="flex flex-col">
                <span>{option.label}</span>
                {option.description && (
                  <span className="text-xs text-[var(--text-muted)]">
                    {option.description}
                  </span>
                )}
              </div>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {hasHelp && (
        <div id={helpId} className="space-y-1 text-[11px] leading-4 text-[var(--text-muted)]">
          {disabledReasons.map((reason) => <p key={reason}>{reason}</p>)}
          {helpContent}
        </div>
      )}
    </div>
  );
}

export interface AgentRoleDefaultEditorProps {
  entry: ManualRoleCatalogEntry;
  disabled: boolean;
  providers: readonly string[];
  modelsForProvider: (provider: string) => readonly AgentModelCatalogEntry[];
  personas: readonly Persona[];
  forcePersonaAccessOpen: boolean;
  onUpdate: (value: ManualRoleDefault) => void;
  onManagePersonas: () => void;
}

export function AgentRoleDefaultEditor({
  entry,
  disabled,
  providers,
  modelsForProvider,
  personas,
  forcePersonaAccessOpen,
  onUpdate,
  onManagePersonas,
}: AgentRoleDefaultEditorProps) {
  const value = entry.configured ?? entry.effective;
  const blocked = disabled || value === null;
  const models = value ? modelsForProvider(value.provider) : [];
  const selectedModel = models.find((model) => model.id === value?.model);
  const efforts = selectedModel?.supportedEfforts ?? [];
  const [personaAccessExpanded, setPersonaAccessExpanded] = useState(
    () => Boolean(
      entry.configured?.personaId ||
      entry.configured?.approvalPolicy ||
      entry.configured?.sandboxMode,
    ),
  );
  const showPersonaAccess = forcePersonaAccessOpen || personaAccessExpanded;

  if (!value) {
    return (
      <p className="text-xs text-[var(--status-error)]">
        This role has no effective default to edit.
      </p>
    );
  }

  const commit = (patch: Partial<ManualRoleDefault>) => {
    onUpdate({ ...value, ...patch });
  };
  const providerOptions = (
    providers.includes(value.provider)
      ? providers
      : [value.provider, ...providers]
  ).map((provider) => ({
    value: provider,
    label: providerLabel(provider),
  }));
  const modelOptions: EditorOption[] = [
    { value: DEFAULT_VALUE, label: "Provider default" },
    ...models.map((model) => ({ value: model.id, label: model.menuLabel })),
  ];
  if (value.model && !selectedModel) {
    modelOptions.push({ value: value.model, label: value.model });
  }
  const effortOptions: EditorOption[] = [
    { value: DEFAULT_VALUE, label: "Provider default" },
    ...efforts.map((effort) => ({ value: effort, label: effort })),
  ];
  if (value.effort && !efforts.some((effort) => effort === value.effort)) {
    effortOptions.push({ value: value.effort, label: value.effort });
  }

  return (
    <div className="space-y-4">
      <section aria-labelledby={`${entry.role}-runtime-title`}>
        <h5
          id={`${entry.role}-runtime-title`}
          className="mb-3 text-[11px] font-semibold uppercase tracking-[0.08em] text-[var(--text-muted)]"
        >
          Runtime
        </h5>
        <div className="agents-role-editor-grid">
          <EditorSelect
            label="Provider"
            ariaLabel={`${entry.displayName} provider`}
            value={value.provider}
            options={providerOptions}
            disabled={blocked}
            onValueChange={(provider) => {
              const firstModel = modelsForProvider(provider)[0];
              commit({
                provider,
                model: firstModel?.id ?? null,
                effort: firstModel?.defaultEffort ?? null,
              });
            }}
          />
          <EditorSelect
            label="Model"
            ariaLabel={`${entry.displayName} model`}
            value={value.model ?? DEFAULT_VALUE}
            options={modelOptions}
            disabled={blocked}
            onValueChange={(next) => {
              const model = nullableValue(next);
              const selected = models.find((candidate) => candidate.id === model);
              commit({ model, effort: selected?.defaultEffort ?? value.effort });
            }}
          />
          <EditorSelect
            label="Effort"
            ariaLabel={`${entry.displayName} effort`}
            value={value.effort ?? DEFAULT_VALUE}
            options={effortOptions}
            disabled={blocked}
            onValueChange={(effort) => commit({ effort: nullableValue(effort) })}
          />
          <EditorSelect
            label="Speed"
            ariaLabel={`${entry.displayName} speed`}
            value={value.serviceTier}
            options={entry.controls.speeds.map((option) => ({
              value: option.value,
              label: labelFor(option.value, SPEED_LABELS),
              enabled: option.enabled,
              description: option.disabledReason,
            }))}
            disabled={blocked}
            onValueChange={(serviceTier) =>
              commit({ serviceTier: serviceTier as ManualRoleDefault["serviceTier"] })
            }
          />
          <EditorSelect
            label="Capability"
            ariaLabel={`${entry.displayName} capability`}
            value={value.coordinationMode ?? "solo"}
            options={entry.controls.capabilities.map((option) => ({
              value: option.value,
              label: labelFor(option.value, CAPABILITY_LABELS),
              enabled: option.enabled,
              description: option.disabledReason,
            }))}
            disabled={blocked}
            onValueChange={(coordinationMode) => commit({ coordinationMode })}
          />
        </div>
      </section>

      <section className="border-t border-[var(--border-subtle)] pt-3">
        <button
          type="button"
          aria-expanded={showPersonaAccess}
          aria-disabled={forcePersonaAccessOpen}
          aria-controls={`${entry.role}-persona-access`}
          onClick={() => {
            if (!forcePersonaAccessOpen) {
              setPersonaAccessExpanded((current) => !current);
            }
          }}
          className="flex w-full items-center gap-2 text-left text-xs font-medium text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
        >
          {showPersonaAccess
            ? <ChevronDown aria-hidden="true" className="h-3.5 w-3.5" />
            : <ChevronRight aria-hidden="true" className="h-3.5 w-3.5" />}
          Persona & access
        </button>
        {showPersonaAccess && (
          <div id={`${entry.role}-persona-access`} className="agents-role-editor-grid mt-3">
            <EditorSelect
              label="Persona"
              ariaLabel={`${entry.displayName} persona`}
              value={value.personaId ?? DEFAULT_VALUE}
              options={[
                { value: DEFAULT_VALUE, label: "No role persona" },
                ...personas.map((persona) => ({ value: persona.id, label: persona.name })),
              ]}
              disabled={blocked || !entry.controls.persona.enabled}
              helpContent={
                !entry.controls.persona.enabled &&
                entry.controls.persona.disabledReason ? (
                  <p>
                    {entry.controls.persona.disabledReason}{" "}
                    <button
                      type="button"
                      className="text-[var(--accent-primary)] underline"
                      onClick={onManagePersonas}
                    >
                      Manage personas
                    </button>
                  </p>
                ) : undefined
              }
              onValueChange={(personaId) => commit({ personaId: nullableValue(personaId) })}
            />
            <EditorSelect
              label="Approval"
              ariaLabel={`${entry.displayName} approval policy`}
              value={value.approvalPolicy ?? DEFAULT_VALUE}
              options={[
                { value: DEFAULT_VALUE, label: "Provider default" },
                { value: "untrusted", label: "Untrusted" },
                { value: "on-request", label: "On request" },
                { value: "never", label: "Never" },
              ]}
              disabled={blocked}
              onValueChange={(approvalPolicy) =>
                commit({ approvalPolicy: nullableValue(approvalPolicy) })
              }
            />
            <EditorSelect
              label="Sandbox"
              ariaLabel={`${entry.displayName} sandbox mode`}
              value={value.sandboxMode ?? DEFAULT_VALUE}
              options={[
                { value: DEFAULT_VALUE, label: "Provider default" },
                { value: "read-only", label: "Read only" },
                { value: "workspace-write", label: "Workspace write" },
                { value: "danger-full-access", label: "Danger full access" },
              ]}
              disabled={blocked}
              onValueChange={(sandboxMode) =>
                commit({ sandboxMode: nullableValue(sandboxMode) })
              }
            />
          </div>
        )}
      </section>
    </div>
  );
}
