import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";

import { AgentConversationWorkspaceLine } from "./AgentConversationWorkspaceLine";
import { conversationWorkspaceFixture } from "./agentsTestFixtures";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn(),
}));

function workspace(
  overrides: Partial<AgentConversationWorkspace> = {},
): AgentConversationWorkspace {
  return conversationWorkspaceFixture({
    conversationId: "conversation-1",
    branchName: "ralphx/ralphx/agent-abcdef12",
    publicationPrNumber: 42,
    publicationPrUrl: "https://github.com/ralphx/ralphx/pull/42",
    ...overrides,
  });
}

describe("AgentConversationWorkspaceLine", () => {
  it("shows a capitalized Closed badge for a closed PR", () => {
    render(
      <AgentConversationWorkspaceLine
        workspace={workspace({ publicationPrStatus: "closed" })}
      />,
    );
    expect(
      screen.getByTestId("agents-conversation-branch-line"),
    ).toHaveTextContent("Closed");
  });

  it("shows a capitalized Merged badge for a merged PR", () => {
    render(
      <AgentConversationWorkspaceLine
        workspace={workspace({ publicationPrStatus: "merged" })}
      />,
    );
    expect(
      screen.getByTestId("agents-conversation-branch-line"),
    ).toHaveTextContent("Merged");
  });

  it("clears the terminal badge once status returns to open", () => {
    render(
      <AgentConversationWorkspaceLine
        workspace={workspace({
          publicationPrStatus: "open",
          publicationPushStatus: "pushed",
        })}
      />,
    );
    const branchLine = screen.getByTestId("agents-conversation-branch-line");
    expect(branchLine).not.toHaveTextContent("Closed");
    expect(branchLine).not.toHaveTextContent("Merged");
    expect(branchLine).toHaveTextContent("pushed");
  });
});
