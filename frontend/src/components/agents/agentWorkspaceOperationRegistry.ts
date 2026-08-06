/**
 * Durable watch registry for agent workspace operations (publish/base-update/
 * observed), keyed by conversationId. Backs a single global toast driver that
 * needs to know "which conversations currently have a workspace operation
 * worth showing" and must survive an app restart. Follows the direct-
 * localStorage pattern from `agentWorkspaceOperationDismissals.ts` /
 * `stores/uiStore.ts` since this is module-level state consumed outside React,
 * and is shaped for `useSyncExternalStore` per
 * `.claude/rules/data-architecture-patterns.md` → External Store Subscriptions.
 */

import type { AgentWorkspaceOperationResult } from "./agentWorkspacePublishAttempt";

const STORAGE_KEY = "ralphx-workspace-operation-watch";
const MAX_WATCH_AGE_MS = 24 * 60 * 60 * 1000;

export interface WatchedAgentWorkspaceOperation {
  conversationId: string;
  projectId: string | null;
  conversationTitle: string | null;
  addedAtMs: number;
  /** Set when THIS session started the operation. */
  startedAtMs: number | null;
  /** FROZEN at watch time; never recomputed while the entry lives. */
  publishAttemptKey: string;
  /** Last terminal state this app already announced. */
  announcedStateKey: string | null;
  kind: "publish" | "base-update" | "observed";
}

let entriesMap: Map<string, WatchedAgentWorkspaceOperation> | null = null;
let snapshotArray: WatchedAgentWorkspaceOperation[] = [];
let persistedStorageDisabled = false;
const listeners = new Set<() => void>();
const resultMailbox = new Map<string, AgentWorkspaceOperationResult>();

function isValidKind(
  value: unknown,
): value is WatchedAgentWorkspaceOperation["kind"] {
  return value === "publish" || value === "base-update" || value === "observed";
}

function parseWatchedEntry(raw: unknown): WatchedAgentWorkspaceOperation | null {
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    return null;
  }
  const candidate = raw as Record<string, unknown>;
  if (
    typeof candidate.conversationId !== "string" ||
    typeof candidate.addedAtMs !== "number" ||
    typeof candidate.publishAttemptKey !== "string" ||
    !isValidKind(candidate.kind)
  ) {
    return null;
  }
  return {
    conversationId: candidate.conversationId,
    projectId: typeof candidate.projectId === "string" ? candidate.projectId : null,
    conversationTitle:
      typeof candidate.conversationTitle === "string" ? candidate.conversationTitle : null,
    addedAtMs: candidate.addedAtMs,
    startedAtMs: typeof candidate.startedAtMs === "number" ? candidate.startedAtMs : null,
    publishAttemptKey: candidate.publishAttemptKey,
    announcedStateKey:
      typeof candidate.announcedStateKey === "string" ? candidate.announcedStateKey : null,
    kind: candidate.kind,
  };
}

function parseStoredWatches(raw: string): Map<string, WatchedAgentWorkspaceOperation> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return new Map();
  }
  if (!Array.isArray(parsed)) {
    return new Map();
  }
  const now = Date.now();
  const map = new Map<string, WatchedAgentWorkspaceOperation>();
  for (const item of parsed) {
    const entry = parseWatchedEntry(item);
    if (entry && now - entry.addedAtMs <= MAX_WATCH_AGE_MS) {
      map.set(entry.conversationId, entry);
    }
  }
  return map;
}

function readPersistedWatches(): Map<string, WatchedAgentWorkspaceOperation> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw === null ? new Map() : parseStoredWatches(raw);
  } catch {
    return new Map();
  }
}

function writePersistedWatches(map: Map<string, WatchedAgentWorkspaceOperation>): void {
  if (persistedStorageDisabled) {
    return;
  }
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(Array.from(map.values())));
  } catch {
    persistedStorageDisabled = true;
  }
}

function pruneExpired(
  map: Map<string, WatchedAgentWorkspaceOperation>,
  now: number,
): Map<string, WatchedAgentWorkspaceOperation> {
  const pruned = new Map<string, WatchedAgentWorkspaceOperation>();
  for (const [conversationId, entry] of map) {
    if (now - entry.addedAtMs <= MAX_WATCH_AGE_MS) {
      pruned.set(conversationId, entry);
    }
  }
  return pruned;
}

