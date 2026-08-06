/**
 * Durable dismissal store for agent workspace operation toasts (repair/publish),
 * keyed by operation identity so a page reload/app restart does not resurrect
 * a toast the user already dismissed, while a genuinely new operation still
 * gets a fresh one. Follows the direct-localStorage pattern from
 * `stores/uiStore.ts` since this is module-level state consumed outside React.
 */

const STORAGE_KEY = "ralphx-workspace-operation-dismissals";
const MAX_DISMISSAL_AGE_MS = 7 * 24 * 60 * 60 * 1000;

type DismissalMap = Record<string, number>;

let cache: DismissalMap | null = null;
let persistedStorageDisabled = false;

export function agentWorkspaceRepairDismissalKey(
  conversationId: string,
  operationId: string,
  generation: number,
): string {
  return `repair:${conversationId}:${operationId}:${generation}`;
}

export function agentWorkspacePublishDismissalKey(
  conversationId: string,
  attemptKey: string,
): string {
  return `publish:${conversationId}:${attemptKey}`;
}

function parseStoredDismissals(raw: string): DismissalMap {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    return {};
  }
  const result: DismissalMap = {};
  for (const [key, value] of Object.entries(parsed as Record<string, unknown>)) {
    if (typeof value === "number") {
      result[key] = value;
    }
  }
  return result;
}

function readPersistedDismissals(): DismissalMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw === null ? {} : parseStoredDismissals(raw);
  } catch {
    return {};
  }
}

function writePersistedDismissals(map: DismissalMap): void {
  if (persistedStorageDisabled) {
    return;
  }
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(map));
  } catch {
    persistedStorageDisabled = true;
  }
}

function pruneExpired(map: DismissalMap, now: number): DismissalMap {
  const pruned: DismissalMap = {};
  for (const [key, dismissedAtMs] of Object.entries(map)) {
    if (now - dismissedAtMs <= MAX_DISMISSAL_AGE_MS) {
      pruned[key] = dismissedAtMs;
    }
  }
  return pruned;
}

function hydratedCache(): DismissalMap {
  if (cache === null) {
    cache = readPersistedDismissals();
  }
  return cache;
}

export function isAgentWorkspaceOperationDismissed(key: string): boolean {
  const map = hydratedCache();
  const dismissedAtMs = map[key];
  return dismissedAtMs !== undefined && Date.now() - dismissedAtMs <= MAX_DISMISSAL_AGE_MS;
}

export function dismissAgentWorkspaceOperation(key: string): void {
  const now = Date.now();
  const next = pruneExpired(hydratedCache(), now);
  next[key] = now;
  cache = next;
  writePersistedDismissals(next);
}

export function clearAgentWorkspaceOperationDismissal(key: string): void {
  const map = hydratedCache();
  if (!(key in map)) {
    return;
  }
  const next = { ...map };
  delete next[key];
  cache = next;
  writePersistedDismissals(next);
}

/** Clears both the in-memory cache and the persisted entry entirely. */
export function resetAgentWorkspaceOperationDismissalsForTests(): void {
  cache = {};
  persistedStorageDisabled = false;
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* ignore */
  }
}

/**
 * Clears only the in-memory cache, forcing the next read to re-hydrate from
 * localStorage. Simulates an app restart/module reload while leaving
 * persisted data intact.
 */
export function rehydrateAgentWorkspaceOperationDismissalsForTests(): void {
  cache = null;
}
