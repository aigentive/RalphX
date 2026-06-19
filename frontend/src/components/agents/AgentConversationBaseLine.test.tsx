import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AgentConversationBaseLine } from "./AgentConversationBaseLine";
import { conversationWorkspaceFixture } from "./agentsTestFixtures";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("AgentConversationBaseLine", () => {
  it("allows callers to override the base picker prefix label", () => {
    render(
      <AgentConversationBaseLine
        workspace={conversationWorkspaceFixture()}
        editable
        prefixLabel="BASE:"
      />,
    );

    const picker = screen.getByTestId("agents-conversation-base-picker");
    expect(picker).toHaveTextContent("BASE:");
    expect(picker).toHaveTextContent("Project default (main)");
    expect(picker).toHaveAccessibleName("Change workspace base branch");
  });
});
