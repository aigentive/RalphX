export type SettingsSectionId =
  | "providers"
  | "agents"
  | "execution"
  | "models"
  | "global-execution"
  | "personas"
  | "capabilities"
  | "workspace-review"
  | "review"
  | "autonomy"
  | "repository"
  | "project-analysis"
  | "ideation-workflow"
  | "api-keys"
  | "integrations"
  | "github"
  | "linear"
  | "clickup"
  | "granola"
  | "mcp"
  | "accessibility"
  | "notifications";

export type SettingsGroupId =
  | "harness"
  | "general"
  | "workspace"
  | "ideation"
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
  { id: "ideation", label: "Ideation" },
  { id: "integrations", label: "Integrations" },
  { id: "access", label: "Access" },
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
  { id: "execution", groupId: "general", label: "Execution" },
  { id: "global-execution", groupId: "general", label: "Global Capacity" },
  { id: "personas", groupId: "general", label: "Personas" },
  { id: "capabilities", groupId: "general", label: "Capabilities" },
  { id: "workspace-review", groupId: "general", label: "Workspace Review" },
  { id: "review", groupId: "general", label: "Review Policy" },
  { id: "autonomy", groupId: "general", label: "Autonomy Policy" },
  { id: "ideation-workflow", groupId: "ideation", label: "Planning & Verification" },
  { id: "integrations", groupId: "integrations", label: "Atlassian" },
  { id: "github", groupId: "integrations", label: "GitHub" },
  { id: "linear", groupId: "integrations", label: "Linear" },
  { id: "clickup", groupId: "integrations", label: "ClickUp" },
  { id: "granola", groupId: "integrations", label: "Granola" },
  { id: "api-keys", groupId: "access", label: "API Keys" },
  { id: "accessibility", groupId: "preferences", label: "Accessibility" },
  { id: "notifications", groupId: "preferences", label: "Notifications" },
];

const TASKS_ONLY_SETTINGS_SECTIONS = new Set<SettingsSectionId>([
  "execution",
  "global-execution",
  "review",
  "workspace-review",
  "autonomy",
]);

export function visibleSettingsSections(tasksEnabled: boolean): SettingsSectionMeta[] {
  return tasksEnabled
    ? SETTINGS_SECTIONS
    : SETTINGS_SECTIONS.filter((section) => !TASKS_ONLY_SETTINGS_SECTIONS.has(section.id));
}

export function isSettingsSectionId(value: unknown): value is SettingsSectionId {
  return (
    typeof value === "string" &&
    SETTINGS_SECTIONS.some((section) => section.id === value)
  );
}

export function resolveSettingsSectionId(value: unknown): SettingsSectionId | null {
  if (value === "execution-harnesses" || value === "ideation-harnesses") {
    return "agents";
  }
  if (value === "external-mcp") {
    return "mcp";
  }
  return isSettingsSectionId(value) ? value : null;
}
