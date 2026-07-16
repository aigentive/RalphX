import { beforeEach, describe, expect, it } from "vitest";

import { useArtifactSelectionStore } from "./artifactSelectionStore";

describe("artifactSelectionStore", () => {
  beforeEach(() => {
    useArtifactSelectionStore.getState().clearAllSelections();
  });

  it("keeps one in-memory selection per conversation and replaces it atomically", () => {
    const first = {
      sourceType: "artifact" as const,
      sourceKind: "plan" as const,
      sourceId: "plan-v2",
      startLine: 2,
      endLine: 3,
      content: "two\nthree",
    };
    const replacement = {
      sourceType: "ticket" as const,
      sourceKind: "jira" as const,
      sourceId: "10042",
      sourceKey: "RX-42",
      provider: "atlassian" as const,
      startLine: 8,
      endLine: 8,
      content: "replacement",
    };

    useArtifactSelectionStore.getState().setSelection("conversation-1", first);
    useArtifactSelectionStore
      .getState()
      .setSelection("conversation-1", replacement);

    expect(
      useArtifactSelectionStore.getState().selections["conversation-1"],
    ).toEqual(replacement);
    expect(useArtifactSelectionStore.getState().selections["conversation-2"]).toBeUndefined();
  });

  it("clears only the selected conversation", () => {
    const snapshot = {
      sourceType: "ticket" as const,
      sourceKind: "clickup" as const,
      sourceId: "task-1",
      provider: "clickup" as const,
      startLine: 1,
      endLine: 1,
      content: "one",
    };
    const store = useArtifactSelectionStore.getState();
    store.setSelection("conversation-1", snapshot);
    store.setSelection("conversation-2", snapshot);

    store.clearSelection("conversation-1");

    expect(useArtifactSelectionStore.getState().selections["conversation-1"]).toBeUndefined();
    expect(useArtifactSelectionStore.getState().selections["conversation-2"]).toEqual(snapshot);
  });
});
