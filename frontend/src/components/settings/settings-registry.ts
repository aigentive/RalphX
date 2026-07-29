import {
  Bell,
  Blocks,
  Bot,
  Cpu,
  GitBranch,
  SlidersHorizontal,
  Zap,
  type LucideIcon,
} from "lucide-react";

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
  | "connections"
  | "integrations-hub"
  | "integrations"
  | "github"
  | "linear"
  | "clickup"
  | "granola"
  | "mcp"
  | "updates"
  | "accessibility"
  | "notifications";

export interface SettingsSectionMeta {
  id: SettingsSectionId;
  /** Leaf tab label and page-header title. */
  label: string;
  /** Page-header subtitle shown under the title. */
  description: string;
}

export const DEFAULT_SETTINGS_SECTION: SettingsSectionId = "providers";

export const SETTINGS_SECTIONS: SettingsSectionMeta[] = [
  {
    id: "providers",
    label: "Providers",
    description:
      "Validate CLI harnesses, enable agent providers, and choose the default used by new agent lanes.",
  },
  {
    id: "models",
    label: "Models",
    description:
      "Manage provider models and effort compatibility used by Agents and lane settings.",
  },
  {
    id: "mcp",
    label: "MCP",
    description:
      "Inherit provider-native MCP servers and apply RalphX server or tool restrictions.",
  },
  {
    id: "agents",
    label: "Roles",
    description:
      "Configure new-run behavior and backend-owned defaults for every agent role.",
  },
  {
    id: "personas",
    label: "Personas",
    description:
      "Conversation-bound behavior profiles for Project Agent conversations.",
  },
  {
    id: "capabilities",
    label: "Capabilities",
    description:
      "Opt-in Agent conversation capabilities that apply across the app.",
  },
  {
    id: "tasks",
    label: "Tasks",
    description:
      "Configure task enablement, review policy, and autonomous follow-up behavior.",
  },
  {
    id: "planning",
    label: "Planning",
    description: "Configure acceptance-triggered automatic plan verification.",
  },
  {
    id: "workspace",
    label: "Workspace",
    description: "Configure workspace publishing defaults and review behavior.",
  },
  {
    id: "capacity",
    label: "Capacity",
    description:
      "Configure project and global execution and ideation concurrency.",
  },
  {
    id: "repository",
    label: "Repository",
    description: "Version control and GitHub integration.",
  },
  {
    id: "project-analysis",
    label: "Setup & Validation",
    description: "Build system detection and validation commands.",
  },
  {
    id: "integrations-hub",
    label: "Integrations",
    description:
      "Connect RalphX to the issue trackers, note tools, and external clients your team already uses.",
  },
  {
    id: "integrations",
    label: "Atlassian",
    description: "Jira and Confluence context for agent conversations.",
  },
  {
    id: "github",
    label: "GitHub",
    description: "Local GitHub CLI connection.",
  },
  { id: "linear", label: "Linear", description: "Linear issue references." },
  { id: "clickup", label: "ClickUp", description: "ClickUp ticket references." },
  { id: "granola", label: "Granola", description: "Granola note references." },
  {
    id: "api-keys",
    label: "API Keys",
    description:
      "Manage external API keys for accessing RalphX programmatically.",
  },
  {
    id: "external-mcp",
    label: "External MCP",
    description:
      "Configure external MCP server access (restart required to apply).",
  },
  {
    id: "remote-access",
    label: "Remote Access",
    description:
      "Expose this RalphX host to your other devices over an authenticated connection.",
  },
  {
    id: "connections",
    label: "Connections",
    description:
      "Remote environments this client is paired with, and their current status.",
  },
  {
    id: "notifications",
    label: "Notifications",
    description: "Choose how RalphX gets your attention.",
  },
  {
    id: "updates",
    label: "Updates",
    description: "Choose which RalphX release stream to check for updates.",
  },
  {
    id: "accessibility",
    label: "Accessibility",
    description:
      "Theme, motion, and typography preferences that apply across the entire app.",
  },
];

export type SettingsNavId =
  | "models-providers"
  | "agents"
  | "automation"
  | "repository"
  | "integrations"
  | "notifications"
  | "application";

export interface SettingsNavMeta {
  id: SettingsNavId;
  label: string;
  /** Secondary line rendered under the nav label. */
  sublabel: string;
  /** Page-header subtitle for every leaf under this entry. */
  description: string;
  icon: LucideIcon;
  /** Leaf sections rendered as in-page tabs, in display order. */
  leaves: SettingsSectionId[];
  /**
   * Landing leaf that replaces the tab bar with a hub grid. Only the
   * Integrations nav uses one; its remaining leaves are drill-in pages.
   */
  hubLeaf?: SettingsSectionId;
}

