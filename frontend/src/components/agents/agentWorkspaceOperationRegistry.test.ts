import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  getWatchedAgentWorkspaceOperations,
  hasAgentWorkspaceOperationResult,
  markAgentWorkspaceOperationAnnounced,
  rehydrateAgentWorkspaceOperationRegistryForTests,
  reportAgentWorkspaceOperationResult,
  resetAgentWorkspaceOperationRegistryForTests,
  subscribeWatchedAgentWorkspaceOperations,
  takeAgentWorkspaceOperationResult,
  unwatchAgentWorkspaceOperation,
  watchAgentWorkspaceOperation,
} from "./agentWorkspaceOperationRegistry";

const STORAGE_KEY = "ralphx-workspace-operation-watch";
const TWENTY_FOUR_HOURS_MS = 24 * 60 * 60 * 1000;

function watchObserved(conversationId: string, overrides: Partial<{
  projectId: string | null;
  conversationTitle: string | null;
  startedAtMs: number | null;
  publishAttemptKey: string | undefined;
}> = {}): void {
  watchAgentWorkspaceOperation({
    conversationId,
    projectId: overrides.projectId ?? "project-1",
    conversationTitle: overrides.conversationTitle ?? "Conversation",
    kind: "observed",
    startedAtMs: overrides.startedAtMs ?? null,
    publishAttemptKey: overrides.publishAttemptKey,
  });
}

