export type SettingsSectionId =
  | "providers"
  | "agents"
  | "models"
  | "personas"
  | "capabilities"
  | "tasks"
  | "planning"
  | "workspace"
  | "capacity"
  | "repository"
  | "project-analysis"
  | "api-keys"
  | "external-mcp"
  | "remote-access"
  | "integrations"
  | "github"
  | "linear"
  | "clickup"
  | "granola"
  | "mcp"
  | "updates"
  | "accessibility"
  | "notifications";

export type SettingsGroupId =
  | "harness"
  | "general"
  | "workspace"
  | "access"
  | "integrations"
  | "preferences";

export interface SettingsSectionMeta {
  id: SettingsSectionId;
  label: string;
  groupId: SettingsGroupId;
}

export const SETTINGS_GROUPS: { id: SettingsGroupId; label: string }[] = [
  { id: "harness", label: "Harness" },
  { id: "workspace", label: "Workspace" },
  { id: "general", label: "General" },
  { id: "integrations", label: "Integrations" },
  { id: "access", label: "External Access" },
  { id: "preferences", label: "Preferences" },
];

export const DEFAULT_SETTINGS_SECTION: SettingsSectionId = "providers";

export const SETTINGS_SECTIONS: SettingsSectionMeta[] = [
  { id: "providers", groupId: "harness", label: "Providers" },
  { id: "models", groupId: "harness", label: "Models" },
  { id: "agents", groupId: "harness", label: "Agents" },
  { id: "mcp", groupId: "harness", label: "MCP" },
  { id: "repository", groupId: "workspace", label: "Repository" },
  { id: "project-analysis", groupId: "workspace", label: "Setup & Validation" },
  { id: "tasks", groupId: "general", label: "Tasks" },
  { id: "planning", groupId: "general", label: "Planning" },
  { id: "workspace", groupId: "general", label: "Workspace" },
  { id: "capacity", groupId: "general", label: "Capacity" },
  { id: "personas", groupId: "general", label: "Personas" },
  { id: "capabilities", groupId: "general", label: "Capabilities" },
  { id: "integrations", groupId: "integrations", label: "Atlassian" },
  { id: "github", groupId: "integrations", label: "GitHub" },
  { id: "linear", groupId: "integrations", label: "Linear" },
  { id: "clickup", groupId: "integrations", label: "ClickUp" },
  { id: "granola", groupId: "integrations", label: "Granola" },
  { id: "api-keys", groupId: "access", label: "API Keys" },
  { id: "external-mcp", groupId: "access", label: "External MCP" },
  { id: "remote-access", groupId: "access", label: "Remote Access" },
  { id: "updates", groupId: "preferences", label: "Updates" },
  { id: "accessibility", groupId: "preferences", label: "Accessibility" },
  { id: "notifications", groupId: "preferences", label: "Notifications" },
];

/** Feature-flag slice the settings nav depends on (subset of FeatureFlags). */
export interface SettingsSectionFlagGates {
  remoteEnvironments?: boolean;
}

/**
 * Sections visible for the given feature flags. `remote-access` ships dark
 * behind `remoteEnvironments` (PR 1.7, §8 flags note).
 */
export function visibleSettingsSections(
  flags: SettingsSectionFlagGates,
): SettingsSectionMeta[] {
  return SETTINGS_SECTIONS.filter(
    (section) => section.id !== "remote-access" || flags.remoteEnvironments === true,
  );
}

export type SettingsCompositeTab = "general" | "review-policy" | "autonomy-policy" | "review";

export interface SettingsDestination {
  section: SettingsSectionId;
  tab?: SettingsCompositeTab;
}

export function isSettingsSectionId(value: unknown): value is SettingsSectionId {
  return (
    typeof value === "string" &&
    SETTINGS_SECTIONS.some((section) => section.id === value)
  );
}

export function resolveSettingsDestination(value: unknown): SettingsDestination | null {
  if (value === "execution-harnesses" || value === "ideation-harnesses") {
    return { section: "agents" };
  }
  const legacy: Record<string, SettingsDestination> = {
    review: { section: "tasks", tab: "review-policy" },
    autonomy: { section: "tasks", tab: "autonomy-policy" },
    "ideation-workflow": { section: "tasks", tab: "general" },
    "workspace-review": { section: "workspace", tab: "review" },
    execution: { section: "workspace", tab: "general" },
    "global-execution": { section: "capacity" },
  };
  if (typeof value === "string" && legacy[value]) {
    return legacy[value];
  }
  return isSettingsSectionId(value) ? { section: value } : null;
}

export function resolveSettingsSectionId(value: unknown): SettingsSectionId | null {
  return resolveSettingsDestination(value)?.section ?? null;
}
