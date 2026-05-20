import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useSyncedAgentProjectFocus } from "./useSyncedAgentProjectFocus";

describe("useSyncedAgentProjectFocus", () => {
  beforeEach(() => {
    useAgentSessionStore.setState({
      focusedProjectId: null,
      selectedProjectId: null,
      selectedConversationId: null,
      expandedProjectIds: {},
    });
  });

  it("expands the route project when there is no stored selection", () => {
    const setFocusedProject = vi.fn();

    renderHook(() => useSyncedAgentProjectFocus("project-1", setFocusedProject));

    expect(setFocusedProject).toHaveBeenCalledWith("project-1");
  });

  it("expands the stored selected project instead of the route project on initial mount", () => {
    useAgentSessionStore.setState({
      selectedProjectId: "project-2",
      selectedConversationId: "conversation-in-project-2",
    });
    const setFocusedProject = vi.fn();

    renderHook(() => useSyncedAgentProjectFocus("project-1", setFocusedProject));

    expect(setFocusedProject).toHaveBeenCalledWith("project-2");
    expect(setFocusedProject).not.toHaveBeenCalledWith("project-1");
  });

  it("uses the route project for subsequent project changes after initial mount", () => {
    useAgentSessionStore.setState({
      selectedProjectId: "project-2",
      selectedConversationId: "conversation-in-project-2",
    });
    const setFocusedProject = vi.fn();

    const { rerender } = renderHook(
      ({ projectId }) => useSyncedAgentProjectFocus(projectId, setFocusedProject),
      { initialProps: { projectId: "project-1" } },
    );

    expect(setFocusedProject).toHaveBeenCalledWith("project-2");
    setFocusedProject.mockClear();

    rerender({ projectId: "project-3" });

    expect(setFocusedProject).toHaveBeenCalledWith("project-3");
  });

  it("does not re-fire when the route project stays the same", () => {
    const setFocusedProject = vi.fn();

    const { rerender } = renderHook(
      ({ projectId }) => useSyncedAgentProjectFocus(projectId, setFocusedProject),
      { initialProps: { projectId: "project-1" } },
    );

    expect(setFocusedProject).toHaveBeenCalledTimes(1);

    rerender({ projectId: "project-1" });

    expect(setFocusedProject).toHaveBeenCalledTimes(1);
  });

  it("falls back to route project when selectedProjectId equals route project", () => {
    useAgentSessionStore.setState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-in-project-1",
    });
    const setFocusedProject = vi.fn();

    renderHook(() => useSyncedAgentProjectFocus("project-1", setFocusedProject));

    expect(setFocusedProject).toHaveBeenCalledWith("project-1");
  });
});
