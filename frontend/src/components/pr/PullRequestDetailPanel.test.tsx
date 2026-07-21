import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";

import { PullRequestDetailPanel } from "./PullRequestDetailPanel";

const bodyProps = vi.fn();

vi.mock("./PullRequestDetailBody", () => ({
  PullRequestDetailBody: (props: {
    presentation?: "default" | "agentsWorkspace";
    showRxConversation?: boolean;
  }) => {
    bodyProps(props);
    return <div data-testid="pr-body">body</div>;
  },
}));

vi.mock("@/components/agents/AgentsArtifactEmptyState", () => ({
  EmptyArtifactState: ({ title }: { title: string }) => (
    <div data-testid="pr-empty">{title}</div>
  ),
}));

function workspaceWithPr(): AgentConversationWorkspace {
  return {
    projectId: "project-1",
    conversationId: "conversation-1",
    branchName: "feat/pr-detail",
    publicationPrNumber: 42,
    publicationPrUrl: "https://github.com/acme/app/pull/42",
    publicationPrStatus: "open",
  } as unknown as AgentConversationWorkspace;
}

describe("PullRequestDetailPanel", () => {
  it("hides the redundant RX conversation embed for the workspace PR tab", () => {
    bodyProps.mockClear();

    render(<PullRequestDetailPanel workspace={workspaceWithPr()} />);

    expect(screen.getByTestId("pr-body")).toBeInTheDocument();
    expect(bodyProps).toHaveBeenCalledWith(
      expect.objectContaining({
        presentation: "agentsWorkspace",
        showRxConversation: false,
      }),
    );
  });

  it("renders an empty state when the workspace has no pull request", () => {
    render(<PullRequestDetailPanel workspace={null} />);

    expect(screen.getByTestId("pr-empty")).toBeInTheDocument();
  });
});
