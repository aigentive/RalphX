/**
 * IdeationModelSection — Settings section for configuring ideation agent model selection.
 *
 * Uses the chrome-free SettingsSection container from SettingsView.shared.tsx.
 * Shows global dropdowns and per-project override dropdowns.
 * Effective value hint shown only when value is `inherit`.
 */

import { useState } from "react";
import { Separator } from "@/components/ui/separator";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { SettingsSection, ErrorBanner } from "./SettingsView.shared";
import { useHarnessProviders } from "@/hooks/useHarnessProviders";
import { useIdeationModelSettings } from "@/hooks/useIdeationModelSettings";
import { useProjectStore, selectActiveProject } from "@/stores/projectStore";
import { isAgentModelSelectableForProvider } from "@/lib/agent-models";

// ============================================================================
// Constants
// ============================================================================

interface ModelOption {
  value: string;
  label: string;
  description: string;
  disabled?: boolean;
}

const MODEL_OPTIONS = [
  {
    value: "inherit",
    label: "Inherit",
    description: "Use default from configuration",
  },
  {
    value: "sonnet",
    label: "Sonnet",
    description: "Fast and capable",
  },
  {
    value: "opus",
    label: "Opus",
    description: "Most capable, highest cost",
  },
  {
    value: "claude-opus-4-7",
    label: "Claude Opus 4.7",
    description: "Claude Opus 4.7, requires Claude Code 2.1.111+",
  },
  {
    value: "claude-opus-4-8",
    label: "Claude Opus 4.8",
    description: "Claude Opus 4.8, requires Claude Code 2.1.154+",
  },
  {
    value: "claude-opus-5",
    label: "Claude Opus 5",
    description: "Claude Opus 5, requires Claude Code 2.1.219+",
  },
  {
    value: "haiku",
    label: "Haiku",
    description: "Fastest, lowest cost",
  },
  {
    value: "fable",
    label: "Fable",
    description: "Claude Fable 5, requires Claude Code 2.1.170+",
  },
] as const satisfies readonly ModelOption[];

// ============================================================================
// Helpers
// ============================================================================

function formatSource(source: string): string {
  switch (source) {
    case "user":
      return "user override";
    case "global":
      return "global setting";
    case "yaml":
      return "YAML config";
    case "yaml_default":
      return "YAML default";
    case "default":
      return "Default";
    default:
      return source;
  }
}

function modelOptionsForClaudeCapabilities(
  providers: readonly {
    provider: string;
    supportedModelAliases?: readonly string[] | null | undefined;
  }[],
): readonly ModelOption[] {
  const aliases =
    providers.find((provider) => provider.provider === "claude")
      ?.supportedModelAliases ?? null;
  return MODEL_OPTIONS.map((option) =>
    option.value === "inherit" ||
    isAgentModelSelectableForProvider("claude", option.value, aliases)
      ? option
      : { ...option, disabled: true },
  );
}

// ============================================================================
// ModelRow — Custom row with optional effective value hint
// ============================================================================

interface ModelRowProps {
  id: string;
  label: string;
  description: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
  effectiveValue: string;
  effectiveSource: string;
  isPlaceholderData: boolean;
  modelOptions: readonly ModelOption[];
  isLast?: boolean;
}