function hydratedMap(): Map<string, WatchedAgentWorkspaceOperation> {
  if (entriesMap === null) {
    entriesMap = readPersistedWatches();
    snapshotArray = Array.from(entriesMap.values());
  }
  return entriesMap;
}

function commit(next: Map<string, WatchedAgentWorkspaceOperation>): void {
  const pruned = pruneExpired(next, Date.now());
  entriesMap = pruned;
  snapshotArray = Array.from(pruned.values());
  writePersistedWatches(pruned);
  for (const listener of listeners) {
    listener();
  }
}

function resolvePublishAttemptKey(
  supplied: string | undefined,
  startedAtMs: number | null,
): string {
  if (supplied !== undefined) {
    return supplied;
  }
  if (startedAtMs !== null) {
    return String(startedAtMs);
  }
  return String(Date.now());
}

export function watchAgentWorkspaceOperation(entry: {
  conversationId: string;
  projectId: string | null;
  conversationTitle: string | null;
  kind: WatchedAgentWorkspaceOperation["kind"];
  startedAtMs: number | null;
  publishAttemptKey?: string | undefined;
}): void {
  const map = hydratedMap();
  if (entry.kind === "observed" && map.has(entry.conversationId)) {
    // Idempotent: the publish panel re-runs its effect on every workspace
    // object identity change, so a repeat "observed" watch must be a
    // complete no-op (no field refresh, no persistence write, no notify).
    return;
  }
  const next = new Map(map);
  next.set(entry.conversationId, {
    conversationId: entry.conversationId,
    projectId: entry.projectId,
    conversationTitle: entry.conversationTitle,
    addedAtMs: Date.now(),
    startedAtMs: entry.startedAtMs,
    publishAttemptKey: resolvePublishAttemptKey(entry.publishAttemptKey, entry.startedAtMs),
    announcedStateKey: null,
    kind: entry.kind,
  });
  commit(next);
}

export function unwatchAgentWorkspaceOperation(conversationId: string): void {
  // Clear any orphaned session result so it can never be announced against a
  // later, unrelated watch of the same conversation.
  resultMailbox.delete(conversationId);
  const map = hydratedMap();
  if (!map.has(conversationId)) {
    return;
  }
  const next = new Map(map);
  next.delete(conversationId);
  commit(next);
}

export function markAgentWorkspaceOperationAnnounced(
  conversationId: string,
  stateKey: string,
): void {
  const map = hydratedMap();
  const existing = map.get(conversationId);
  if (!existing || existing.announcedStateKey === stateKey) {
    return;
  }
  const next = new Map(map);
  next.set(conversationId, { ...existing, announcedStateKey: stateKey });
  commit(next);
}

export function getWatchedAgentWorkspaceOperations(): WatchedAgentWorkspaceOperation[] {
  hydratedMap();
  return snapshotArray;
}

export function subscribeWatchedAgentWorkspaceOperations(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

/** Clears in-memory cache, persisted storage, subscribers, and the result mailbox entirely. */
export function resetAgentWorkspaceOperationRegistryForTests(): void {
  entriesMap = new Map();
  snapshotArray = [];
  persistedStorageDisabled = false;
  listeners.clear();
  resultMailbox.clear();
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* ignore */
  }
}

/**
 * Clears only the in-memory cache (and the session-only result mailbox),
 * forcing the next read to re-hydrate from localStorage. Simulates an app
 * restart/module reload while leaving persisted watch data intact.
 */
export function rehydrateAgentWorkspaceOperationRegistryForTests(): void {
  entriesMap = null;
  snapshotArray = [];
  resultMailbox.clear();
}

export function reportAgentWorkspaceOperationResult(
  conversationId: string,
  result: AgentWorkspaceOperationResult,
): void {
  resultMailbox.set(conversationId, result);
}

export function takeAgentWorkspaceOperationResult(
  conversationId: string,
): AgentWorkspaceOperationResult | null {
  const result = resultMailbox.get(conversationId);
  if (result === undefined) {
    return null;
  }
  resultMailbox.delete(conversationId);
  return result;
}

/** Reads the result mailbox without consuming it. */
export function hasAgentWorkspaceOperationResult(conversationId: string): boolean {
  return resultMailbox.has(conversationId);
}
