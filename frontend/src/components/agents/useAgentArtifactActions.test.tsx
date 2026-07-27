import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useAgentArtifactActions } from "./useAgentArtifactActions";

describe("useAgentArtifactActions", () => {
  it("selects the Plan tab without treating it as a pane close toggle", () => {
    const openArtifactTab = vi.fn();
    const onPublishSubTabRequest = vi.fn();
    const scheduleArtifactPanePreload = vi.fn();

    const { result } = renderHook(() =>
      useAgentArtifactActions({
        openArtifactTab,
        onPublishSubTabRequest,
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
    const onPublishSubTabRequest = vi.fn();
    const scheduleArtifactPanePreload = vi.fn();

    const { result } = renderHook(() =>
      useAgentArtifactActions({
        openArtifactTab,
        onPublishSubTabRequest,
        scheduleArtifactPanePreload,
        selectedConversationId: null,
      }),
    );

    act(() => {
      result.current.handleSelectArtifact("plan");
    });

    expect(openArtifactTab).not.toHaveBeenCalled();
    expect(onPublishSubTabRequest).not.toHaveBeenCalled();
  });

  it("opens Commit & Publish with an explicit internal destination", () => {
    const openArtifactTab = vi.fn();
    const onPublishSubTabRequest = vi.fn();

    const { result } = renderHook(() =>
      useAgentArtifactActions({
        onPublishSubTabRequest,
        openArtifactTab,
        scheduleArtifactPanePreload: vi.fn(),
        selectedConversationId: "conversation-1",
      }),
    );

    act(() => {
      result.current.handleOpenPublishPane("review");
    });

    expect(onPublishSubTabRequest).toHaveBeenCalledWith(
      "conversation-1",
      "review",
    );
    expect(openArtifactTab).toHaveBeenCalledWith("conversation-1", "publish");
  });

  it.each(["history", "automation"] as const)(
    "requests %s before opening Commit & Publish",
    (destination) => {
      const callOrder: string[] = [];
      const openArtifactTab = vi.fn(() => callOrder.push("open"));
      const onPublishSubTabRequest = vi.fn(() => callOrder.push("request"));

      const { result } = renderHook(() =>
        useAgentArtifactActions({
          onPublishSubTabRequest,
          openArtifactTab,
          scheduleArtifactPanePreload: vi.fn(),
          selectedConversationId: "conversation-1",
        }),
      );

      act(() => {
        result.current.handleOpenPublishPane(destination);
      });

      expect(onPublishSubTabRequest).toHaveBeenCalledWith(
        "conversation-1",
        destination,
      );
      expect(openArtifactTab).toHaveBeenCalledWith(
        "conversation-1",
        "publish",
      );
      expect(callOrder).toEqual(["request", "open"]);
    },
  );
});