function ModelRow({
  id,
  label,
  description,
  value,
  disabled,
  onChange,
  effectiveValue,
  effectiveSource,
  isPlaceholderData,
  modelOptions,
  isLast = false,
}: ModelRowProps) {
  const showHint = value === "inherit" && !isPlaceholderData && !!effectiveValue;

  return (
    <div
      className={
        isLast
          ? undefined
          : "border-b border-[var(--border-subtle)]"
      }
    >
      <div
        className={[
          "flex items-start justify-between py-3 -mx-2 px-2 rounded-md transition-colors",
          !disabled ? "hover:bg-[var(--bg-hover)]" : "opacity-50",
        ].join(" ")}
      >
        <div className="flex-1 min-w-0 pr-4">
          <label
            htmlFor={id}
            className="text-sm font-medium text-[var(--text-primary)]"
          >
            {label}
          </label>
          <p className="text-xs text-[var(--text-muted)] mt-0.5">{description}</p>
        </div>
        <div className="shrink-0">
          <Select value={value} onValueChange={onChange} disabled={disabled}>
            <SelectTrigger
              id={id}
              data-testid={id}
              className="w-[180px] bg-[var(--bg-surface)] border-[var(--border-default)] focus:ring-[var(--accent-primary)]"
            >
              <SelectValue placeholder="Select model" />
            </SelectTrigger>
            <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-default)]">
              {modelOptions.map((opt) => (
                <SelectItem
                  key={opt.value}
                  value={opt.value}
                  disabled={opt.disabled === true}
                  className="focus:bg-[var(--accent-muted)]"
                >
                  <div className="flex flex-col">
                    <span className="text-[var(--text-primary)]">{opt.label}</span>
                    <span className="text-xs text-[var(--text-muted)]">
                      {opt.description}
                    </span>
                  </div>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
      {showHint && (
        <p className="text-xs text-[var(--text-muted)] pb-2 px-2">
          Effective: <span className="text-[var(--text-secondary)]">{effectiveValue}</span>{" "}
          (from {formatSource(effectiveSource)})
        </p>
      )}
    </div>
  );
}

// ============================================================================
// GlobalModelSubsection
// ============================================================================

function GlobalModelSubsection({
  modelOptions,
}: {
  modelOptions: readonly ModelOption[];
}) {
  const [showError, setShowError] = useState(false);
  const { settings, isPlaceholderData, updateSettings, saveError } = useIdeationModelSettings(null);

  const handlePrimaryChange = (value: string) => {
    setShowError(false);
    updateSettings(
      { primaryModel: value },
      { onError: () => setShowError(true) }
    );
  };

  const handleIdeationSubagentChange = (value: string) => {
    setShowError(false);
    updateSettings(
      { ideationSubagentModel: value },
      { onError: () => setShowError(true) }
    );
  };

  return (
    <div>
      {showError && saveError && (
        <ErrorBanner
          error={saveError.message ?? "Failed to save model settings"}
          onDismiss={() => setShowError(false)}
        />
      )}
      <div className="space-y-0">
        <ModelRow
          id="global-primary-model"
          label="Primary Ideation Model"
          description="Model for the primary ideation agent"
          value={settings.primaryModel}
          disabled={false}
          onChange={handlePrimaryChange}
          effectiveValue={settings.effectivePrimaryModel}
          effectiveSource={settings.primaryModelSource}
          isPlaceholderData={isPlaceholderData}
          modelOptions={modelOptions}
        />
        <ModelRow
          id="ideation-subagent-model"
          label="Ideation Subagent Model"
          description="Model used by subagents spawned by ralphx-ideation"
          value={settings.ideationSubagentModel ?? "inherit"}
          disabled={false}
          onChange={handleIdeationSubagentChange}
          effectiveValue={settings.effectiveIdeationSubagentModel ?? ""}
          effectiveSource={settings.ideationSubagentModelSource ?? ""}
          isPlaceholderData={isPlaceholderData}
          modelOptions={modelOptions}
          isLast
        />
      </div>
    </div>
  );
}

// ============================================================================
// ProjectModelSubsection
// ============================================================================

interface ProjectModelSubsectionProps {
  projectId: string | null;
  projectName: string | null;
  modelOptions: readonly ModelOption[];
}

function ProjectModelSubsection({
  projectId,
  projectName,
  modelOptions,
}: ProjectModelSubsectionProps) {
  const [showError, setShowError] = useState(false);
  const { settings, isPlaceholderData, updateSettings, saveError } = useIdeationModelSettings(projectId);
  const isDisabled = projectId === null;

  const handlePrimaryChange = (value: string) => {
    if (isDisabled) return;
    setShowError(false);
    updateSettings(
      { primaryModel: value },
      { onError: () => setShowError(true) }
    );
  };

  const handleIdeationSubagentChange = (value: string) => {
    if (isDisabled) return;
    setShowError(false);
    updateSettings(
      { ideationSubagentModel: value },
      { onError: () => setShowError(true) }
    );
  };

  return (
    <div>
      <div className="mb-3">
        <p className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
          {projectName ? `Project: ${projectName}` : "Project Override"}
        </p>
        {isDisabled && (
          <p className="text-xs text-[var(--text-muted)] mt-1">
            Select a project to configure per-project overrides
          </p>
        )}
      </div>
      {showError && saveError && (
        <ErrorBanner
          error={saveError.message ?? "Failed to save model settings"}
          onDismiss={() => setShowError(false)}
        />
      )}
      <div className="space-y-0">
        <ModelRow
          id="project-primary-model"
          label="Primary Ideation Model"
          description="Override for this project's ideation agents"
          value={settings.primaryModel}
          disabled={isDisabled}
          onChange={handlePrimaryChange}
          effectiveValue={settings.effectivePrimaryModel}
          effectiveSource={settings.primaryModelSource}
          isPlaceholderData={isPlaceholderData}
          modelOptions={modelOptions}
        />
        <ModelRow
          id="project-ideation-subagent-model"
          label="Ideation Subagent Model"
          description="Override ideation subagent model for this project"
          value={settings.ideationSubagentModel ?? "inherit"}
          disabled={isDisabled}
          onChange={handleIdeationSubagentChange}
          effectiveValue={settings.effectiveIdeationSubagentModel ?? ""}
          effectiveSource={settings.ideationSubagentModelSource ?? ""}
          isPlaceholderData={isPlaceholderData}
          modelOptions={modelOptions}
          isLast
        />
      </div>
    </div>
  );
}

// ============================================================================
// IdeationModelSection — Main export
// ============================================================================

export function IdeationModelSection() {
  const activeProject = useProjectStore(selectActiveProject);
  const { providers } = useHarnessProviders({ refreshRuntime: true });
  const modelOptions = modelOptionsForClaudeCapabilities(providers);

  return (
    <SettingsSection>
      <GlobalModelSubsection modelOptions={modelOptions} />
      <Separator className="my-4 bg-[var(--border-subtle)]" />
      <ProjectModelSubsection
        projectId={activeProject?.id ?? null}
        projectName={activeProject?.name ?? null}
        modelOptions={modelOptions}
      />
    </SettingsSection>
  );
}
