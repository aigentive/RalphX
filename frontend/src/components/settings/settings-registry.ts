export type SettingsSectionId =
  | "providers"
  | "execution"
  | "execution-harnesses"
  | "models"
  | "global-execution"
  | "personas"
  | "workspace-review"
  | "review"
  | "autonomy"
  | "repository"
  | "project-analysis"
  | "ideation-workflow"
  | "ideation-harnesses"
  | "api-keys"
  | "integrations"
  | "github"
  | "linear"
  | "clickup"
  | "granola"
  | "external-mcp"
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
  { id: "repository", groupId: "workspace", label: "Repository" },
  { id: "project-analysis", groupId: "workspace", label: "Setup & Validation" },
  { id: "execution", groupId: "general", label: "Execution" },
  { id: "execution-harnesses", groupId: "general", label: "Execution Agents" },
  { id: "global-execution", groupId: "general", label: "Global Capacity" },
  { id: "personas", groupId: "general", label: "Personas" },
  { id: "workspace-review", groupId: "general", label: "Workspace Review" },
  { id: "review", groupId: "general", label: "Review Policy" },
  { id: "autonomy", groupId: "general", label: "Autonomy Policy" },
  { id: "ideation-workflow", groupId: "ideation", label: "Planning & Verification" },
  { id: "ideation-harnesses", groupId: "ideation", label: "Ideation Agents" },
  { id: "integrations", groupId: "integrations", label: "Atlassian" },
  { id: "github", groupId: "integrations", label: "GitHub" },
  { id: "linear", groupId: "integrations", label: "Linear" },
  { id: "clickup", groupId: "integrations", label: "ClickUp" },
  { id: "granola", groupId: "integrations", label: "Granola" },
  { id: "api-keys", groupId: "access", label: "API Keys" },
  { id: "external-mcp", groupId: "access", label: "External MCP" },
  { id: "accessibility", groupId: "preferences", label: "Accessibility" },
  { id: "notifications", groupId: "preferences", label: "Notifications" },
];

export function isSettingsSectionId(value: unknown): value is SettingsSectionId {
  return (
    typeof value === "string" &&
    SETTINGS_SECTIONS.some((section) => section.id === value)
  );
}
