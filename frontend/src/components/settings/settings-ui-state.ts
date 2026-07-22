/**
 * Persistence helpers for settings dialog UI state.
 *
 * Wraps localStorage so the user returns to the same section, tab,
 * and collapse state across app restarts.
 */

import {
  DEFAULT_SETTINGS_SECTION,
  resolveSettingsDestination,
  type SettingsDestination,
  type SettingsSectionId,
} from "./settings-registry";

const ACTIVE_SECTION_KEY = "ralphx-settings-active-section";
const ACTIVE_SECTION_VERSION_KEY = "ralphx-settings-active-section-version";
const AGENTS_STATE_KEY = "ralphx-settings-agents-state";
const SETTINGS_ACTIVE_SECTION_VERSION = 4;
const AGENTS_STATE_VERSION = 1;

export type AgentsTabValue = "global" | "project";
export type AgentsDisclosureScope = "global" | `project:${string}`;

export interface AgentsDisclosure {
  families: Record<string, boolean>;
  roles: Record<string, boolean>;
}

interface AgentsSettingsState {
  version: typeof AGENTS_STATE_VERSION;
  activeTab: AgentsTabValue;
  disclosures: Record<string, AgentsDisclosure>;
}

function safeGet(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function safeSet(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* ignore write errors */
  }
}

function safeRemove(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    /* ignore write errors */
  }
}

function emptyAgentsState(): AgentsSettingsState {
  return {
    version: AGENTS_STATE_VERSION,
    activeTab: "global",
    disclosures: {},
  };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseBooleanRecord(value: unknown): Record<string, boolean> | null {
  if (!isRecord(value)) return null;
  const result: Record<string, boolean> = {};
  for (const [key, candidate] of Object.entries(value)) {
    if (typeof candidate !== "boolean") return null;
    result[key] = candidate;
  }
  return result;
}

function parseAgentsDisclosure(value: unknown): AgentsDisclosure | null {
  if (!isRecord(value)) return null;
  const families = parseBooleanRecord(value.families);
  const roles = parseBooleanRecord(value.roles);
  return families && roles ? { families, roles } : null;
}

function loadAgentsState(): AgentsSettingsState {
  const raw = safeGet(AGENTS_STATE_KEY);
  if (!raw) return emptyAgentsState();
  try {
    const candidate: unknown = JSON.parse(raw);
    if (
      !isRecord(candidate) ||
      candidate.version !== AGENTS_STATE_VERSION ||
      (candidate.activeTab !== "global" && candidate.activeTab !== "project") ||
      !isRecord(candidate.disclosures)
    ) {
      return emptyAgentsState();
    }
    const disclosures: Record<string, AgentsDisclosure> = {};
    for (const [scope, disclosure] of Object.entries(candidate.disclosures)) {
      const parsed = parseAgentsDisclosure(disclosure);
      if (!parsed) return emptyAgentsState();
      disclosures[scope] = parsed;
    }
    return {
      version: AGENTS_STATE_VERSION,
      activeTab: candidate.activeTab,
      disclosures,
    };
  } catch {
    return emptyAgentsState();
  }
}

function saveAgentsState(state: AgentsSettingsState): void {
  safeSet(AGENTS_STATE_KEY, JSON.stringify(state));
}

export function agentsDisclosureScope(
  projectId: string | null,
): AgentsDisclosureScope {
  return projectId ? `project:${projectId}` : "global";
}

export function loadAgentsTab(hasActiveProject: boolean): AgentsTabValue {
  const saved = loadAgentsState().activeTab;
  return saved === "project" && !hasActiveProject ? "global" : saved;
}

export function saveAgentsTab(tab: AgentsTabValue): void {
  saveAgentsState({ ...loadAgentsState(), activeTab: tab });
}

export function loadAgentsDisclosure(
  scope: AgentsDisclosureScope,
): AgentsDisclosure {
  const disclosure = loadAgentsState().disclosures[scope];
  return disclosure
    ? { families: { ...disclosure.families }, roles: { ...disclosure.roles } }
    : { families: {}, roles: {} };
}

export function loadAgentsDisclosures(): Record<string, AgentsDisclosure> {
  return Object.fromEntries(
    Object.entries(loadAgentsState().disclosures).map(([scope, disclosure]) => [
      scope,
      {
        families: { ...disclosure.families },
        roles: { ...disclosure.roles },
      },
    ]),
  );
}

function saveAgentsDisclosure(
  scope: AgentsDisclosureScope,
  disclosure: AgentsDisclosure,
): void {
  const state = loadAgentsState();
  saveAgentsState({
    ...state,
    disclosures: { ...state.disclosures, [scope]: disclosure },
  });
}

export function saveAgentsFamilyExpanded(
  scope: AgentsDisclosureScope,
  family: string,
  expanded: boolean,
): void {
  const disclosure = loadAgentsDisclosure(scope);
  saveAgentsDisclosure(scope, {
    ...disclosure,
    families: { ...disclosure.families, [family]: expanded },
  });
}

export function saveAgentsFamiliesExpanded(
  scope: AgentsDisclosureScope,
  families: readonly string[],
  expanded: boolean,
): void {
  const disclosure = loadAgentsDisclosure(scope);
  saveAgentsDisclosure(scope, {
    ...disclosure,
    families: {
      ...disclosure.families,
      ...Object.fromEntries(families.map((family) => [family, expanded])),
    },
  });
}

export function saveAgentsRoleExpanded(
  scope: AgentsDisclosureScope,
  role: string,
  expanded: boolean,
): void {
  const disclosure = loadAgentsDisclosure(scope);
  saveAgentsDisclosure(scope, {
    ...disclosure,
    roles: { ...disclosure.roles, [role]: expanded },
  });
}

function loadActiveSectionVersion(): number {
  const raw = safeGet(ACTIVE_SECTION_VERSION_KEY);
  const parsed = raw ? Number.parseInt(raw, 10) : 0;
  return Number.isFinite(parsed) ? parsed : 0;
}

export function migrateActiveSectionPreference(
  raw: string | null,
  version: number,
): SettingsDestination | null {
  const saved = resolveSettingsDestination(raw);
  if (version >= SETTINGS_ACTIVE_SECTION_VERSION) {
    return saved;
  }
  return saved ?? { section: DEFAULT_SETTINGS_SECTION };
}

export function migrateSettingsUiState(): void {
  const version = loadActiveSectionVersion();
  if (version >= SETTINGS_ACTIVE_SECTION_VERSION) {
    return;
  }

  const migrated = migrateActiveSectionPreference(
    safeGet(ACTIVE_SECTION_KEY),
    version,
  );
  if (migrated) {
    safeSet(ACTIVE_SECTION_KEY, migrated.section);
  } else {
    safeRemove(ACTIVE_SECTION_KEY);
  }
  safeSet(ACTIVE_SECTION_VERSION_KEY, String(SETTINGS_ACTIVE_SECTION_VERSION));
}

export function loadActiveSection(): SettingsSectionId | null {
  return loadActiveDestination()?.section ?? null;
}

export function loadActiveDestination(): SettingsDestination | null {
  const version = loadActiveSectionVersion();
  const raw = safeGet(ACTIVE_SECTION_KEY);
  return migrateActiveSectionPreference(raw, version);
}

export function saveActiveSection(section: SettingsSectionId): void {
  safeSet(ACTIVE_SECTION_KEY, section);
  safeSet(ACTIVE_SECTION_VERSION_KEY, String(SETTINGS_ACTIVE_SECTION_VERSION));
}