describe("agentWorkspaceOperationRegistry", () => {
  beforeEach(() => {
    resetAgentWorkspaceOperationRegistryForTests();
  });

  afterEach(() => {
    vi.useRealTimers();
    resetAgentWorkspaceOperationRegistryForTests();
  });

  it("watches a conversation and includes it in the snapshot, then unwatch removes it", () => {
    watchObserved("c1");

    const snapshot = getWatchedAgentWorkspaceOperations();
    expect(snapshot).toHaveLength(1);
    expect(snapshot[0]?.conversationId).toBe("c1");
    expect(snapshot[0]?.kind).toBe("observed");

    unwatchAgentWorkspaceOperation("c1");
    expect(getWatchedAgentWorkspaceOperations()).toHaveLength(0);
  });

  it("returns a referentially stable array when nothing has changed", () => {
    watchObserved("c1");

    const first = getWatchedAgentWorkspaceOperations();
    const second = getWatchedAgentWorkspaceOperations();

    expect(first).toBe(second);
  });

  it("treats a repeated 'observed' watch as a no-op that neither notifies nor changes the snapshot reference", () => {
    watchObserved("c1");
    const before = getWatchedAgentWorkspaceOperations();
    const listener = vi.fn();
    const unsubscribe = subscribeWatchedAgentWorkspaceOperations(listener);

    watchObserved("c1", { conversationTitle: "Renamed" });

    expect(listener).not.toHaveBeenCalled();
    const after = getWatchedAgentWorkspaceOperations();
    expect(after).toBe(before);
    expect(after[0]?.conversationTitle).toBe("Conversation");

    unsubscribe();
  });

  it("replaces an existing entry on a 'publish' watch, minting a new key and resetting announcedStateKey", () => {
    watchObserved("c1");
    markAgentWorkspaceOperationAnnounced("c1", "state-a");
    const observedKey = getWatchedAgentWorkspaceOperations()[0]?.publishAttemptKey;

    watchAgentWorkspaceOperation({
      conversationId: "c1",
      projectId: "project-1",
      conversationTitle: "Conversation",
      kind: "publish",
      startedAtMs: 1000,
      publishAttemptKey: "publish-attempt-1",
    });

    const snapshot = getWatchedAgentWorkspaceOperations();
    expect(snapshot).toHaveLength(1);
    expect(snapshot[0]?.kind).toBe("publish");
    expect(snapshot[0]?.publishAttemptKey).toBe("publish-attempt-1");
    expect(snapshot[0]?.publishAttemptKey).not.toBe(observedKey);
    expect(snapshot[0]?.announcedStateKey).toBeNull();
  });

  it("keeps publishAttemptKey unchanged across later 'observed' watches", () => {
    watchAgentWorkspaceOperation({
      conversationId: "c1",
      projectId: "project-1",
      conversationTitle: "Conversation",
      kind: "publish",
      startedAtMs: 1000,
      publishAttemptKey: "publish-attempt-1",
    });

    watchObserved("c1");
    watchObserved("c1");

    expect(getWatchedAgentWorkspaceOperations()[0]?.publishAttemptKey).toBe("publish-attempt-1");
  });

  it("honours the supplied publishAttemptKey, else falls back to String(startedAtMs), else Date.now", () => {
    watchAgentWorkspaceOperation({
      conversationId: "supplied",
      projectId: null,
      conversationTitle: null,
      kind: "publish",
      startedAtMs: 42,
      publishAttemptKey: "explicit-key",
    });
    expect(
      getWatchedAgentWorkspaceOperations().find((e) => e.conversationId === "supplied")
        ?.publishAttemptKey,
    ).toBe("explicit-key");

    watchAgentWorkspaceOperation({
      conversationId: "started-at",
      projectId: null,
      conversationTitle: null,
      kind: "publish",
      startedAtMs: 777,
    });
    expect(
      getWatchedAgentWorkspaceOperations().find((e) => e.conversationId === "started-at")
        ?.publishAttemptKey,
    ).toBe("777");

    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00.000Z"));
    watchAgentWorkspaceOperation({
      conversationId: "fallback",
      projectId: null,
      conversationTitle: null,
      kind: "publish",
      startedAtMs: null,
    });
    expect(
      getWatchedAgentWorkspaceOperations().find((e) => e.conversationId === "fallback")
        ?.publishAttemptKey,
    ).toBe(String(Date.now()));
  });

  it("persists and notifies once on markAgentWorkspaceOperationAnnounced; re-marking the same key does not notify", () => {
    watchObserved("c1");
    const listener = vi.fn();
    const unsubscribe = subscribeWatchedAgentWorkspaceOperations(listener);

    markAgentWorkspaceOperationAnnounced("c1", "state-a");
    expect(listener).toHaveBeenCalledTimes(1);
    expect(getWatchedAgentWorkspaceOperations()[0]?.announcedStateKey).toBe("state-a");

    markAgentWorkspaceOperationAnnounced("c1", "state-a");
    expect(listener).toHaveBeenCalledTimes(1);

    unsubscribe();
  });

  it("survives a simulated app restart: watch + announce persist across cache-only rehydration", () => {
    watchObserved("c1");
    markAgentWorkspaceOperationAnnounced("c1", "state-a");

    rehydrateAgentWorkspaceOperationRegistryForTests();

    const snapshot = getWatchedAgentWorkspaceOperations();
    expect(snapshot).toHaveLength(1);
    expect(snapshot[0]?.conversationId).toBe("c1");
    expect(snapshot[0]?.announcedStateKey).toBe("state-a");
  });

  it("prunes entries older than 24h", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00.000Z"));
    watchObserved("old");

    vi.setSystemTime(new Date(Date.now() + TWENTY_FOUR_HOURS_MS + 1));
    watchObserved("fresh");

    const snapshot = getWatchedAgentWorkspaceOperations();
    expect(snapshot.map((e) => e.conversationId)).not.toContain("old");
    expect(snapshot.map((e) => e.conversationId)).toContain("fresh");

    const persisted = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "[]") as Array<{
      conversationId: string;
    }>;
    expect(persisted.map((e) => e.conversationId)).not.toContain("old");
  });

  it("is a one-shot result mailbox that is not persisted across a rehydrate", () => {
    reportAgentWorkspaceOperationResult("c1", { kind: "no_changes" });

    expect(takeAgentWorkspaceOperationResult("c1")).toEqual({ kind: "no_changes" });
    expect(takeAgentWorkspaceOperationResult("c1")).toBeNull();

    reportAgentWorkspaceOperationResult("c2", { kind: "ready" });
    rehydrateAgentWorkspaceOperationRegistryForTests();
    expect(takeAgentWorkspaceOperationResult("c2")).toBeNull();
  });

  it("wakes subscribers with a fresh snapshot reference when a result is reported for a watched conversation", () => {
    watchObserved("c1");
    const before = getWatchedAgentWorkspaceOperations();
    const listener = vi.fn();
    const unsubscribe = subscribeWatchedAgentWorkspaceOperations(listener);

    reportAgentWorkspaceOperationResult("c1", { kind: "no_changes" });

    expect(listener).toHaveBeenCalledTimes(1);
    const after = getWatchedAgentWorkspaceOperations();
    expect(after).not.toBe(before);
    expect(after).toEqual(before);

    unsubscribe();
  });

  it("does not wake subscribers when a result is reported for an unwatched conversation", () => {
    watchObserved("c1");
    const listener = vi.fn();
    const unsubscribe = subscribeWatchedAgentWorkspaceOperations(listener);

    reportAgentWorkspaceOperationResult("other", { kind: "no_changes" });

    expect(listener).not.toHaveBeenCalled();

    unsubscribe();
  });

  it("hasAgentWorkspaceOperationResult peeks the mailbox without consuming it", () => {
    expect(hasAgentWorkspaceOperationResult("c1")).toBe(false);

    reportAgentWorkspaceOperationResult("c1", { kind: "no_changes" });
    expect(hasAgentWorkspaceOperationResult("c1")).toBe(true);
    expect(hasAgentWorkspaceOperationResult("c1")).toBe(true);

    expect(takeAgentWorkspaceOperationResult("c1")).toEqual({ kind: "no_changes" });
    expect(hasAgentWorkspaceOperationResult("c1")).toBe(false);
  });

  it("unwatchAgentWorkspaceOperation clears a pending mailbox entry so it cannot be announced later against an unrelated watch", () => {
    watchObserved("c1");
    reportAgentWorkspaceOperationResult("c1", { kind: "no_changes" });

    unwatchAgentWorkspaceOperation("c1");

    expect(hasAgentWorkspaceOperationResult("c1")).toBe(false);
    expect(takeAgentWorkspaceOperationResult("c1")).toBeNull();
  });

  it("tolerates corrupt JSON in localStorage without throwing", () => {
    localStorage.setItem(STORAGE_KEY, "{not-valid-json");
    rehydrateAgentWorkspaceOperationRegistryForTests();

    expect(() => getWatchedAgentWorkspaceOperations()).not.toThrow();
    expect(getWatchedAgentWorkspaceOperations()).toHaveLength(0);
  });

  it("degrades to in-memory-only when localStorage.setItem throws", () => {
    const setItemSpy = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("QuotaExceededError");
      });

    expect(() => watchObserved("c1")).not.toThrow();
    expect(getWatchedAgentWorkspaceOperations()).toHaveLength(1);

    setItemSpy.mockRestore();
  });

  it("tolerates localStorage.getItem throwing without throwing", () => {
    const getItemSpy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    rehydrateAgentWorkspaceOperationRegistryForTests();

    expect(() => getWatchedAgentWorkspaceOperations()).not.toThrow();
    expect(getWatchedAgentWorkspaceOperations()).toHaveLength(0);

    getItemSpy.mockRestore();
  });
});
