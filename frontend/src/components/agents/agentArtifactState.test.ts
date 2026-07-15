import { beforeEach, describe, expect, it } from "vitest";

import { useAgentSessionStore } from "@/stores/agentSessionStore";

import { seedAgentArtifactTab } from "./agentArtifactState";
import { useAgentArtifactUiStore } from "./agentArtifactUiStore";

describe("seedAgentArtifactTab", () => {
  beforeEach(() => {
    useAgentSessionStore.setState(useAgentSessionStore.getInitialState(), true);
    useAgentArtifactUiStore.setState({ artifactByConversationId: {} });
  });

  it("never unhides a level-seeded tab or invents an unavailable fallback", () => {
    useAgentSessionStore.getState().setArtifactState("conversation-1", {
      isOpen: false,
      activeTab: "automation",
      taskMode: "graph",
      hiddenTabs: ["automation"],
    });

    seedAgentArtifactTab("conversation-1", "automation", false);

    expect(
      useAgentArtifactUiStore.getState().artifactByConversationId["conversation-1"],
    ).toMatchObject({
      isOpen: true,
      activeTab: "automation",
      hiddenTabs: ["automation"],
    });
    expect(
      useAgentSessionStore.getState().artifactByConversationId["conversation-1"]
        ?.hiddenTabs,
    ).toEqual(["automation"]);
  });

  it("activates a visible seeded tab without changing hidden preferences", () => {
    useAgentSessionStore.getState().setArtifactState("conversation-1", {
      isOpen: false,
      activeTab: "plan",
      taskMode: "graph",
      hiddenTabs: ["jira"],
    });

    seedAgentArtifactTab("conversation-1", "automation", false);

    expect(
      useAgentArtifactUiStore.getState().artifactByConversationId["conversation-1"],
    ).toMatchObject({
      activeTab: "automation",
      hiddenTabs: ["jira"],
    });
  });
});
