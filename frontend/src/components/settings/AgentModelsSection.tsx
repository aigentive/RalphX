import { useMemo, useState } from "react";
import { Cpu, Plus, Save, Trash2 } from "lucide-react";

import type { AgentModelResponse } from "@/api/agent-models";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { useAgentModels } from "@/hooks/useAgentModels";
import {
  AGENT_EFFORT_CATALOG,
  type AgentEffort,
  type AgentProvider,
} from "@/lib/agent-models";

import { ErrorBanner, SectionCard } from "./SettingsView.shared";

const PROVIDERS: Array<{ value: AgentProvider; label: string }> = [
  { value: "claude", label: "Claude" },
  { value: "codex", label: "Codex" },
];

interface ModelFormState {
  provider: AgentProvider;
  modelId: string;
  label: string;
  menuLabel: string;
  description: string;
  supportedEfforts: AgentEffort[];
  defaultEffort: AgentEffort;
  enabled: boolean;
}

function emptyForm(): ModelFormState {
  return {
    provider: "codex",
    modelId: "",
    label: "",
    menuLabel: "",
    description: "",
    supportedEfforts: ["low", "medium", "high", "xhigh"],
    defaultEffort: "xhigh",
    enabled: true,
  };
}

function formFromModel(model: AgentModelResponse): ModelFormState {
  const supportedEfforts = model.supportedEfforts.filter(
    (effort): effort is AgentEffort =>
      AGENT_EFFORT_CATALOG.some((entry) => entry.id === effort),
  );
  const defaultEffort = supportedEfforts.includes(model.defaultEffort as AgentEffort)
    ? (model.defaultEffort as AgentEffort)
    : supportedEfforts[0] ?? "medium";

  return {
    provider: model.provider as AgentProvider,
    modelId: model.modelId,
    label: model.label,
    menuLabel: model.menuLabel,
    description: model.description ?? "",
    supportedEfforts,
    defaultEffort,
    enabled: model.enabled,
  };
}

function providerLabel(provider: string): string {
  return PROVIDERS.find((entry) => entry.value === provider)?.label ?? provider;
}

function sourceLabel(source: AgentModelResponse["source"]): string {
  return source === "custom" ? "Custom" : "Built-in";
}

function effortLabel(effort: string): string {
  return AGENT_EFFORT_CATALOG.find((entry) => entry.id === effort)?.label ?? effort;
}

