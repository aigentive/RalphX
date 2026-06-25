import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useProjectStore } from "@/stores/projectStore";

import { useAgentsSessionBindings } from "./useAgentsSessionBindings";

describe("useAgentsSessionBindings", () => {
  beforeEach(() => {
    useAgentSessionStore.setState({
      focusedProjectId: null,
      selectedProjectId: null,
      selectedConversationId: null,
      lastSelectedConversationByProjectId: {},
      expandedProjectIds: {},
    });
    useProjectStore.setState({
      activeProjectId: "project-b",
      projects: {},
    });
  });

  it("sets the RX active project when selecting an Agents conversation", () => {
    const setOptimisticSelectedConversationId = vi.fn();
    const { result } = renderHook(() =>
      useAgentsSessionBindings({ setOptimisticSelectedConversationId })
    );

    act(() => {
      result.current.selectConversation("project-a", "conversation-a");
    });

    expect(useProjectStore.getState().activeProjectId).toBe("project-a");
    expect(useAgentSessionStore.getState().selectedProjectId).toBe("project-a");
    expect(useAgentSessionStore.getState().selectedConversationId).toBe(
      "conversation-a"
    );
  });
});
