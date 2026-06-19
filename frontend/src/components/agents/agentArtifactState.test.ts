import { describe, expect, it } from "vitest";

import { resolveAgentArtifactState } from "./agentArtifactState";

describe("resolveAgentArtifactState", () => {
  it("prefers ideation tasks over stale persisted external tabs", () => {
    const state = resolveAgentArtifactState({
      optimistic: null,
      persisted: {
        isOpen: true,
        activeTab: "linear",
        taskMode: "kanban",
      },
      hasStored: true,
      hasAutoOpenArtifacts: true,
      availableTabs: ["plan", "verification", "proposal", "tasks"],
    });

    expect(state).toEqual({
      isOpen: true,
      activeTab: "tasks",
      taskMode: "kanban",
    });
  });

  it("keeps optimistic external tabs so explicit user selection still works", () => {
    const state = resolveAgentArtifactState({
      optimistic: {
        isOpen: true,
        activeTab: "linear",
        taskMode: "graph",
      },
      persisted: {
        isOpen: true,
        activeTab: "tasks",
        taskMode: "graph",
      },
      hasStored: true,
      hasAutoOpenArtifacts: true,
      availableTabs: ["plan", "verification", "proposal", "tasks"],
    });

    expect(state.activeTab).toBe("linear");
  });
});
