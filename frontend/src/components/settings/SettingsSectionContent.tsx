import { Suspense, lazy } from "react";

import type { ProjectSettings } from "@/types/settings";
import { useIdeationSettings } from "@/hooks/useIdeationSettings";

import type { SettingsCompositeTab, SettingsSectionId } from "./settings-registry";

const LazyAgentsSettingsSection = lazy(() =>
  import("./AgentsSettingsSection").then((module) => ({
    default: module.AgentsSettingsSection,
  })),
);
const LazyHarnessProvidersSection = lazy(() =>
  import("./HarnessProvidersSection").then((module) => ({
    default: module.HarnessProvidersSection,
  })),
);
const LazyAgentModelsSection = lazy(() =>
  import("./AgentModelsSection").then((module) => ({
    default: module.AgentModelsSection,
  })),
);
const LazyTasksSettingsSection = lazy(() => import("./sections/TasksSettingsSection"));
const LazyPlanningSettingsSection = lazy(() => import("./sections/PlanningSettingsSection"));
const LazyWorkspaceSettingsSection = lazy(() => import("./sections/WorkspaceSettingsSection"));
const LazyCapacitySettingsSection = lazy(() => import("./sections/CapacitySettingsSection"));
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
const LazyRemoteAccessSection = lazy(() =>
  import("./remote-access/RemoteAccessSection").then((module) => ({
    default: module.RemoteAccessSection,
  })),
);

const LazyConnectionsSection = lazy(() =>
  import("./connections/ConnectionsSection").then((module) => ({
    default: module.ConnectionsSection,
  })),
);
const LazyAtlassianIntegrationSettingsPanel = lazy(() =>
  import("./AtlassianIntegrationSettingsPanel").then((module) => ({
    default: module.AtlassianIntegrationSettingsPanel,
  })),
);
const LazyGitHubIntegrationSettingsPanel = lazy(() =>
  import("./GitHubIntegrationSettingsPanel").then((module) => ({
    default: module.GitHubIntegrationSettingsPanel,
  })),
);
const LazyLinearIntegrationSettingsPanel = lazy(() =>
  import("./LinearIntegrationSettingsPanel").then((module) => ({
    default: module.LinearIntegrationSettingsPanel,
  })),
);
const LazyClickUpIntegrationSettingsPanel = lazy(() =>
  import("./ClickUpIntegrationSettingsPanel").then((module) => ({
    default: module.ClickUpIntegrationSettingsPanel,
  })),
);
const LazyGranolaIntegrationSettingsPanel = lazy(() =>
  import("./GranolaIntegrationSettingsPanel").then((module) => ({
    default: module.GranolaIntegrationSettingsPanel,
  })),
);
const LazyMcpSettingsSection = lazy(() =>
  import("./McpSettingsSection").then((module) => ({
    default: module.McpSettingsSection,
  })),
);
const LazyAccessibilitySection = lazy(() =>
  import("./AccessibilitySection").then((module) => ({
    default: module.AccessibilitySection,
  })),
);
const LazyNotificationSettingsPanel = lazy(() =>
  import("./NotificationSettingsPanel").then((module) => ({
    default: module.NotificationSettingsPanel,
  })),
);
const LazyUpdatesSettingsSection = lazy(() =>
  import("./UpdatesSettingsSection").then((module) => ({
    default: module.UpdatesSettingsSection,
  })),
);
const LazyPersonasSection = lazy(() =>
  import("./PersonasSection").then((module) => ({
    default: module.PersonasSection,
  })),
);
const LazyCapabilitiesSection = lazy(() =>
  import("./CapabilitiesSection").then((module) => ({
    default: module.CapabilitiesSection,
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
  destinationTab?: SettingsCompositeTab;
  executionSettings: ProjectSettings | null;
  disabled: boolean;
  isHydrated: boolean;
  onSettingsChange: (settings: ProjectSettings) => void;
}

export function SettingsSectionContent({
  section,
  destinationTab,
  executionSettings,
  disabled,
  isHydrated,
  onSettingsChange,
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
      {section === "integrations" && <LazyAtlassianIntegrationSettingsPanel />}
      {section === "github" && <LazyGitHubIntegrationSettingsPanel />}
      {section === "linear" && <LazyLinearIntegrationSettingsPanel />}
      {section === "clickup" && <LazyClickUpIntegrationSettingsPanel />}
      {section === "granola" && <LazyGranolaIntegrationSettingsPanel />}
      {section === "api-keys" && <LazyApiKeysSection />}
      {section === "external-mcp" && <LazyExternalMcpSettingsPanel />}
      {section === "remote-access" && <LazyRemoteAccessSection />}
      {section === "connections" && <LazyConnectionsSection />}
      {section === "mcp" && <LazyMcpSettingsSection />}
      {section === "updates" && <LazyUpdatesSettingsSection />}
      {section === "accessibility" && <LazyAccessibilitySection />}
      {section === "notifications" && <LazyNotificationSettingsPanel />}
    </Suspense>
  );
}
