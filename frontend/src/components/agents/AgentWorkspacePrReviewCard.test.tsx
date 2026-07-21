import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";

import {
  chatApi,
  type AgentWorkspacePrReviewAction,
  type AgentWorkspacePrReviewContext,
  type AgentWorkspacePrReviewMonitor,
} from "@/api/chat";

import { conversationWorkspaceFixture } from "./agentsTestFixtures";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { AgentWorkspacePrReviewCard } from "./AgentWorkspacePrReviewCard";

const { openUrlMock } = vi.hoisted(() => ({
  openUrlMock: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      submitAgentWorkspacePrReviewAction: vi.fn(),
      skipAgentWorkspacePrReviewAction: vi.fn(),
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
  },
}));

const now = "2026-06-18T12:00:00.000Z";

function monitor(
  overrides: Partial<AgentWorkspacePrReviewMonitor> = {},
): AgentWorkspacePrReviewMonitor {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    prNumber: 411,
    status: "awaiting_user",
    monitorEnabled: true,
    autoApproveEnabled: true,
    firstReviewCompleted: false,
    firstActionResolved: false,
    lastSeenHeadSha: "abcdef1234567890",
    lastReviewedHeadSha: null,
    lastReviewRunId: null,
    lastReviewOutcome: null,
    lastSubmittedReviewId: null,
    reviewArtifactId: "review-artifact-1",
    reviewArtifactHeadSha: "abcdef1234567890",
    reviewArtifactVersion: 1,
    reviewArtifactUpdatedAt: now,
    lastError: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function reviewAction(
  overrides: Partial<AgentWorkspacePrReviewAction> = {},
): AgentWorkspacePrReviewAction {
  return {
    id: "action-1",
    conversationId: "conversation-1",
    prNumber: 411,
    headSha: "abcdef1234567890",
    proposedAction: "request_changes",
    summary: "Found a blocking regression in the PR.",
    reviewBody: "Please fix the regression before merge.",
    findingsJson: '[{"path":"src/lib.rs"}]',
    status: "pending",
    submittedReviewId: null,
    createdByRunId: "run-1",
    createdAt: now,
    updatedAt: now,
    resolvedAt: null,
    ...overrides,
  };
}

function reviewContext(
  overrides: Partial<AgentWorkspacePrReviewContext> = {},
): AgentWorkspacePrReviewContext {
  return {
    success: true,
    workspace: conversationWorkspaceFixture({
      conversationId: "conversation-1",
      publicationPrNumber: 411,
      publicationPrUrl: "https://github.com/aigentive/ralphx.app/pull/411",
    }),
    events: [],
    prNumber: 411,
    prUrl: "https://github.com/aigentive/ralphx.app/pull/411",
    currentHeadSha: "abcdef1234567890",
    pendingActionHeadStatus: "current",
    health: null,
    reviewFeedback: null,
    monitor: monitor(),
    pendingAction: reviewAction(),
    recentActions: [],
    issueCommentEvidence: [],
    ...overrides,
  };
}

function renderCard(
  props: Partial<ComponentProps<typeof AgentWorkspacePrReviewCard>> = {},
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const context = props.context ?? reviewContext();
  queryClient.setQueryData(agentWorkspaceKeys.prReview("conversation-1"), context);

  return {
    queryClient,
    ...render(
      <QueryClientProvider client={queryClient}>
        <AgentWorkspacePrReviewCard
          conversationId="conversation-1"
          context={context}
          isLoading={false}
          isFetching={false}
          error={null}
          {...props}
        />
      </QueryClientProvider>,
    ),
  };
}

describe("AgentWorkspacePrReviewCard", () => {
  beforeEach(() => {
    vi.mocked(chatApi.submitAgentWorkspacePrReviewAction).mockReset();
    vi.mocked(chatApi.skipAgentWorkspacePrReviewAction).mockReset();
    vi.mocked(toast.success).mockReset();
    vi.mocked(toast.error).mockReset();
    openUrlMock.mockReset();
    openUrlMock.mockResolvedValue(undefined);
  });

  it("renders loading and unavailable states without PR review context", () => {
    const { rerender } = renderCard({
      context: null,
      isLoading: true,
    });

    expect(
      screen.getByTestId("agent-workspace-pr-review-card-loading"),
    ).toHaveTextContent("Loading PR review state...");

    rerender(
      <QueryClientProvider
        client={
          new QueryClient({
            defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
          })
        }
      >
        <AgentWorkspacePrReviewCard
          conversationId="conversation-1"
          context={null}
          isLoading={false}
          isFetching={false}
          error={new Error("boom")}
        />
      </QueryClientProvider>,
    );

    expect(
      screen.getByTestId("agent-workspace-pr-review-card-error"),
    ).toHaveTextContent("Review PR context is unavailable.");
  });

  it("opens the reviewed pull request from the monitor header", async () => {
    const user = userEvent.setup();

    renderCard();

    const openPrButton = screen.getByRole("button", {
      name: "Open PR #411 in GitHub",
    });
    expect(
      screen.getByText(/PR #411 · Reviewed head abcdef12/i),
    ).toBeInTheDocument();

    await user.click(openPrButton);

    expect(openUrlMock).toHaveBeenCalledWith(
      "https://github.com/aigentive/ralphx.app/pull/411",
    );
  });

  it("submits a proposed approval and updates the cached review action", async () => {
    const user = userEvent.setup();
    const pending = reviewAction({
      id: "approve-action",
      proposedAction: "approve",
      summary: "The follow-up commit fixed the issue.",
      reviewBody: "Approved after re-review.",
    });
    const submitted = reviewAction({
      ...pending,
      status: "submitted",
      submittedReviewId: "review-1",
      resolvedAt: now,
    });
    vi.mocked(chatApi.submitAgentWorkspacePrReviewAction).mockResolvedValue({
      success: true,
      monitor: monitor({
        status: "watching",
        firstReviewCompleted: true,
        lastSubmittedReviewId: "review-1",
      }),
      action: submitted,
      submittedReviewId: "review-1",
      submittedReviewUrl: "https://github.com/aigentive/ralphx.app/pull/411#pullrequestreview-1",
    });

    const { queryClient } = renderCard({
      context: reviewContext({
        pendingAction: pending,
        recentActions: [reviewAction({ id: "older-action", status: "skipped" })],
      }),
      isFetching: true,
    });

    expect(screen.getByText("Approve PR proposed")).toBeInTheDocument();
    expect(screen.getByText(/head abcdef12/i)).toBeInTheDocument();
    expect(screen.getByText("refreshing")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Approve PR/i }));

    await waitFor(() => {
      expect(chatApi.submitAgentWorkspacePrReviewAction).toHaveBeenCalledWith(
        "conversation-1",
        "approve-action",
        "approve",
      );
    });
    expect(toast.success).toHaveBeenCalledWith("PR approved");
    const cached = queryClient.getQueryData<AgentWorkspacePrReviewContext>(
      agentWorkspaceKeys.prReview("conversation-1"),
    );
    expect(cached).toMatchObject({
      pendingAction: null,
      monitor: { status: "watching", lastSubmittedReviewId: "review-1" },
    });
    expect(cached?.recentActions[0]).toMatchObject({
      id: "approve-action",
      status: "submitted",
    });
    expect(cached?.recentActions[1]).toMatchObject({
      id: "older-action",
      status: "skipped",
    });
  });

  it("blocks submitting a pending action until the review artifact matches the action head", async () => {
    const user = userEvent.setup();
    const pending = reviewAction({
      id: "stale-action",
      headSha: "new-head-sha",
    });

    renderCard({
      context: reviewContext({
        monitor: monitor({
          reviewArtifactId: "review-artifact-1",
          reviewArtifactHeadSha: "old-head-sha",
        }),
        pendingAction: pending,
      }),
    });

    expect(
      screen.getByText("Write the Review for this PR head before submitting."),
    ).toBeInTheDocument();

    const submitButton = screen.getByRole("button", { name: /Request Changes/i });
    expect(submitButton).toBeDisabled();

    await user.click(submitButton);

    expect(chatApi.submitAgentWorkspacePrReviewAction).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Skip" })).toBeEnabled();
  });

  it("keeps a stale proposal visible and skippable but disables submission", () => {
    renderCard({
      context: reviewContext({
        currentHeadSha: "current-head-b",
        pendingActionHeadStatus: "stale",
      }),
    });

    expect(screen.getByText("Found a blocking regression in the PR.")).toBeInTheDocument();
    expect(screen.getByText(/Reviewed head abcdef12/i)).toBeInTheDocument();
    expect(screen.getByText(/Verified current head current-/i)).toBeInTheDocument();
    expect(
      screen.getByText("PR head changed; a fresh review is required."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Request Changes/i }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Skip" })).toBeEnabled();
  });

  it("keeps an unverified proposal visible and skippable without calling the snapshot current", () => {
    renderCard({
      context: reviewContext({
        currentHeadSha: "source-snapshot-head",
        pendingActionHeadStatus: "unverified",
      }),
    });

    expect(screen.getByText("Found a blocking regression in the PR.")).toBeInTheDocument();
    expect(
      screen.getByText("The remote PR head cannot currently be verified."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Verified current head/i)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Request Changes/i }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Skip" })).toBeEnabled();
  });

  it("shows a recoverable consistency state when awaiting user has no pending action", () => {
    renderCard({
      context: reviewContext({
        pendingAction: null,
        pendingActionHeadStatus: null,
        recentActions: [],
      }),
    });

    expect(
      screen.getByText(
        "The reviewer proposal is temporarily unavailable. RalphX will keep trying to restore it.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("Waiting for a reviewer proposal"),
    ).not.toBeInTheDocument();
  });

  it("suppresses stale proposal controls when terminal workspace authority is present", () => {
    renderCard({
      context: reviewContext({
        workspace: conversationWorkspaceFixture({
          conversationId: "conversation-1",
          mode: "review_pr",
          publicationPrNumber: 411,
          publicationPrStatus: "merged",
        }),
        monitor: monitor({
          status: "awaiting_user",
          monitorEnabled: true,
          lastReviewOutcome: "approve",
        }),
        recentActions: [
          reviewAction({
            status: "superseded",
            resolvedAt: now,
          }),
        ],
      }),
    });

    expect(screen.getByText("Complete")).toBeInTheDocument();
    expect(screen.getByText("Last action: Superseded")).toBeInTheDocument();
    expect(screen.queryByText("Needs approval")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Skip" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Request Changes/i }),
    ).not.toBeInTheDocument();
  });

  it("shows retry guidance when a pending action has a saved submit failure", () => {
    renderCard({
      context: reviewContext({
        monitor: monitor({
          status: "awaiting_user",
          lastError: "network unavailable",
        }),
        pendingAction: reviewAction({ id: "retry-action" }),
      }),
    });

    expect(screen.getByText(/Previous submit failed/i)).toBeInTheDocument();
    expect(screen.getByText(/network unavailable/i)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Request Changes/i }),
    ).toBeEnabled();
  });

  it("skips a proposed comment and shows the last resolved action summary", async () => {
    const user = userEvent.setup();
    const pending = reviewAction({
      id: "comment-action",
      proposedAction: "comment",
      summary: "This deserves a non-blocking note.",
      reviewBody: "Leaving a comment only.",
    });
    const skipped = reviewAction({
      ...pending,
      status: "skipped",
      resolvedAt: now,
    });
    vi.mocked(chatApi.skipAgentWorkspacePrReviewAction).mockResolvedValue({
      success: true,
      monitor: monitor({ status: "watching" }),
      action: skipped,
    });

    const { rerender } = renderCard({
      context: reviewContext({ pendingAction: pending }),
    });

    expect(screen.getByText("Review comment proposed")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Skip" }));

    await waitFor(() => {
      expect(chatApi.skipAgentWorkspacePrReviewAction).toHaveBeenCalledWith(
        "conversation-1",
        "comment-action",
        "Skipped from Review PR action card",
      );
    });
    expect(toast.success).toHaveBeenCalledWith("PR review action skipped");

    rerender(
      <QueryClientProvider
        client={
          new QueryClient({
            defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
          })
        }
      >
        <AgentWorkspacePrReviewCard
          conversationId="conversation-1"
          context={reviewContext({
            monitor: monitor({
              status: "blocked",
              monitorEnabled: false,
              lastError: "GitHub review submission failed",
            }),
            pendingAction: null,
            recentActions: [skipped],
          })}
          isLoading={false}
          isFetching={false}
          error={null}
        />
      </QueryClientProvider>,
    );

    expect(screen.getByText("Blocked")).toBeInTheDocument();
    expect(screen.getByText("Last action: Skipped")).toBeInTheDocument();
    expect(screen.getByText("GitHub review submission failed")).toBeInTheDocument();
  });

  it.each([
    ["approved", "Approved"],
    ["superseded", "Superseded"],
  ] as const)(
    "shows %s actions as the last resolved action",
    (status, label) => {
      renderCard({
        context: reviewContext({
          monitor: monitor({ status: "watching" }),
          pendingAction: null,
          recentActions: [
            reviewAction({
              id: `${status}-action`,
              status,
              resolvedAt: now,
            }),
          ],
        }),
      });

      expect(screen.getByText(`Last action: ${label}`)).toBeInTheDocument();
      expect(
        screen.queryByText("Waiting for a reviewer proposal"),
      ).not.toBeInTheDocument();
    },
  );

  it("shows mutation errors from review action submission", async () => {
    const user = userEvent.setup();
    vi.mocked(chatApi.submitAgentWorkspacePrReviewAction).mockRejectedValue(
      new Error("GitHub rejected the review"),
    );

    renderCard();

    await user.click(screen.getByRole("button", { name: /Request Changes/i }));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith("GitHub rejected the review");
    });
  });
});
