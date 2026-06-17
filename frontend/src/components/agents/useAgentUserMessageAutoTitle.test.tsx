import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { conversationFixture as conversation } from "./agentsTestFixtures";
import { useAgentUserMessageAutoTitle } from "./useAgentUserMessageAutoTitle";

describe("useAgentUserMessageAutoTitle", () => {
  it("forwards the conversation provider harness to auto-managed title handling", () => {
    const handleAutoManagedTitle = vi.fn();
    const foundConversation = conversation({
      id: "conversation-1",
      contextType: "project",
      providerHarness: "codex",
    });

    const { result } = renderHook(() =>
      useAgentUserMessageAutoTitle({
        activeProjectId: "project-1",
        findConversationById: (conversationId) =>
          conversationId === foundConversation.id ? foundConversation : null,
        handleAutoManagedTitle,
        selectedConversationId: null,
      })
    );

    result.current({
      content: "fix session naming",
      result: { conversationId: "conversation-1" },
    });

    expect(handleAutoManagedTitle).toHaveBeenCalledWith({
      content: "fix session naming",
      conversationId: "conversation-1",
      targetProjectId: "project-1",
      shouldSpawnSessionNamer: true,
      providerHarness: "codex",
    });
  });

  it("uses the selected conversation fallback and skips without an active project", () => {
    const handleAutoManagedTitle = vi.fn();
    const { result, rerender } = renderHook(
      ({ activeProjectId }: { activeProjectId: string | null }) =>
        useAgentUserMessageAutoTitle({
          activeProjectId,
          findConversationById: () => null,
          handleAutoManagedTitle,
          selectedConversationId: "selected-conversation",
        }),
      {
        initialProps: { activeProjectId: "project-1" },
      }
    );

    result.current({
      content: "rename selected conversation",
      result: { conversationId: "" },
    });
    expect(handleAutoManagedTitle).toHaveBeenCalledWith({
      content: "rename selected conversation",
      conversationId: "selected-conversation",
      targetProjectId: "project-1",
      shouldSpawnSessionNamer: false,
      providerHarness: null,
    });

    rerender({ activeProjectId: null });
    result.current({
      content: "ignored without project",
      result: { conversationId: "" },
    });
    expect(handleAutoManagedTitle).toHaveBeenCalledTimes(1);
  });
});
