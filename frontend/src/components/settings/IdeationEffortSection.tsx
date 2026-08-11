/**
 * IdeationEffortSection — Settings section for configuring ideation agent effort levels.
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
import { useIdeationEffortSettings } from "@/hooks/useIdeationEffortSettings";
import { useProjectStore, selectActiveProject } from "@/stores/projectStore";

// ============================================================================
// Constants
// ============================================================================

const EFFORT_OPTIONS = [
  {
    value: "inherit",
    label: "Inherit",
    description: "Falls through to YAML config default",
  },
  {
    value: "low",
    label: "Low",
    description: "Fastest responses, minimal reasoning",
  },
  {
    value: "medium",
    label: "Medium",
    description: "Balanced speed and quality",
  },
  {
    value: "high",
    label: "High",
    description: "Deeper reasoning, slower responses",
  },
  {
    value: "max",
    label: "Maximum",
    description: "Highest quality, longest response time",
  },
] as const;

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
    default:
      return source;
  }
}

// ============================================================================
// EffortRow — Custom row with optional effective value hint
// ============================================================================

interface EffortRowProps {
  id: string;
  label: string;
  description: string;
  value: string;
  disabled: boolean;
  onChange: (value: string) => void;
  effectiveValue: string;
  effectiveSource: string;
  isPlaceholderData: boolean;
  isLast?: boolean;
}

function EffortRow({
  id,
  label,
  description,
  value,
  disabled,
  onChange,
  effectiveValue,
  effectiveSource,
  isPlaceholderData,
  isLast = false,
}: EffortRowProps) {
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
              <SelectValue placeholder="Select effort" />
            </SelectTrigger>
            <SelectContent className="bg-[var(--bg-elevated)] border-[var(--border-default)]">
              {EFFORT_OPTIONS.map((opt) => (
                <SelectItem
                  key={opt.value}
                  value={opt.value}
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
// GlobalEffortSubsection
// ============================================================================

function GlobalEffortSubsection() {
  const [showError, setShowError] = useState(false);
  const { settings, isPlaceholderData, updateSettings, saveError } = useIdeationEffortSettings(null);

  const handlePrimaryChange = (value: string) => {
    setShowError(false);
    updateSettings(
      { primaryEffort: value },
      { onError: () => setShowError(true) }
    );
  };

  return (
    <div>
      {showError && saveError && (
        <ErrorBanner
          error={saveError.message ?? "Failed to save effort settings"}
          onDismiss={() => setShowError(false)}
        />
      )}
      <div className="space-y-0">
        <EffortRow
          id="global-primary-effort"
          label="Primary Ideation Effort"
          description="Effort level for the primary ideation agent"
          value={settings.primaryEffort}
          disabled={false}
          onChange={handlePrimaryChange}
          effectiveValue={settings.effectivePrimary}
          effectiveSource={settings.primarySource}
          isPlaceholderData={isPlaceholderData}
        />
      </div>
    </div>
  );
}

// ============================================================================
// ProjectEffortSubsection
// ============================================================================

interface ProjectEffortSubsectionProps {
  projectId: string | null;
  projectName: string | null;
}

function ProjectEffortSubsection({
  projectId,
  projectName,
}: ProjectEffortSubsectionProps) {
  const [showError, setShowError] = useState(false);
  const { settings, isPlaceholderData, updateSettings, saveError } = useIdeationEffortSettings(projectId);
  const isDisabled = projectId === null;

  const handlePrimaryChange = (value: string) => {
    if (isDisabled) return;
    setShowError(false);
    updateSettings(
      { primaryEffort: value },
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
          error={saveError.message ?? "Failed to save effort settings"}
          onDismiss={() => setShowError(false)}
        />
      )}
      <div className="space-y-0">
        <EffortRow
          id="project-primary-effort"
          label="Primary Ideation Effort"
          description="Override for this project's ideation agents"
          value={settings.primaryEffort}
          disabled={isDisabled}
          onChange={handlePrimaryChange}
          effectiveValue={settings.effectivePrimary}
          effectiveSource={settings.primarySource}
          isPlaceholderData={isPlaceholderData}
        />
      </div>
    </div>
  );
}

// ============================================================================
// IdeationEffortSection — Main export
// ============================================================================

export function IdeationEffortSection() {
  const activeProject = useProjectStore(selectActiveProject);

  return (
    <SettingsSection>
      <GlobalEffortSubsection />
      <Separator className="my-4 bg-[var(--border-subtle)]" />
      <ProjectEffortSubsection
        projectId={activeProject?.id ?? null}
        projectName={activeProject?.name ?? null}
      />
    </SettingsSection>
  );
}
