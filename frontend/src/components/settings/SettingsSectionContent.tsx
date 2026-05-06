import { Suspense, lazy } from "react";

import type { ProjectSettings } from "@/types/settings";

import type { SettingsSectionId } from "./settings-registry";

const LazyExecutionSection = lazy(() => import("./sections/ExecutionSection"));
const LazyExecutionHarnessSection = lazy(() =>
  import("./IdeationHarnessSection").then((module) => ({
    default: module.ExecutionHarnessSection,
  })),
);
const LazyGlobalExecutionSection = lazy(() =>
  import("./sections/GlobalExecutionSection"),
);
const LazyAgentModelsSection = lazy(() =>
  import("./AgentModelsSection").then((module) => ({
    default: module.AgentModelsSection,
  })),
);
const LazyReviewPolicySection = lazy(() =>
  import("./sections/ReviewPolicySection"),
);
const LazyRepositorySettingsSection = lazy(() =>
  import("./RepositorySettingsSection").then((module) => ({
    default: module.RepositorySettingsSection,
  })),
);
const LazyProjectAnalysisSection = lazy(() =>
  import("./ProjectAnalysisSection").then((module) => ({
    default: module.ProjectAnalysisSection,
  })),
);
const LazyIdeationSettingsPanel = lazy(() =>
  import("./IdeationSettingsPanel").then((module) => ({
    default: module.IdeationSettingsPanel,
  })),
);
const LazyIdeationHarnessSection = lazy(() =>
  import("./IdeationHarnessSection").then((module) => ({
    default: module.IdeationHarnessSection,
  })),
);
const LazyApiKeysSection = lazy(() =>
  import("./ApiKeysSection").then((module) => ({
    default: module.ApiKeysSection,
  })),
);
const LazyExternalMcpSettingsPanel = lazy(() =>
  import("./ExternalMcpSettingsPanel").then((module) => ({
    default: module.ExternalMcpSettingsPanel,
  })),
);
const LazyAccessibilitySection = lazy(() =>
  import("./AccessibilitySection").then((module) => ({
    default: module.AccessibilitySection,
  })),
);

function SettingsSectionLoading() {
  return (
    <div
      data-testid="settings-section-loading"
      className="space-y-4"
      aria-label="Loading settings section"
    >
      <div className="h-5 w-40 rounded bg-[var(--bg-hover)]" />
      <div className="h-24 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]" />
      <div className="h-24 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]" />
    </div>
  );
}

interface SettingsSectionContentProps {
  section: SettingsSectionId;
  executionSettings: ProjectSettings | null;
  disabled: boolean;
  isHydrated: boolean;
  onSettingsChange: (settings: ProjectSettings) => void;
}

export function SettingsSectionContent({
  section,
  executionSettings,
  disabled,
  isHydrated,
  onSettingsChange,
}: SettingsSectionContentProps) {
  if (!isHydrated) {
    return <SettingsSectionLoading />;
  }

  return (
    <Suspense fallback={<SettingsSectionLoading />}>
      {section === "execution" &&
        (executionSettings ? (
          <LazyExecutionSection
            settings={executionSettings.execution}
            onChange={(changes) =>
              onSettingsChange({
                ...executionSettings,
                execution: { ...executionSettings.execution, ...changes },
              })
            }
            disabled={disabled}
          />
        ) : null)}
      {section === "execution-harnesses" && <LazyExecutionHarnessSection />}
      {section === "models" && <LazyAgentModelsSection />}
      {section === "global-execution" && <LazyGlobalExecutionSection />}
      {section === "review" && <LazyReviewPolicySection />}
      {section === "repository" && <LazyRepositorySettingsSection />}
      {section === "project-analysis" && <LazyProjectAnalysisSection />}
      {section === "ideation-workflow" && <LazyIdeationSettingsPanel />}
      {section === "ideation-harnesses" && <LazyIdeationHarnessSection />}
      {section === "api-keys" && <LazyApiKeysSection />}
      {section === "external-mcp" && <LazyExternalMcpSettingsPanel />}
      {section === "accessibility" && <LazyAccessibilitySection />}
    </Suspense>
  );
}