export const SETTINGS_NAV: SettingsNavMeta[] = [
  {
    id: "models-providers",
    label: "Models & Providers",
    sublabel: "Harnesses, models, MCP",
    description:
      "Validate CLI harnesses, pick agent models, and control which MCP servers agents inherit.",
    icon: Cpu,
    leaves: ["providers", "models", "mcp"],
  },
  {
    id: "agents",
    label: "Agents",
    sublabel: "Roles, personas, capabilities",
    description:
      "Backend-owned role defaults, conversation personas, and opt-in agent capabilities.",
    icon: Bot,
    leaves: ["agents", "personas", "capabilities"],
  },
  {
    id: "automation",
    label: "Automation",
    sublabel: "Tasks, planning, capacity",
    description:
      "How tasks, plans, workspaces, and concurrency behave when agents run on their own.",
    icon: Zap,
    leaves: ["tasks", "planning", "workspace", "capacity"],
  },
  {
    id: "repository",
    label: "Repository",
    sublabel: "Version control, validation",
    description:
      "Version control defaults and the build/validation commands agents run.",
    icon: GitBranch,
    leaves: ["repository", "project-analysis"],
  },
  {
    id: "integrations",
    label: "Integrations",
    sublabel: "Trackers, notes, external access",
    description:
      "Connect RalphX to the issue trackers, note tools, and external clients your team already uses.",
    icon: Blocks,
    leaves: [
      "integrations-hub",
      "integrations",
      "github",
      "linear",
      "clickup",
      "granola",
      "api-keys",
      "external-mcp",
      "remote-access",
      "connections",
    ],
    hubLeaf: "integrations-hub",
  },
  {
    id: "notifications",
    label: "Notifications",
    sublabel: "Alerts and attention",
    description: "Choose how RalphX gets your attention.",
    icon: Bell,
    leaves: ["notifications"],
  },
  {
    id: "application",
    label: "Application",
    sublabel: "Updates and accessibility",
    description:
      "Release stream, theme, motion, and typography preferences for the whole app.",
    icon: SlidersHorizontal,
    leaves: ["updates", "accessibility"],
  },
];

/** Feature-flag slice the settings nav depends on (subset of FeatureFlags). */
export interface SettingsSectionFlagGates {
  remoteEnvironments?: boolean;
}

/**
 * Section ids that ship dark behind `remoteEnvironments` (§8 flags note): the host
 * pane (PR 1.7) and the client pane (PR 2.5). A set rather than a chain of id
 * comparisons — the next gated section should be one entry here, not a third special
 * case someone can forget to add to the filter.
 */
const REMOTE_ENVIRONMENT_GATED_SECTIONS: ReadonlySet<SettingsSectionId> =
  new Set(["remote-access", "connections"]);

/** True when `section` is visible for the given feature flags. */
export function isSettingsSectionVisible(
  section: SettingsSectionId,
  flags: SettingsSectionFlagGates,
): boolean {
  return (
    !REMOTE_ENVIRONMENT_GATED_SECTIONS.has(section) ||
    flags.remoteEnvironments === true
  );
}

/** Sections visible for the given feature flags. */
export function visibleSettingsSections(
  flags: SettingsSectionFlagGates,
): SettingsSectionMeta[] {
  return SETTINGS_SECTIONS.filter((section) =>
    isSettingsSectionVisible(section.id, flags),
  );
}

/**
 * Nav entries with flag-gated leaves stripped. Nav entries themselves are never
 * hidden — every gated leaf currently lives under Integrations, which always has
 * ungated leaves — but an entry left with no leaves is dropped rather than
 * rendered as a dead rail item.
 */
export function visibleSettingsNav(
  flags: SettingsSectionFlagGates,
): SettingsNavMeta[] {
  return SETTINGS_NAV.map((nav) => ({
    ...nav,
    leaves: nav.leaves.filter((leaf) => isSettingsSectionVisible(leaf, flags)),
  })).filter((nav) => nav.leaves.length > 0);
}

export type SettingsCompositeTab =
  | "general"
  | "review-policy"
  | "autonomy-policy"
  | "review";

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

export function sectionMeta(
  section: SettingsSectionId,
): SettingsSectionMeta | null {
  return SETTINGS_SECTIONS.find((meta) => meta.id === section) ?? null;
}

const DEFAULT_NAV =
  SETTINGS_NAV.find((nav) => nav.leaves.includes(DEFAULT_SETTINGS_SECTION)) ??
  (SETTINGS_NAV[0] as SettingsNavMeta);

/** Nav entry owning `section`; falls back to the default section's nav. */
export function navForSection(section: SettingsSectionId): SettingsNavMeta {
  return SETTINGS_NAV.find((nav) => nav.leaves.includes(section)) ?? DEFAULT_NAV;
}

export function resolveSettingsDestination(
  value: unknown,
): SettingsDestination | null {
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
    // The flat "integrations" nav entry became the Integrations hub; the
    // Atlassian panel it used to open is one card away, or one alias away.
    integrations: { section: "integrations-hub" },
    atlassian: { section: "integrations" },
  };
  if (typeof value === "string" && legacy[value]) {
    return legacy[value];
  }
  return isSettingsSectionId(value) ? { section: value } : null;
}

export function resolveSettingsSectionId(
  value: unknown,
): SettingsSectionId | null {
  return resolveSettingsDestination(value)?.section ?? null;
}
