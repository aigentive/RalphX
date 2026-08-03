import { Suspense } from "react";

import { lazyWithRetry } from "@/lib/lazy-with-retry";
import type { ProjectSettings } from "@/types/settings";
import { useIdeationSettings } from "@/hooks/useIdeationSettings";

import type { SettingsCompositeTab, SettingsSectionId } from "./settings-registry";

const LazyAgentsSettingsSection = lazyWithRetry(() =>
  import("./AgentsSettingsSection").then((module) => ({
    default: module.AgentsSettingsSection,
  })),
);
const LazyHarnessProvidersSection = lazyWithRetry(() =>
  import("./HarnessProvidersSection").then((module) => ({
    default: module.HarnessProvidersSection,
  })),
);
const LazyAgentModelsSection = lazyWithRetry(() =>
  import("./AgentModelsSection").then((module) => ({
    default: module.AgentModelsSection,
  })),
);
const LazyTasksSettingsSection = lazyWithRetry(() => import("./sections/TasksSettingsSection"));
const LazyPlanningSettingsSection = lazyWithRetry(() => import("./sections/PlanningSettingsSection"));
const LazyWorkspaceSettingsSection = lazyWithRetry(() => import("./sections/WorkspaceSettingsSection"));
const LazyCapacitySettingsSection = lazyWithRetry(() => import("./sections/CapacitySettingsSection"));
const LazyRepositorySettingsSection = lazyWithRetry(() =>
  import("./RepositorySettingsSection").then((module) => ({
    default: module.RepositorySettingsSection,
  })),
);
const LazyProjectAnalysisSection = lazyWithRetry(() =>
  import("./ProjectAnalysisSection").then((module) => ({
    default: module.ProjectAnalysisSection,
  })),
);
const LazyApiKeysSection = lazyWithRetry(() =>
  import("./ApiKeysSection").then((module) => ({
    default: module.ApiKeysSection,
  })),
);
const LazyExternalMcpSettingsPanel = lazyWithRetry(() =>
  import("./ExternalMcpSettingsPanel").then((module) => ({
    default: module.ExternalMcpSettingsPanel,
  })),
);
const LazyAtlassianIntegrationSettingsPanel = lazyWithRetry(() =>
  import("./AtlassianIntegrationSettingsPanel").then((module) => ({
    default: module.AtlassianIntegrationSettingsPanel,
  })),
);
const LazyGitHubIntegrationSettingsPanel = lazyWithRetry(() =>
  import("./GitHubIntegrationSettingsPanel").then((module) => ({
    default: module.GitHubIntegrationSettingsPanel,
  })),
);
const LazyLinearIntegrationSettingsPanel = lazyWithRetry(() =>
  import("./LinearIntegrationSettingsPanel").then((module) => ({
    default: module.LinearIntegrationSettingsPanel,
  })),
);
const LazyClickUpIntegrationSettingsPanel = lazyWithRetry(() =>
  import("./ClickUpIntegrationSettingsPanel").then((module) => ({
    default: module.ClickUpIntegrationSettingsPanel,
  })),
);
const LazyGranolaIntegrationSettingsPanel = lazyWithRetry(() =>
  import("./GranolaIntegrationSettingsPanel").then((module) => ({
    default: module.GranolaIntegrationSettingsPanel,
  })),
);
const LazyMcpSettingsSection = lazyWithRetry(() =>
  import("./McpSettingsSection").then((module) => ({
    default: module.McpSettingsSection,
  })),
);
const LazyAccessibilitySection = lazyWithRetry(() =>
  import("./AccessibilitySection").then((module) => ({
    default: module.AccessibilitySection,
  })),
);
const LazyNotificationSettingsPanel = lazyWithRetry(() =>
  import("./NotificationSettingsPanel").then((module) => ({
    default: module.NotificationSettingsPanel,
  })),
);
const LazyUpdatesSettingsSection = lazyWithRetry(() =>
  import("./UpdatesSettingsSection").then((module) => ({
    default: module.UpdatesSettingsSection,
  })),
);
const LazyDatabaseMaintenanceSection = lazyWithRetry(() =>
  import("./DatabaseMaintenanceSection").then((module) => ({
    default: module.DatabaseMaintenanceSection,
  })),
);
const LazyPersonasSection = lazyWithRetry(() =>
  import("./PersonasSection").then((module) => ({
    default: module.PersonasSection,
  })),
);
const LazyCapabilitiesSection = lazyWithRetry(() =>
  import("./CapabilitiesSection").then((module) => ({
    default: module.CapabilitiesSection,
  })),
);
const LazyIntegrationsHubSection = lazyWithRetry(() =>
  import("./IntegrationsHubSection").then((module) => ({
    default: module.IntegrationsHubSection,
  })),
);

