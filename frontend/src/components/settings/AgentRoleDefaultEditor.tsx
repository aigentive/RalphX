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
import { ManualRoleRuntimeSelector } from "@/components/agents/composer/runtime/ManualRoleRuntimeSelector";

const DEFAULT_VALUE = "__default__";

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

function nullableValue(value: string): string | null {
  return value === DEFAULT_VALUE ? null : value;
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

  return (
    <div className="space-y-4">
      <section aria-labelledby={`${entry.role}-runtime-title`}>
        <h5
          id={`${entry.role}-runtime-title`}
          className="mb-3 text-[11px] font-semibold uppercase tracking-[0.08em] text-[var(--text-muted)]"
        >
          Runtime
        </h5>
        <ManualRoleRuntimeSelector
          entry={entry}
          value={value}
          providerOptions={providers.map((provider) => ({
            id: provider as "claude" | "codex",
            label: provider === "codex" ? "Codex" : provider === "claude" ? "Claude" : provider,
          }))}
          modelsForProvider={modelsForProvider}
          personas={personas}
          disabled={blocked}
          onManagePersonas={onManagePersonas}
          onChange={(selection) => onUpdate({ ...value, ...selection })}
        />
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
          Permissions
        </button>
        {showPersonaAccess && (
          <div id={`${entry.role}-persona-access`} className="agents-role-editor-grid mt-3">
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
