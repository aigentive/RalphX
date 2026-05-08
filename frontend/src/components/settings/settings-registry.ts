export type SettingsSectionId =
  | "execution"
  | "execution-harnesses"
  | "models"
  | "global-execution"
  | "review"
  | "repository"
  | "project-analysis"
  | "ideation-workflow"
  | "ideation-harnesses"
  | "api-keys"
  | "external-mcp"
  | "accessibility";

export type SettingsGroupId =
  | "general"
  | "workspace"
  | "ideation"
  | "access"
  | "preferences";

export interface SettingsSectionMeta {
  id: SettingsSectionId;
  label: string;
  groupId: SettingsGroupId;
}

export const SETTINGS_GROUPS: { id: SettingsGroupId; label: string }[] = [
  { id: "workspace", label: "Workspace" },
  { id: "general", label: "General" },
  { id: "ideation", label: "Ideation" },
  { id: "access", label: "Access" },
  { id: "preferences", label: "Preferences" },
];

export const DEFAULT_SETTINGS_SECTION: SettingsSectionId = "repository";

export const SETTINGS_SECTIONS: SettingsSectionMeta[] = [
  { id: "repository", groupId: "workspace", label: "Repository" },
  { id: "project-analysis", groupId: "workspace", label: "Setup & Validation" },
  { id: "execution", groupId: "general", label: "Execution" },
  { id: "execution-harnesses", groupId: "general", label: "Execution Agents" },
  { id: "models", groupId: "general", label: "Models" },
  { id: "global-execution", groupId: "general", label: "Global Capacity" },
  { id: "review", groupId: "general", label: "Review Policy" },
  { id: "ideation-workflow", groupId: "ideation", label: "Planning & Verification" },
  { id: "ideation-harnesses", groupId: "ideation", label: "Ideation Agents" },
  { id: "api-keys", groupId: "access", label: "API Keys" },
  { id: "external-mcp", groupId: "access", label: "External MCP" },
  { id: "accessibility", groupId: "preferences", label: "Accessibility" },
];

export function isSettingsSectionId(value: unknown): value is SettingsSectionId {
  return (
    typeof value === "string" &&
    SETTINGS_SECTIONS.some((section) => section.id === value)
  );
}
