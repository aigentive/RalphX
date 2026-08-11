import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";

import { TooltipProvider } from "@/components/ui/tooltip";
import {
  chatApi,
  type AgentConversationWorkspace,
  type ReopenAgentWorkspacePrResult,
} from "@/api/chat";

import { AgentPublishPanel } from "./AgentsPublishPanel";
import { conversationWorkspaceFixture } from "./agentsTestFixtures";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";

const { openUrlMock } = vi.hoisted(() => ({
  openUrlMock: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      reopenAgentWorkspacePr: vi.fn(),
      closeAgentWorkspacePr: vi.fn(),
    },
  };
});

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (...args: unknown[]) => openUrlMock(...args),
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
  },
}));

function closedPrWorkspace(
  overrides: Partial<AgentConversationWorkspace> = {},
): AgentConversationWorkspace {
  return conversationWorkspaceFixture({
    conversationId: "conversation-1",
    publicationPrNumber: 42,
    publicationPrUrl: "https://github.com/ralphx/ralphx/pull/42",
    publicationPrStatus: "closed",
    ...overrides,
  });
}

function reopenResult(
  overrides: Partial<ReopenAgentWorkspacePrResult> = {},
): ReopenAgentWorkspacePrResult {
  return {
    outcome: "reopened_on_github",
    prNumber: 42,
    localWorkspace: "restored",
    message: "Pull request reopened",
    workspace: closedPrWorkspace({ publicationPrStatus: "open" }),
    ...overrides,
  };
}

function renderPanel(
  paneWorkspace: AgentConversationWorkspace | null = closedPrWorkspace(),
  queryClient: QueryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  }),
  props: Partial<ComponentProps<typeof AgentPublishPanel>> = {},
) {
  return {
    queryClient,
    ...render(
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={0}>
          <div className="h-[480px]">
            <AgentPublishPanel
              workspace={paneWorkspace}
              conversationTitle="Agent conversation"
              onPublishWorkspace={vi.fn()}
              publishAttempt={null}
              activeSubTab="automation"
              showReviewTab={false}
              onSubTabChange={() => {}}
              reviewContent={() => null}
              {...props}
            />
          </div>
        </TooltipProvider>
      </QueryClientProvider>,
    ),
  };
}

async function openPublishActionsMenu() {
  const trigger = await screen.findByTestId("agents-publish-actions-menu");
  await userEvent.click(trigger);
  return screen.findByRole("menu");
}

