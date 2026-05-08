/**
 * Persistence helpers for settings dialog UI state.
 *
 * Wraps localStorage so the user returns to the same section, tab,
 * and collapse state across app restarts.
 */

import {
  DEFAULT_SETTINGS_SECTION,
  isSettingsSectionId,
  type SettingsSectionId,
} from "./settings-registry";

const ACTIVE_SECTION_KEY = "ralphx-settings-active-section";
const ACTIVE_SECTION_VERSION_KEY = "ralphx-settings-active-section-version";
const HARNESS_TAB_KEY = "ralphx-settings-harness-tab";
const HARNESS_EXPANDED_KEY = "ralphx-settings-harness-expanded";
const SETTINGS_ACTIVE_SECTION_VERSION = 1;
const LEGACY_DEFAULT_ACTIVE_SECTION: SettingsSectionId = "execution";

export type HarnessTabScope = "ideation" | "execution";
export type HarnessTabValue = "global" | "project";

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

function loadActiveSectionVersion(): number {
  const raw = safeGet(ACTIVE_SECTION_VERSION_KEY);
  const parsed = raw ? Number.parseInt(raw, 10) : 0;
  return Number.isFinite(parsed) ? parsed : 0;
}

export function migrateActiveSectionPreference(
  raw: string | null,
  version: number,
): SettingsSectionId | null {
  const saved = isSettingsSectionId(raw) ? raw : null;
  if (version >= SETTINGS_ACTIVE_SECTION_VERSION) {
    return saved;
  }
  if (saved === null || saved === LEGACY_DEFAULT_ACTIVE_SECTION) {
    return DEFAULT_SETTINGS_SECTION;
  }
  return saved;
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
    safeSet(ACTIVE_SECTION_KEY, migrated);
  } else {
    safeRemove(ACTIVE_SECTION_KEY);
  }
  safeSet(ACTIVE_SECTION_VERSION_KEY, String(SETTINGS_ACTIVE_SECTION_VERSION));
}

export function loadActiveSection(): SettingsSectionId | null {
  migrateSettingsUiState();
  const saved = safeGet(ACTIVE_SECTION_KEY);
  return isSettingsSectionId(saved) ? saved : null;
}

export function saveActiveSection(section: SettingsSectionId): void {
  safeSet(ACTIVE_SECTION_KEY, section);
  safeSet(ACTIVE_SECTION_VERSION_KEY, String(SETTINGS_ACTIVE_SECTION_VERSION));
}

function loadHarnessTabMap(): Record<string, HarnessTabValue> {
  try {
    const raw = localStorage.getItem(HARNESS_TAB_KEY);
    return raw ? (JSON.parse(raw) as Record<string, HarnessTabValue>) : {};
  } catch {
    return {};
  }
}

export function loadHarnessTab(scope: HarnessTabScope): HarnessTabValue {
  return loadHarnessTabMap()[scope] ?? "global";
}

export function saveHarnessTab(
  scope: HarnessTabScope,
  tab: HarnessTabValue,
): void {
  try {
    const next = { ...loadHarnessTabMap(), [scope]: tab };
    localStorage.setItem(HARNESS_TAB_KEY, JSON.stringify(next));
  } catch {
    /* ignore write errors */
  }
}

function loadHarnessExpandedMap(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(HARNESS_EXPANDED_KEY);
    return raw ? (JSON.parse(raw) as Record<string, boolean>) : {};
  } catch {
    return {};
  }
}

export function expandedKey(
  tab: HarnessTabValue,
  laneId: string,
): string {
  return `${tab}:${laneId}`;
}

export function loadHarnessExpanded(
  tab: HarnessTabValue,
  laneIds: string[],
): Record<string, boolean> {
  const map = loadHarnessExpandedMap();
  const result: Record<string, boolean> = {};
  for (const laneId of laneIds) {
    const stored = map[expandedKey(tab, laneId)];
    if (stored !== undefined) {
      result[laneId] = stored;
    }
  }
  return result;
}

export function saveHarnessExpanded(
  tab: HarnessTabValue,
  laneId: string,
  expanded: boolean,
): void {
  try {
    const map = loadHarnessExpandedMap();
    map[expandedKey(tab, laneId)] = expanded;
    localStorage.setItem(HARNESS_EXPANDED_KEY, JSON.stringify(map));
  } catch {
    /* ignore write errors */
  }
}

export function saveHarnessExpandedBulk(
  tab: HarnessTabValue,
  laneIds: string[],
  expanded: boolean,
): void {
  try {
    const map = loadHarnessExpandedMap();
    for (const laneId of laneIds) {
      map[expandedKey(tab, laneId)] = expanded;
    }
    localStorage.setItem(HARNESS_EXPANDED_KEY, JSON.stringify(map));
  } catch {
    /* ignore write errors */
  }
}
