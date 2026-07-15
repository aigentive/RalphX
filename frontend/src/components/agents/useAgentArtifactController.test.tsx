import { act, fireEvent, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { useAgentSessionStore } from "@/stores/agentSessionStore";

import { useAgentArtifactController } from "./useAgentArtifactController";
import { useAgentArtifactUiStore } from "./agentArtifactUiStore";

const conversationId = "conversation-1";

function optimisticArtifactState() {
  return useAgentArtifactUiStore.getState().artifactByConversationId[conversationId];
}

describe("useAgentArtifactController", () => {
  beforeEach(() => {
    useAgentArtifactUiStore.setState({ artifactByConversationId: {} });
    useAgentSessionStore.setState({ artifactByConversationId: {} });
    useAgentSessionStore.getState().setArtifactState(conversationId, {
      isOpen: true,
      activeTab: "verification",
      taskMode: "graph",
      hiddenTabs: [],
    });
  });

  it("hides the active tab with a nearby fallback and can restore it without activating it", () => {
    const { result } = renderHook(() =>
      useAgentArtifactController({
        hasAutoOpenArtifacts: false,
        selectedConversationId: conversationId,
      }),
    );

    act(() => {
      result.current.hideArtifactTab(conversationId, "verification", [
        "plan",
        "verification",
        "tasks",
      ]);
    });

    expect(optimisticArtifactState()).toMatchObject({
      activeTab: "plan",
      hiddenTabs: ["verification"],
    });

    act(() => {
      result.current.showArtifactTab(conversationId, "verification");
    });

    expect(optimisticArtifactState()).toMatchObject({
      activeTab: "plan",
      hiddenTabs: [],
    });
  });

  it("keeps a hidden tab hidden during level seeding and preserves non-active state when hiding", () => {
    const { result } = renderHook(() =>
      useAgentArtifactController({
        hasAutoOpenArtifacts: false,
        selectedConversationId: conversationId,
      }),
    );

    act(() => {
      result.current.hideArtifactTab(conversationId, "tasks", [
        "plan",
        "verification",
        "tasks",
      ]);
      result.current.seedArtifactTab(conversationId, "tasks");
    });

    expect(optimisticArtifactState()).toMatchObject({
      activeTab: "verification",
      isOpen: true,
      hiddenTabs: ["tasks"],
    });
  });

  it("supports task-mode and keyboard actions while ignoring shortcuts in text inputs", () => {
    const { result } = renderHook(() =>
      useAgentArtifactController({
        hasAutoOpenArtifacts: false,
        selectedConversationId: conversationId,
      }),
    );

    act(() => {
      result.current.setArtifactTaskMode(conversationId, "kanban");
      fireEvent.keyDown(window, { key: "\\", metaKey: true });
      fireEvent.keyDown(window, { key: "2", metaKey: true });
    });

    expect(optimisticArtifactState()).toMatchObject({
      activeTab: "verification",
      isOpen: true,
      taskMode: "kanban",
    });

    const input = document.createElement("input");
    document.body.append(input);
    input.focus();

    act(() => {
      fireEvent.keyDown(window, { key: "1", metaKey: true });
    });

    expect(optimisticArtifactState()?.activeTab).toBe("verification");
    input.remove();
  });
});
