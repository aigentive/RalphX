import { useState } from "react";

import type {
  ManualRoleCatalogEntry,
  ManualRoleRuntimeSelection,
} from "@/api/manual-role-defaults.types";
import type { AgentModelRegistry } from "@/lib/agent-models";
import type { Persona } from "@/types/persona";
import type { AgentProviderAvailabilityOption } from "./agentProviderAvailability";

import { ManualRoleRuntimeSelector } from "./composer/runtime/ManualRoleRuntimeSelector";

export function RoleRuntimeConfirmationBody({
  entry,
  initialValue,
  hasSavedOverride,
  modelRegistry,
  personas,
  providerOptions,
  onChange,
  onReset,
  onValidityChange,
}: {
  entry: ManualRoleCatalogEntry;
  initialValue: ManualRoleRuntimeSelection;
  hasSavedOverride: boolean;
  modelRegistry: AgentModelRegistry;
  personas: readonly Persona[];
  providerOptions: readonly AgentProviderAvailabilityOption[];
  onChange: (value: ManualRoleRuntimeSelection) => void;
  onReset: (value: ManualRoleRuntimeSelection) => void;
  onValidityChange: (issue: string | null) => void;
}) {
  const [value, setValue] = useState(initialValue);
  const [customized, setCustomized] = useState(hasSavedOverride);
  const effective = entry.effective;

  return (
    <div className="space-y-3 rounded-lg border p-3 text-left">
      <div>
        <p className="text-xs font-medium text-[var(--text-primary)]">
          {entry.familyDisplayName} → {entry.displayName}
        </p>
        <p className="text-[11px] text-[var(--text-muted)]">
          {customized ? "Saved for this conversation" : `Role default${entry.source ? ` · ${entry.source}` : ""}`}
        </p>
      </div>
      {entry.diagnostics.map((diagnostic) => (
        <p key={diagnostic} className="text-xs text-[var(--status-warning)]">
          {diagnostic}
        </p>
      ))}
      <ManualRoleRuntimeSelector
        entry={entry}
        value={value}
        providerOptions={providerOptions}
        modelsForProvider={(provider) =>
          modelRegistry[provider as keyof AgentModelRegistry] ?? []
        }
        personas={personas}
        onChange={(next) => {
          setValue(next);
          setCustomized(true);
          onChange(next);
        }}
        onValidityChange={onValidityChange}
        runtimeDefault={{
          source: customized ? "conversation override" : entry.source,
          onReset: () => {
            if (!effective) return;
            const next: ManualRoleRuntimeSelection = effective;
            setValue(next);
            setCustomized(false);
            onReset(next);
          },
          disabled: !effective,
        }}
      />
    </div>
  );
}