export function AgentModelsSection() {
  const {
    models,
    isError,
    error,
    upsertError,
    deleteError,
    upsertModelAsync,
    deleteModelAsync,
    isUpserting,
    isDeleting,
  } = useAgentModels();
  const [form, setForm] = useState<ModelFormState>(() => emptyForm());
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);

  const modelsByProvider = useMemo(
    () =>
      PROVIDERS.map((provider) => ({
        ...provider,
        models: models.filter((model) => model.provider === provider.value),
      })),
    [models],
  );

  const checkedEffortSet = useMemo(
    () => new Set(form.supportedEfforts),
    [form.supportedEfforts],
  );

  const resetForm = () => {
    setEditingKey(null);
    setLocalError(null);
    setForm(emptyForm());
  };

  const setSupportedEffort = (effort: AgentEffort, checked: boolean) => {
    setForm((current) => {
      const nextEfforts = checked
        ? [...new Set([...current.supportedEfforts, effort])]
        : current.supportedEfforts.filter((value) => value !== effort);
      const orderedEfforts = AGENT_EFFORT_CATALOG
        .map((entry) => entry.id)
        .filter((value) => nextEfforts.includes(value));
      const defaultEffort = orderedEfforts.includes(current.defaultEffort)
        ? current.defaultEffort
        : orderedEfforts[0] ?? current.defaultEffort;
      return {
        ...current,
        supportedEfforts: orderedEfforts,
        defaultEffort,
      };
    });
  };

  const saveModel = async () => {
    setLocalError(null);
    if (!form.modelId.trim()) {
      setLocalError("Model ID is required");
      return;
    }
    if (form.supportedEfforts.length === 0) {
      setLocalError("Select at least one effort");
      return;
    }

    await upsertModelAsync({
      provider: form.provider,
      modelId: form.modelId.trim(),
      label: form.label.trim() || form.modelId.trim(),
      menuLabel: form.menuLabel.trim() || form.label.trim() || form.modelId.trim(),
      description: form.description.trim() || null,
      supportedEfforts: form.supportedEfforts,
      defaultEffort: form.defaultEffort,
      enabled: form.enabled,
    });
    resetForm();
  };

  const removeModel = async (model: AgentModelResponse) => {
    setLocalError(null);
    await deleteModelAsync({ provider: model.provider, modelId: model.modelId });
    if (editingKey === `${model.provider}:${model.modelId}`) {
      resetForm();
    }
  };

  const displayedError =
    localError ??
    (isError && error instanceof Error ? error.message : null) ??
    (upsertError instanceof Error ? upsertError.message : null) ??
    (deleteError instanceof Error ? deleteError.message : null);

  return (
    <SectionCard
      icon={<Cpu className="h-5 w-5" />}
      title="Models"
      description="Manage provider models and effort compatibility used by Agents and lane settings."
    >
      {displayedError && (
        <ErrorBanner error={displayedError} onDismiss={() => setLocalError(null)} />
      )}

      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_320px]">
        <div className="space-y-4">
          {modelsByProvider.map((group) => (
            <div key={group.value} className="space-y-2">
              <div className="flex items-center justify-between">
                <h4 className="text-sm font-semibold text-[var(--text-primary)]">
                  {group.label}
                </h4>
                <span className="text-[11px] text-[var(--text-muted)]">
                  {group.models.length} models
                </span>
              </div>
              <div className="overflow-hidden rounded-lg border border-[var(--border-subtle)]">
                {group.models.map((model) => (
                  <div
                    key={`${model.provider}:${model.modelId}`}
                    className="flex items-start justify-between gap-3 border-b border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-3 last:border-b-0"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex min-w-0 flex-wrap items-center gap-2">
                        <span className="truncate text-sm font-medium text-[var(--text-primary)]">
                          {model.menuLabel}
                        </span>
                        <span className="rounded-md border border-[var(--border-subtle)] px-1.5 py-0.5 text-[10px] text-[var(--text-muted)]">
                          {sourceLabel(model.source)}
                        </span>
                        {!model.enabled && (
                          <span className="rounded-md border border-[var(--status-warning-border)] px-1.5 py-0.5 text-[10px] text-[var(--status-warning)]">
                            Disabled
                          </span>
                        )}
                      </div>
                      <p className="mt-1 truncate text-xs text-[var(--text-muted)]">
                        {model.modelId}
                      </p>
                      <p className="mt-1 text-[11px] text-[var(--text-secondary)]">
                        Default {effortLabel(model.defaultEffort)} ·{" "}
                        {model.supportedEfforts.map(effortLabel).join(", ")}
                      </p>
                    </div>
                    {model.source === "custom" && (
                      <div className="flex shrink-0 items-center gap-1.5">
                        <Button
                          type="button"
                          size="sm"
                          variant="secondary"
                          onClick={() => {
                            setEditingKey(`${model.provider}:${model.modelId}`);
                            setLocalError(null);
                            setForm(formFromModel(model));
                          }}
                        >
                          Edit
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          variant="destructive"
                          disabled={isDeleting}
                          onClick={() => void removeModel(model)}
                        >
                          <Trash2 className="mr-1 h-3.5 w-3.5" />
                          Delete
                        </Button>
                      </div>
                    )}
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>

        <div className="space-y-3 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-4">
          <div className="flex items-center justify-between">
            <h4 className="text-sm font-semibold text-[var(--text-primary)]">
              {editingKey ? "Edit Custom Model" : "Add Custom Model"}
            </h4>
            {editingKey && (
              <Button type="button" size="sm" variant="ghost" onClick={resetForm}>
                <Plus className="mr-1 h-3.5 w-3.5" />
                New
              </Button>
            )}
          </div>

          <div className="space-y-3">
            <div className="space-y-1">
              <Label htmlFor="agent-model-provider">Provider</Label>
              <Select
                value={form.provider}
                onValueChange={(value) =>
                  setForm((current) => ({
                    ...current,
                    provider: value as AgentProvider,
                  }))
                }
              >
                <SelectTrigger id="agent-model-provider">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PROVIDERS.map((provider) => (
                    <SelectItem key={provider.value} value={provider.value}>
                      {provider.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-1">
              <Label htmlFor="agent-model-id">Model ID</Label>
              <Input
                id="agent-model-id"
                value={form.modelId}
                onChange={(event) =>
                  setForm((current) => ({ ...current, modelId: event.target.value }))
                }
                placeholder="gpt-5.6"
              />
            </div>

            <div className="space-y-1">
              <Label htmlFor="agent-model-label">Label</Label>
              <Input
                id="agent-model-label"
                value={form.label}
                onChange={(event) =>
                  setForm((current) => ({ ...current, label: event.target.value }))
                }
                placeholder="gpt-5.6"
              />
            </div>

            <div className="space-y-1">
              <Label htmlFor="agent-model-menu-label">Menu Label</Label>
              <Input
                id="agent-model-menu-label"
                value={form.menuLabel}
                onChange={(event) =>
                  setForm((current) => ({ ...current, menuLabel: event.target.value }))
                }
                placeholder={`${providerLabel(form.provider)} model`}
              />
            </div>

            <div className="space-y-1">
              <Label htmlFor="agent-model-description">Description</Label>
              <Input
                id="agent-model-description"
                value={form.description}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    description: event.target.value,
                  }))
                }
                placeholder="Optional"
              />
            </div>

            <div className="space-y-2">
              <Label>Supported Efforts</Label>
              <div className="grid grid-cols-2 gap-2">
                {AGENT_EFFORT_CATALOG.map((effort) => (
                  <label
                    key={effort.id}
                    className="flex items-center gap-2 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-2 py-1.5 text-xs text-[var(--text-primary)]"
                  >
                    <Checkbox
                      checked={checkedEffortSet.has(effort.id)}
                      onCheckedChange={(checked) =>
                        setSupportedEffort(effort.id, checked === true)
                      }
                    />
                    {effort.label}
                  </label>
                ))}
              </div>
            </div>

            <div className="space-y-1">
              <Label htmlFor="agent-model-default-effort">Default Effort</Label>
              <Select
                value={form.defaultEffort}
                onValueChange={(value) =>
                  setForm((current) => ({
                    ...current,
                    defaultEffort: value as AgentEffort,
                  }))
                }
              >
                <SelectTrigger id="agent-model-default-effort">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {form.supportedEfforts.map((effort) => (
                    <SelectItem key={effort} value={effort}>
                      {effortLabel(effort)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="flex items-center justify-between rounded-md border border-[var(--border-subtle)] bg-[var(--bg-elevated)] px-3 py-2">
              <Label htmlFor="agent-model-enabled">Enabled</Label>
              <Switch
                id="agent-model-enabled"
                checked={form.enabled}
                onCheckedChange={(checked) =>
                  setForm((current) => ({ ...current, enabled: checked }))
                }
              />
            </div>

            <Button
              type="button"
              onClick={() => void saveModel()}
              disabled={isUpserting}
              className="w-full"
            >
              <Save className="mr-2 h-4 w-4" />
              Save Model
            </Button>
          </div>
        </div>
      </div>
    </SectionCard>
  );
}