function SettingsSectionLoading() {
  return (
    <div
      data-testid="settings-section-loading"
      className="space-y-4"
      aria-label="Loading settings section"
    >
      <div className="h-24 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]" />
      <div className="h-24 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)]" />
    </div>
  );
}

interface SettingsSectionContentProps {
  section: SettingsSectionId;
  destinationTab?: SettingsCompositeTab;
  executionSettings: ProjectSettings | null;
  disabled: boolean;
  isHydrated: boolean;
  onSettingsChange: (settings: ProjectSettings) => void;
  /** Routes hub cards through the same section setter the nav rail uses. */
  onNavigate: (section: SettingsSectionId) => void;
  /** Preloads a drill-in panel's lazy module; deduped by the caller. */
  onWarmSection: (section: SettingsSectionId) => void;
}

export function SettingsSectionContent({
  section,
  destinationTab,
  executionSettings,
  disabled,
  isHydrated,
  onSettingsChange,
  onNavigate,
  onWarmSection,
}: SettingsSectionContentProps) {
  const ideationController = useIdeationSettings(
    isHydrated && (section === "tasks" || section === "planning"),
  );
  if (!isHydrated) {
    return <SettingsSectionLoading />;
  }

  return (
    <Suspense fallback={<SettingsSectionLoading />}>
      {section === "providers" && <LazyHarnessProvidersSection />}
      {section === "agents" && <LazyAgentsSettingsSection />}
      {section === "models" && <LazyAgentModelsSection />}
      {section === "personas" && <LazyPersonasSection />}
      {section === "capabilities" && <LazyCapabilitiesSection />}
      {section === "tasks" && (
        <LazyTasksSettingsSection
          controller={ideationController}
          initialTab={
            destinationTab === "review-policy" || destinationTab === "autonomy-policy"
              ? destinationTab
              : "general"
          }
        />
      )}
      {section === "planning" && (
        <LazyPlanningSettingsSection controller={ideationController} />
      )}
      {section === "workspace" && executionSettings && (
        <LazyWorkspaceSettingsSection
          settings={executionSettings}
          disabled={disabled}
          onSettingsChange={onSettingsChange}
          initialTab={destinationTab === "review" ? "review" : "general"}
        />
      )}
      {section === "capacity" && executionSettings && (
        <LazyCapacitySettingsSection
          settings={executionSettings}
          disabled={disabled}
          onSettingsChange={onSettingsChange}
        />
      )}
      {section === "repository" && <LazyRepositorySettingsSection />}
      {section === "project-analysis" && <LazyProjectAnalysisSection />}
      {section === "integrations-hub" && (
        <LazyIntegrationsHubSection
          onNavigate={onNavigate}
          onWarmSection={onWarmSection}
        />
      )}
      {section === "integrations" && <LazyAtlassianIntegrationSettingsPanel />}
      {section === "github" && <LazyGitHubIntegrationSettingsPanel />}
      {section === "linear" && <LazyLinearIntegrationSettingsPanel />}
      {section === "clickup" && <LazyClickUpIntegrationSettingsPanel />}
      {section === "granola" && <LazyGranolaIntegrationSettingsPanel />}
      {section === "api-keys" && <LazyApiKeysSection />}
      {section === "external-mcp" && <LazyExternalMcpSettingsPanel />}
      {section === "mcp" && <LazyMcpSettingsSection />}
      {section === "updates" && <LazyUpdatesSettingsSection />}
      {section === "database" && <LazyDatabaseMaintenanceSection />}
      {section === "accessibility" && <LazyAccessibilitySection />}
      {section === "notifications" && <LazyNotificationSettingsPanel />}
    </Suspense>
  );
}