describe("AgentsPublishPanel reopen PR", () => {
  beforeEach(() => {
    vi.mocked(chatApi.reopenAgentWorkspacePr).mockReset();
    vi.mocked(chatApi.closeAgentWorkspacePr).mockReset();
    vi.mocked(toast.success).mockReset();
    vi.mocked(toast.error).mockReset();
    vi.mocked(toast.info).mockReset();
    vi.mocked(toast.warning).mockReset();
    openUrlMock.mockReset();
    openUrlMock.mockResolvedValue(undefined);
  });

  it("shows the Reopen PR overflow item when the terminal status is closed", async () => {
    renderPanel(closedPrWorkspace());
    const menu = await openPublishActionsMenu();
    expect(within(menu).getByTestId("agents-reopen-pr")).toBeInTheDocument();
  });

  it("hides the Reopen PR overflow item when the terminal status is merged", async () => {
    renderPanel(closedPrWorkspace({ publicationPrStatus: "merged" }));
    await waitFor(() =>
      expect(
        screen.queryByTestId("agents-publish-actions-menu"),
      ).not.toBeInTheDocument(),
    );
  });

  it("hides the Reopen PR overflow item while repair is pending", async () => {
    renderPanel(
      closedPrWorkspace({
        maintenanceOperation: null,
        publicationPushStatus: "needs_agent",
        publicationPrStatus: null,
      }),
    );
    await waitFor(() =>
      expect(
        screen.queryByTestId("agents-publish-actions-menu"),
      ).not.toBeInTheDocument(),
    );
  });

  it("hides the Reopen PR overflow item while maintenance is active", async () => {
    renderPanel(
      closedPrWorkspace({
        publicationPrStatus: null,
        maintenanceOperation: {
          operationId: "op-1",
          generation: 1,
          source: "pr_autofix",
          stage: "repairing",
          status: "active",
          summary: "Repairing workspace",
          blocker: null,
          automaticContinuation: false,
          startedAt: "2026-04-23T09:00:00Z",
          updatedAt: "2026-04-23T09:00:00Z",
        },
      }),
    );
    await waitFor(() =>
      expect(
        screen.queryByTestId("agents-publish-actions-menu"),
      ).not.toBeInTheDocument(),
    );
  });

  it("opens a confirmation dialog and writes no cache when the outcome is confirmation_required", async () => {
    vi.mocked(chatApi.reopenAgentWorkspacePr).mockResolvedValue(
      reopenResult({
        outcome: "confirmation_required",
        message: "GitHub still reports this PR as closed.",
      }),
    );
    const { queryClient } = renderPanel(closedPrWorkspace());
    queryClient.setQueryData(
      agentWorkspaceKeys.workspace("conversation-1"),
      closedPrWorkspace(),
    );
    const menu = await openPublishActionsMenu();
    await userEvent.click(within(menu).getByTestId("agents-reopen-pr"));

    await waitFor(() =>
      expect(chatApi.reopenAgentWorkspacePr).toHaveBeenCalledWith(
        "conversation-1",
        false,
      ),
    );

    expect(
      await screen.findByText(/still closed/i),
    ).toBeInTheDocument();
    expect(
      queryClient.getQueryData(agentWorkspaceKeys.workspace("conversation-1")),
    ).toEqual(closedPrWorkspace());
  });

  it("invokes reopenAgentWorkspacePr with reopenOnGithub: true when the user confirms", async () => {
    vi.mocked(chatApi.reopenAgentWorkspacePr).mockImplementation(
      (_conversationId: string, reopenOnGithub: boolean) =>
        Promise.resolve(
          reopenOnGithub
            ? reopenResult({ outcome: "reopened_on_github" })
            : reopenResult({
                outcome: "confirmation_required",
                message: "GitHub still reports this PR as closed.",
              }),
        ),
    );
    renderPanel(closedPrWorkspace());
    const menu = await openPublishActionsMenu();
    await userEvent.click(within(menu).getByTestId("agents-reopen-pr"));

    const confirmButton = await screen.findByRole("button", {
      name: /reopen on github/i,
    });
    await userEvent.click(confirmButton);

    await waitFor(() =>
      expect(chatApi.reopenAgentWorkspacePr).toHaveBeenLastCalledWith(
        "conversation-1",
        true,
      ),
    );
  });

  it("patches the workspace cache and invalidates queries on success", async () => {
    const updatedWorkspace = closedPrWorkspace({ publicationPrStatus: "open" });
    vi.mocked(chatApi.reopenAgentWorkspacePr).mockResolvedValue(
      reopenResult({ outcome: "reopened_on_github", workspace: updatedWorkspace }),
    );
    const { queryClient } = renderPanel(closedPrWorkspace());
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const menu = await openPublishActionsMenu();
    await userEvent.click(within(menu).getByTestId("agents-reopen-pr"));

    await waitFor(() =>
      expect(
        queryClient.getQueryData(agentWorkspaceKeys.workspace("conversation-1")),
      ).toEqual(updatedWorkspace),
    );
    expect(invalidateSpy).toHaveBeenCalled();
    expect(toast.success).toHaveBeenCalledWith("Pull request reopened");
  });

  it("uses toast.info (not toast.success) when the outcome is already_merged", async () => {
    const mergedWorkspace = closedPrWorkspace({ publicationPrStatus: "merged" });
    vi.mocked(chatApi.reopenAgentWorkspacePr).mockResolvedValue(
      reopenResult({
        outcome: "already_merged",
        message: "This pull request was already merged.",
        workspace: mergedWorkspace,
      }),
    );
    const { queryClient } = renderPanel(closedPrWorkspace());
    const menu = await openPublishActionsMenu();
    await userEvent.click(within(menu).getByTestId("agents-reopen-pr"));

    await waitFor(() =>
      expect(toast.info).toHaveBeenCalledWith(
        "This pull request was already merged.",
      ),
    );
    expect(toast.success).not.toHaveBeenCalled();
    expect(
      queryClient.getQueryData(agentWorkspaceKeys.workspace("conversation-1")),
    ).toEqual(mergedWorkspace);
  });

  it("warns instead of celebrating when the local checkout could not be restored", async () => {
    const reopenedWorkspace = closedPrWorkspace({ publicationPrStatus: "open" });
    vi.mocked(chatApi.reopenAgentWorkspacePr).mockResolvedValue(
      reopenResult({
        outcome: "reopened_on_github",
        localWorkspace: "restore_failed",
        message:
          "Pull request #42 has been reopened on GitHub. The workspace could not be restored: the remote branch may have been deleted.",
        workspace: reopenedWorkspace,
      }),
    );
    const { queryClient } = renderPanel(closedPrWorkspace());
    const menu = await openPublishActionsMenu();
    await userEvent.click(within(menu).getByTestId("agents-reopen-pr"));

    await waitFor(() =>
      expect(toast.warning).toHaveBeenCalledWith(
        "Pull request #42 has been reopened on GitHub. The workspace could not be restored: the remote branch may have been deleted.",
      ),
    );
    expect(toast.success).not.toHaveBeenCalled();
    // The remote reopen still happened, so the workspace cache must still be patched.
    expect(
      queryClient.getQueryData(agentWorkspaceKeys.workspace("conversation-1")),
    ).toEqual(reopenedWorkspace);
  });
});
