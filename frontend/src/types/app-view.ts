import { z } from "zod";

/** Root navigation surfaces that can be rendered by the application shell. */
export const APP_VIEW_VALUES = [
  "agents",
  "automations",
  "ticketing",
  "github",
  "granola",
  "extensibility",
  "activity",
  "insights",
] as const;

export const AppViewSchema = z.enum(APP_VIEW_VALUES);
export type AppView = z.infer<typeof AppViewSchema>;

export const DEFAULT_APP_VIEW: AppView = "agents";

export interface ParsedPersistedViewByProject {
  map: Record<string, AppView>;
  changed: boolean;
}

/**
 * Parses the untrusted persisted route map once at the storage boundary.
 * Removed roots and malformed values intentionally resolve to Agents.
 */
export function parsePersistedViewByProject(
  value: unknown,
): ParsedPersistedViewByProject {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return { map: {}, changed: true };
  }

  const map: Record<string, AppView> = {};
  const source = value as Record<string, unknown>;
  for (const [projectId, rawView] of Object.entries(source)) {
    if (projectId.trim().length === 0) {
      continue;
    }
    const parsedView = AppViewSchema.safeParse(rawView);
    map[projectId] = parsedView.success ? parsedView.data : DEFAULT_APP_VIEW;
  }

  return {
    map,
    changed: JSON.stringify(map) !== JSON.stringify(value),
  };
}
