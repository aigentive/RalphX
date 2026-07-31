/**
 * Static search index for the settings dialog.
 *
 * Entries describe settings that actually exist in the app and point at the
 * destination the nav rail would produce, so a search hit and a nav click end
 * up in exactly the same state. Pure data plus a filter — no fetching.
 */

import {
  navForSection,
  sectionMeta,
  type SettingsCompositeTab,
  type SettingsSectionId,
} from "./settings-registry";

export interface SettingsSearchEntry {
  /** User-visible setting or page name. */
  label: string;
  section: SettingsSectionId;
  tab?: SettingsCompositeTab;
  /** Extra terms that should match this entry. */
  keywords: string[];
}

export interface SettingsSearchResult extends SettingsSearchEntry {
  /** "Models & Providers / Providers" breadcrumb for the result row. */
  hint: string;
}

export const SETTINGS_SEARCH_MAX_RESULTS = 8;

export const SETTINGS_SEARCH_INDEX: SettingsSearchEntry[] = [
  {
    label: "Providers",
    section: "providers",
    keywords: ["harness", "claude", "codex", "cli", "default provider", "enable"],
  },
  {
    label: "Models",
    section: "models",
    keywords: ["model", "effort", "opus", "sonnet", "gpt", "compatibility"],
  },
  {
    label: "MCP servers",
    section: "mcp",
    keywords: ["mcp", "server", "tool restrictions", "deny", "provider-native"],
  },
  {
    label: "Agent roles",
    section: "agents",
    keywords: ["role", "defaults", "agent", "sandbox", "approval", "new run"],
  },
  {
    label: "Personas",
    section: "personas",
    keywords: ["persona", "behavior profile", "conversation"],
  },
  {
    label: "Agent capabilities",
    section: "capabilities",
    keywords: ["autopilot", "capability", "opt-in", "experimental"],
  },
  {
    label: "Task settings",
    section: "tasks",
    tab: "general",
    keywords: ["task", "enablement", "acceptance", "gate"],
  },
  {
    label: "Review policy",
    section: "tasks",
    tab: "review-policy",
    keywords: ["review", "policy", "human review", "approval"],
  },
  {
    label: "Autonomy policy",
    section: "tasks",
    tab: "autonomy-policy",
    keywords: ["autonomy", "follow-up", "autonomous"],
  },
  {
    label: "Plan verification",
    section: "planning",
    keywords: ["plan", "verification", "ideation", "acceptance"],
  },
  {
    label: "Workspace publishing",
    section: "workspace",
    tab: "general",
    keywords: ["publish", "workspace", "branch", "pr", "auto-merge"],
  },
  {
    label: "Workspace review",
    section: "workspace",
    tab: "review",
    keywords: ["workspace review", "auto-merge", "gate", "reviewer"],
  },
  {
    label: "Concurrency limits",
    section: "capacity",
    keywords: ["capacity", "concurrency", "parallel", "global execution"],
  },
  {
    label: "Repository",
    section: "repository",
    keywords: ["git", "branch", "remote", "github", "version control"],
  },
  {
    label: "Setup & Validation",
    section: "project-analysis",
    keywords: ["build", "validation", "commands", "analysis", "detection"],
  },
  {
    label: "Integrations",
    section: "integrations-hub",
    keywords: ["integration", "connect", "tools"],
  },
  {
    label: "Atlassian",
    section: "integrations",
    keywords: ["jira", "confluence", "atlassian", "oauth", "ticket"],
  },
  {
    label: "GitHub connection",
    section: "github",
    keywords: ["github", "gh cli", "auth", "token"],
  },
  { label: "Linear", section: "linear", keywords: ["linear", "issue"] },
  { label: "ClickUp", section: "clickup", keywords: ["clickup", "task"] },
  { label: "Granola", section: "granola", keywords: ["granola", "notes"] },
  {
    label: "API keys",
    section: "api-keys",
    keywords: ["api key", "token", "external", "permissions"],
  },
  {
    label: "External MCP access",
    section: "external-mcp",
    keywords: ["external mcp", "http", "bearer", "port"],
  },
  {
    label: "Notifications",
    section: "notifications",
    keywords: ["notification", "alert", "sound", "badge"],
  },
  {
    label: "Updates",
    section: "updates",
    keywords: ["update", "release", "channel", "stable", "beta"],
  },
  {
    label: "Database maintenance",
    section: "database",
    keywords: ["compact", "vacuum", "size", "reclaim", "storage", "sqlite"],
  },
  {
    label: "Theme",
    section: "accessibility",
    keywords: ["theme", "dark", "light", "high contrast", "appearance"],
  },
  {
    label: "Motion & typography",
    section: "accessibility",
    keywords: ["motion", "reduce motion", "font", "text size", "accessibility"],
  },
];

function hintFor(entry: SettingsSearchEntry): string {
  const nav = navForSection(entry.section);
  const leaf = sectionMeta(entry.section)?.label;
  return leaf && leaf !== nav.label ? `${nav.label} / ${leaf}` : nav.label;
}

/** Case-insensitive substring match over label and keywords. */
export function searchSettings(query: string): SettingsSearchResult[] {
  const needle = query.trim().toLowerCase();
  if (!needle) {
    return [];
  }
  return SETTINGS_SEARCH_INDEX.filter(
    (entry) =>
      entry.label.toLowerCase().includes(needle) ||
      entry.keywords.some((keyword) => keyword.includes(needle)),
  )
    .slice(0, SETTINGS_SEARCH_MAX_RESULTS)
    .map((entry) => ({ ...entry, hint: hintFor(entry) }));
}
