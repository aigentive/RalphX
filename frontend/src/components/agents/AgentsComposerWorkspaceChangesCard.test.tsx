import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";

import { AgentsComposerWorkspaceChangesCard } from "./AgentsComposerWorkspaceChangesCard";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { agentConversationRuntimeIndexKeys } from "./useAgentConversationRuntimeIndex";
import { conversationWorkspaceFixture } from "./agentsTestFixtures";

function renderCard({
  conversationId = "conversation-1",
  withChanges = false,
}: {
  conversationId?: string;
  withChanges?: boolean;
} = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  queryClient.setQueryData(agentConversationRuntimeIndexKeys.detail(conversationId), {
    conversationId,
    rows: [],
  });
  if (withChanges) {
    queryClient.setQueryData(agentWorkspaceKeys.changeSummary(conversationId), {
      supportsWorktreeModes: true,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 1, additions: 2, deletions: 1 },
    });
  }

  const viewCallbacks = {
    onViewWorkspace: vi.fn(),
    onViewIdeation: vi.fn(),
    onViewWorkspaceReview: vi.fn(),
    onViewVerification: vi.fn(),
    onViewTaskRuntime: vi.fn(),
    onOpenFile: vi.fn(),
    onPreloadPublishPane: vi.fn(),
  };

  const result = render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <AgentsComposerWorkspaceChangesCard
          conversationId={conversationId}
          projectId="project-1"
          workspace={
            conversationId
              ? conversationWorkspaceFixture({ conversationId })
              : null
          }
          isFocusedChildChat={false}
          currentFocus={{ type: "workspace" }}
          taskLedgerContext={null}
          {...viewCallbacks}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );

  return { ...result, queryClient, viewCallbacks };
}

describe("AgentsComposerWorkspaceChangesCard", () => {
  it("renders nothing without any composer context", () => {
    renderCard({ conversationId: "" });

    expect(
      screen.queryByTestId("agents-composer-context-tray"),
    ).not.toBeInTheDocument();
  });

  it("closes the active changes panel when the change summary becomes empty", async () => {
    const { queryClient } = renderCard({ withChanges: true });

    const changesToggle = await screen.findByTestId(
      "diff-filter-trigger",
      undefined,
      { timeout: 3_000 },
    );
    fireEvent.click(changesToggle);
    expect(screen.getByTestId("agents-composer-context-tray-body")).toBeInTheDocument();

    act(() => {
      queryClient.setQueryData(agentWorkspaceKeys.changeSummary("conversation-1"), {
        supportsWorktreeModes: true,
        staged: { fileCount: 0, additions: 0, deletions: 0 },
        unstaged: { fileCount: 0, additions: 0, deletions: 0 },
      });
    });

    await waitFor(() =>
      expect(
        screen.queryByTestId("agents-composer-context-tray-body"),
      ).not.toBeInTheDocument(),
    );
  });
});
