import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  agentWorkspacePublishDismissalKey,
  agentWorkspaceRepairDismissalKey,
  clearAgentWorkspaceOperationDismissal,
  dismissAgentWorkspaceOperation,
  isAgentWorkspaceOperationDismissed,
  rehydrateAgentWorkspaceOperationDismissalsForTests,
  resetAgentWorkspaceOperationDismissalsForTests,
} from "./agentWorkspaceOperationDismissals";

const STORAGE_KEY = "ralphx-workspace-operation-dismissals";
const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1000;

describe("agentWorkspaceOperationDismissals", () => {
  beforeEach(() => {
    resetAgentWorkspaceOperationDismissalsForTests();
  });

  afterEach(() => {
    vi.useRealTimers();
    resetAgentWorkspaceOperationDismissalsForTests();
  });

  it("builds repair dismissal keys from conversation, operation, and generation", () => {
    expect(agentWorkspaceRepairDismissalKey("c", "o", 3)).toBe("repair:c:o:3");
  });

  it("builds publish dismissal keys from conversation and attempt key", () => {
    expect(agentWorkspacePublishDismissalKey("c", "attempt-1")).toBe("publish:c:attempt-1");
  });

  it("marks a dismissed operation as dismissed, then clears it", () => {
    const key = agentWorkspaceRepairDismissalKey("c", "o", 1);

    expect(isAgentWorkspaceOperationDismissed(key)).toBe(false);

    dismissAgentWorkspaceOperation(key);
    expect(isAgentWorkspaceOperationDismissed(key)).toBe(true);

    clearAgentWorkspaceOperationDismissal(key);
    expect(isAgentWorkspaceOperationDismissed(key)).toBe(false);
  });

  it("survives a simulated module reload (restart) via cache-only rehydration", () => {
    const key = agentWorkspaceRepairDismissalKey("c", "o", 1);

    dismissAgentWorkspaceOperation(key);
    expect(isAgentWorkspaceOperationDismissed(key)).toBe(true);

    // Simulate an app restart: in-memory module cache is gone, but
    // localStorage persists across the reload.
    rehydrateAgentWorkspaceOperationDismissalsForTests();

    expect(isAgentWorkspaceOperationDismissed(key)).toBe(true);
  });

  it("does not treat a different generation of the same operation as dismissed", () => {
    const generationOne = agentWorkspaceRepairDismissalKey("c", "o", 1);
    const generationTwo = agentWorkspaceRepairDismissalKey("c", "o", 2);

    dismissAgentWorkspaceOperation(generationOne);

    expect(isAgentWorkspaceOperationDismissed(generationOne)).toBe(true);
    expect(isAgentWorkspaceOperationDismissed(generationTwo)).toBe(false);
  });

  it("prunes dismissals older than 7 days", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00.000Z"));

    const oldKey = agentWorkspaceRepairDismissalKey("c", "old", 1);
    dismissAgentWorkspaceOperation(oldKey);
    expect(isAgentWorkspaceOperationDismissed(oldKey)).toBe(true);

    vi.setSystemTime(new Date(Date.now() + SEVEN_DAYS_MS + 1));

    expect(isAgentWorkspaceOperationDismissed(oldKey)).toBe(false);

    // A subsequent write should also prune the stale entry out of storage.
    const freshKey = agentWorkspaceRepairDismissalKey("c", "fresh", 1);
    dismissAgentWorkspaceOperation(freshKey);

    const persisted = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Record<
      string,
      number
    >;
    expect(Object.keys(persisted)).not.toContain(oldKey);
    expect(Object.keys(persisted)).toContain(freshKey);
  });

  it("tolerates corrupt JSON in localStorage and behaves as empty", () => {
    localStorage.setItem(STORAGE_KEY, "{not-valid-json");
    rehydrateAgentWorkspaceOperationDismissalsForTests();

    const key = agentWorkspaceRepairDismissalKey("c", "o", 1);
    expect(() => isAgentWorkspaceOperationDismissed(key)).not.toThrow();
    expect(isAgentWorkspaceOperationDismissed(key)).toBe(false);
  });

  it("degrades to in-memory-only when localStorage.setItem throws (e.g. quota exceeded)", () => {
    const setItemSpy = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("QuotaExceededError");
      });

    const key = agentWorkspaceRepairDismissalKey("c", "o", 1);

    expect(() => dismissAgentWorkspaceOperation(key)).not.toThrow();
    expect(isAgentWorkspaceOperationDismissed(key)).toBe(true);

    setItemSpy.mockRestore();
  });
});
