import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useAgentArtifactActions } from "./useAgentArtifactActions";

describe("useAgentArtifactActions", () => {
  it("selects the Plan tab without treating it as a pane close toggle", () => {
    const openArtifactTab = vi.fn();
    const scheduleArtifactPanePreload = vi.fn();

    const { result } = renderHook(() =>
      useAgentArtifactActions({
        openArtifactTab,
        scheduleArtifactPanePreload,
        selectedConversationId: "conversation-1",
      }),
    );

    act(() => {
      result.current.handleSelectArtifact("plan");
    });

    expect(openArtifactTab).toHaveBeenCalledWith("conversation-1", "plan");
  });

  it("does nothing when no conversation is selected", () => {
    const openArtifactTab = vi.fn();
    const scheduleArtifactPanePreload = vi.fn();

    const { result } = renderHook(() =>
      useAgentArtifactActions({
        openArtifactTab,
        scheduleArtifactPanePreload,
        selectedConversationId: null,
      }),
    );

    act(() => {
      result.current.handleSelectArtifact("plan");
    });

    expect(openArtifactTab).not.toHaveBeenCalled();
  });
});
