import {
  getAgentsViewTestMocks,
  mockAgentViewData,
  renderAgentsView,
  selectSidebarConversationRow,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
  AgentConversationWorkspacePublicationEvent,
} from "@/api/chat";
import type { FileChange } from "@/api/diff";
import type { PullRequestDetail } from "@/api/github";
import { useChatStore } from "@/stores/chatStore";
import {
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";
import { getAgentConversationStoreKey } from "./agentConversations";
import {
  DEFAULT_AGENT_ARTIFACT_UI_STATE,
  useAgentArtifactUiStore,
} from "./agentArtifactUiStore";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { prKeys } from "@/hooks/usePullRequestDetail";

const deferredHydrationTimeout = { timeout: 3_000 };

const {
  getAgentConversationRuntimeStatusesMock,
  getPullRequestDetailMock,
  getAgentConversationWorkspaceFreshnessMock,
  getAgentConversationWorkspaceMock,
  getWorkspacePrAnnotationsMock,
  getWorkspaceReviewHunkAnnotationsMock,
  getWorkspaceDiffMock,
  getWorkspaceChangeSummaryMock,
  getWorkspaceReviewMock,
  getWorkspaceReviewContextMock,
  getWorkspaceStagedChangesMock,
  getWorkspaceUnstagedChangesMock,
  listAgentTaskListTasksMock,
  listAgentTaskListsMock,
  listAgentTasksMock,
  listAgentConversationWorkspacePublicationEventsMock,
  preloadAgentsArtifactPaneMock,
  publishAgentConversationWorkspaceMock,
  realPublishPanelState,
  sendAgentMessageMock,
  toastErrorMock,
  toastSuccessMock,
  updateWorkspaceFromBaseMock,
} = getAgentsViewTestMocks();

const reviewFile: FileChange = {
  path: "frontend/src/App.tsx",
  status: "modified",
  additions: 1,
  deletions: 1,
  isGenerated: false,
};

const checksDetail = (
  checks: PullRequestDetail["checks"],
): PullRequestDetail => ({
  state: "loaded",
  origin: "ownedOutbound",
  description: {
    number: 78,
    title: "Published pull request",
    body: null,
    author: "octocat",
    createdAt: "2026-07-23T15:00:00Z",
    url: "https://github.com/mock/project/pull/78",
    state: "open",
    isDraft: false,
    headRefName: "ralphx/ralphx/agent-abcdef12",
    baseRefName: "main",
  },
  checks,
  reviewSummary: null,
  issueComments: [],
  reviewThread: [],
  rxConversations: [],
  linkedTickets: [],
  sourcesUnavailable: [],
});

const fullFreshness = (
  overrides: Partial<AgentConversationWorkspaceFreshness> = {},
): AgentConversationWorkspaceFreshness => ({
  conversationId: "conversation-1",
  freshnessScope: "full",
  baseRef: "main",
  baseDisplayName: "Project default (main)",
  targetRef: "origin/main",
  capturedBaseCommit: "base-sha",
  targetBaseCommit: "base-sha",
  isBaseAhead: false,
  hasUncommittedChanges: false,
  unpublishedCommitCount: null,
  remoteRefreshed: true,
  worktreeStatusChecked: true,
  baseStatus: "valid",
  effectiveBaseRef: "main",
  effectiveBaseDisplayName: "Project default (main)",
  baseBlockReason: null,
  ...overrides,
});

function configurePublishPane({
  workspace = {},
  freshness = {},
  changes = [reviewFile],
  reviewGateStatus = null,
}: {
  workspace?: Partial<AgentConversationWorkspace>;
  freshness?: Partial<AgentConversationWorkspaceFreshness>;
  changes?: FileChange[];
  reviewGateStatus?: "reviewing" | "blocking" | "failed" | "required" | null;
} = {}) {
  mockAgentViewData(conversation({ agentMode: workspace.mode ?? "edit" }));
  getAgentConversationWorkspaceMock.mockResolvedValue(
    conversationWorkspace({ mode: "edit", ...workspace }),
  );
  getAgentConversationWorkspaceFreshnessMock.mockResolvedValue(
    fullFreshness(freshness),
  );
  getWorkspaceReviewMock.mockResolvedValue({
    changes,
    commits: [],
    baseRef: "main",
    headRef: "HEAD",
  });
  const reviewContext = {
    success: true,
    workspace: conversationWorkspace({ mode: "edit", ...workspace }),
    events: [],
    target: null,
    monitor: {
      status: "idle",
      reviewArtifactId: null,
      reviewArtifactVersion: null,
      reviewGateStatus,
      reviewBlockingSummary:
        reviewGateStatus === "blocking" ? "Address the blocking finding." : null,
    },
    isCurrent: false,
    isOutdated: false,
    shouldShowTab: reviewGateStatus !== null,
  };
  getWorkspaceReviewContextMock.mockResolvedValue(reviewContext);
  realPublishPanelState.enabled = true;
  realPublishPanelState.reviewContext = reviewContext;
}

async function openPublishPane() {
  renderAgentsView();
  selectSidebarConversationRow();
  fireEvent.click(await screen.findByTestId("agents-publish-workspace"));
  return screen.findByTestId(
    "agents-publish-actionbar",
    undefined,
    deferredHydrationTimeout,
  );
}

describe("AgentsView publish", () => {
  beforeEach(() => {
    setupAgentsViewTest();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
  });

  it("labels a current open pull request as published to GitHub", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      },
      freshness: { unpublishedCommitCount: 0 },
    });

    const actionbar = await openPublishPane();

    await waitFor(() => {
      expect(
        within(actionbar).getByRole("heading", { name: "Published to GitHub" }),
      ).toBeInTheDocument();
      expect(actionbar).toHaveTextContent("1 changed file published for review.");
    });
  });

  it("keeps publishing ahead of conflict presentation and action branches", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "pushing",
        publicationPrNumber: 78,
        prSupervisionStatus: "blocked",
        prSupervisionSummary: "GitHub reported merge conflicts.",
      },
    });
    useAgentArtifactUiStore.setState({
      artifactByConversationId: {
        "conversation-1": {
          ...DEFAULT_AGENT_ARTIFACT_UI_STATE,
          isOpen: true,
          activeTab: "publish",
        },
      },
    });

    renderAgentsView();
    selectSidebarConversationRow();
    const actionbar = await screen.findByTestId(
      "agents-publish-actionbar",
      undefined,
      deferredHydrationTimeout,
    );

    expect(
      within(actionbar).getByRole("heading", { name: "Publishing workspace" }),
    ).toBeInTheDocument();
    expect(within(actionbar).getByTestId("agents-publish-confirm")).toBeDisabled();
    expect(
      within(actionbar).queryByTestId("agents-resolve-pr-conflicts"),
    ).not.toBeInTheDocument();
  });

  it.each([
    {
      name: "ready changes",
      workspace: {},
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Ready to publish",
      detail: "1 changed file ready for review.",
    },
    {
      name: "empty review",
      workspace: {},
      freshness: {},
      changes: [],
      reviewGateStatus: null,
      title: "No changes to publish",
      detail: "No changed files detected yet.",
    },
    {
      name: "repair pending",
      workspace: { publicationPushStatus: "needs_agent" },
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Repair in progress",
      detail: "Publishing will resume after the repair completes.",
    },
    {
      name: "pull request conflict",
      workspace: {
        publicationPrNumber: 78,
        prSupervisionStatus: "blocked",
        prSupervisionSummary: "GitHub reported merge conflicts.",
      },
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Pull request conflicts",
      detail: "Resolve conflicts",
    },
    {
      name: "base update required",
      workspace: {},
      freshness: { isBaseAhead: true, hasUncommittedChanges: true },
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Update from base required",
      detail: "Publishing will continue after this branch is updated.",
    },
    {
      name: "blocked base",
      workspace: {},
      freshness: {
        baseStatus: "blocked",
        effectiveBaseRef: null,
        effectiveBaseDisplayName: null,
        baseBlockReason: "Saved base cannot be resolved.",
      },
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Publishing blocked",
      detail: "Publishing is blocked until the workspace base branch is resolved.",
    },
    {
      name: "workspace review running",
      workspace: {},
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: "reviewing" as const,
      title: "Workspace Review in progress",
      detail: "Workspace Review is running.",
    },
    {
      name: "workspace review blocking",
      workspace: {},
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: "blocking" as const,
      title: "Workspace Review blocking",
      detail: "Address the blocking finding.",
    },
    {
      name: "workspace review failed",
      workspace: {},
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: "failed" as const,
      title: "Workspace Review failed",
      detail: "Retry Review before publishing.",
    },
    {
      name: "workspace review required",
      workspace: {},
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: "required" as const,
      title: "Workspace Review required",
      detail: "Workspace Review is required before publishing.",
    },
    {
      name: "task pipeline ownership",
      workspace: { linkedPlanBranchId: "plan-branch-1" },
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Managed by task pipeline",
      detail: "Publishing is managed by this ideation plan's task pipeline.",
    },
    {
      name: "description failure",
      workspace: { publicationPushStatus: "description_failed" },
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Publishing failed",
      detail: "could not draft a PR description",
    },
    {
      name: "description failure on a linked pull request",
      workspace: {
        publicationPushStatus: "description_failed",
        publicationPrNumber: 888,
      },
      freshness: { hasUncommittedChanges: true },
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Publishing failed",
      detail: "metadata step for PR #888",
    },
    {
      name: "automatic publishing paused",
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
        autoPublishEnabled: false,
      },
      freshness: { hasUncommittedChanges: true },
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Automatic publishing paused",
      detail: "Manual Commit & Publish remains available.",
    },
    {
      name: "automatic publishing armed",
      workspace: { autoPublishInitialPrEnabled: true },
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Auto Publish enabled",
      detail: "when the agent finishes.",
    },
    {
      name: "merged pull request",
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
        publicationPrStatus: "merged",
      },
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Pull Request Merged",
      detail: "a new workspace branch will be created automatically.",
    },
    {
      name: "closed pull request",
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
        publicationPrStatus: "closed",
      },
      freshness: {},
      changes: [reviewFile],
      reviewGateStatus: null,
      title: "Pull Request Closed",
      detail: "a new workspace branch will be created automatically.",
    },
  ])("renders the $name presentation", async ({
    workspace,
    freshness,
    changes,
    reviewGateStatus,
    title,
    detail,
  }) => {
    configurePublishPane({
      workspace,
      freshness,
      changes,
      reviewGateStatus,
    });

    const actionbar = await openPublishPane();

    await waitFor(() => {
      expect(within(actionbar).getByRole("heading", { name: title })).toBeInTheDocument();
      expect(actionbar).toHaveTextContent(detail);
    });
  });

  it("renders the repair-pending status button as a calm chip, not the accent CTA", async () => {
    configurePublishPane({
      workspace: { publicationPushStatus: "needs_agent" },
    });

    const actionbar = await openPublishPane();
    const repairButton = await within(actionbar).findByTestId(
      "agents-publish-repair-pending",
    );

    expect(repairButton).toBeDisabled();
    // A non-actionable status must not be painted as the solid accent CTA.
    expect(repairButton.className).not.toContain("bg-primary");
    // Stays fully legible instead of the default disabled-CTA half opacity.
    expect(repairButton.className).toContain("disabled:opacity-100");
    // Warning-tinted status surface via WKWebView-safe longhand tokens.
    const style = repairButton.getAttribute("style") ?? "";
    expect(style).toContain("--status-warning-muted");
    expect(style).toContain("--status-warning-border");
  });

  it("uses the durable operation to suppress a competing publish action", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "refreshed",
        maintenanceOperation: {
          operationId: "maintenance-1",
          generation: 1,
          source: "base_update",
          stage: "repairing",
          status: "active",
          summary: "Resolving the base conflict",
          blocker: null,
          automaticContinuation: true,
          startedAt: "2026-07-25T10:00:00Z",
          updatedAt: "2026-07-25T10:01:00Z",
        },
      },
      changes: [reviewFile],
    });

    const actionbar = await openPublishPane();

    expect(
      within(actionbar).getByRole("heading", { name: "Repairing workspace" }),
    ).toBeInTheDocument();
    expect(within(actionbar).getByRole("status")).toHaveTextContent(
      "Will continue automatically.",
    );
    expect(
      within(actionbar).getByTestId("agents-publish-maintenance-active"),
    ).toBeDisabled();
    expect(within(actionbar).queryByTestId("agents-publish-confirm")).not.toBeInTheDocument();
  });

  it("keeps exactly one explicit action for ready maintenance", async () => {
    configurePublishPane({
      workspace: {
        maintenanceOperation: {
          operationId: "maintenance-1",
          generation: 1,
          source: "base_update",
          stage: "ready",
          status: "ready",
          summary: "Base update completed",
          blocker: null,
          automaticContinuation: false,
          startedAt: "2026-07-25T10:00:00Z",
          updatedAt: "2026-07-25T10:01:00Z",
        },
      },
    });

    const actionbar = await openPublishPane();
    expect(
      within(actionbar).getByRole("heading", { name: "Base updated — ready to publish" }),
    ).toBeInTheDocument();
    expect(
      within(actionbar).getByTestId("agents-publish-resume-maintenance"),
    ).toBeEnabled();
    expect(within(actionbar).queryByTestId("agents-publish-confirm")).not.toBeInTheDocument();

  });

  it("keeps exactly one explicit recovery action for blocked maintenance", async () => {
    configurePublishPane({
      workspace: {
        maintenanceOperation: {
          operationId: "maintenance-2",
          generation: 2,
          source: "publish",
          stage: "blocked",
          status: "blocked",
          summary: "Repair cannot continue",
          blocker: "Resolve the protected branch policy.",
          automaticContinuation: false,
          startedAt: "2026-07-25T10:00:00Z",
          updatedAt: "2026-07-25T10:01:00Z",
        },
      },
    });
    const actionbar = await openPublishPane();
    expect(
      within(actionbar).getByRole("heading", { name: "Repair blocked" }),
    ).toBeInTheDocument();
    expect(
      within(actionbar).getByTestId("agents-publish-retry-maintenance"),
    ).toBeEnabled();
    expect(within(actionbar).queryByTestId("agents-publish-confirm")).not.toBeInTheDocument();
  });

  it("keeps the actionable Commit & Publish button as the accent CTA", async () => {
    configurePublishPane({ changes: [reviewFile] });

    const actionbar = await openPublishPane();
    const confirm = await within(actionbar).findByTestId(
      "agents-publish-confirm",
    );

    expect(confirm).toBeEnabled();
    expect(confirm.className).toContain("bg-primary");
    const style = confirm.getAttribute("style") ?? "";
    expect(style).not.toContain("--status-warning-muted");
  });

  it("renders the up-to-date status button as a calm success chip, not the accent CTA", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      },
      freshness: { unpublishedCommitCount: 0 },
    });

    const actionbar = await openPublishPane();
    const confirm = await within(actionbar).findByRole("button", {
      name: "PR is up to date",
    });

    expect(confirm).toBeDisabled();
    expect(confirm.className).not.toContain("bg-primary");
    expect(confirm.className).toContain("disabled:opacity-100");
    const style = confirm.getAttribute("style") ?? "";
    expect(style).toContain("--status-success-muted");
  });

  it("opens closed pull requests in count-free historical cumulative mode", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
        publicationPrStatus: "closed",
      },
      changes: [reviewFile],
    });

    await openPublishPane();

    await waitFor(() =>
      expect(screen.getByTestId("diff-filter-trigger")).toHaveTextContent(
        "Pull request changes",
      ),
    );
    expect(screen.getByTestId("diff-filter-trigger")).not.toHaveTextContent(
      "Workspace changes",
    );
  });

  it("shows inline-pane hydration while the visible review is loading", async () => {
    configurePublishPane();
    getWorkspaceReviewMock.mockImplementation(() => new Promise(() => {}));

    const actionbar = await openPublishPane();

    expect(
      within(actionbar).getByRole("heading", { name: "Checking workspace changes" }),
    ).toBeInTheDocument();
  });

  it("falls back to reviewing workspace changes after review loading fails", async () => {
    configurePublishPane();
    getWorkspaceReviewMock.mockRejectedValue(new Error("Review unavailable"));

    const actionbar = await openPublishPane();

    await waitFor(() =>
      expect(
        within(actionbar).getByRole("heading", { name: "Review workspace changes" }),
      ).toBeInTheDocument(),
    );
  });

  it("keeps a blocking Workspace Review above otherwise-current publish facts", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      },
      freshness: { unpublishedCommitCount: 0 },
      reviewGateStatus: "required",
    });

    const actionbar = await openPublishPane();

    await waitFor(() =>
      expect(
        within(actionbar).getByRole("heading", { name: "Workspace Review required" }),
      ).toBeInTheDocument(),
    );
    expect(within(actionbar).queryByText("Published to GitHub")).not.toBeInTheDocument();
  });

  it("labels only a normal base-ahead update as updating branch", async () => {
    updateWorkspaceFromBaseMock.mockImplementation(() => new Promise(() => {}));
    configurePublishPane({
      freshness: { isBaseAhead: true, hasUncommittedChanges: true },
    });
    const actionbar = await openPublishPane();
    await waitFor(() =>
      expect(
        within(actionbar).getByRole("heading", { name: "Update from base required" }),
      ).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByTestId("agents-update-from-base"));
    const confirmation = await screen.findByRole("alertdialog");
    fireEvent.click(within(confirmation).getByRole("button", { name: "Update branch" }));

    await waitFor(() =>
      expect(
        within(actionbar).getByRole("heading", { name: "Updating branch" }),
      ).toBeInTheDocument(),
    );
  });

  it("keeps conflict recovery above an in-flight branch update", async () => {
    updateWorkspaceFromBaseMock.mockImplementation(() => new Promise(() => {}));
    configurePublishPane({
      workspace: {
        publicationPrNumber: 78,
        prSupervisionStatus: "blocked",
        prSupervisionSummary: "GitHub reported merge conflicts.",
      },
      freshness: { isBaseAhead: true, hasUncommittedChanges: true },
    });
    const actionbar = await openPublishPane();
    await waitFor(() =>
      expect(
        within(actionbar).getByRole("heading", { name: "Pull request conflicts" }),
      ).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByTestId("agents-resolve-pr-conflicts"));
    const confirmation = await screen.findByRole("alertdialog");
    fireEvent.click(within(confirmation).getByRole("button", { name: "Resolve conflicts" }));

    expect(
      within(actionbar).getByRole("heading", { name: "Pull request conflicts" }),
    ).toBeInTheDocument();
    expect(within(actionbar).queryByText("Updating branch")).not.toBeInTheDocument();
  });

  it("shows publishing progress as one pipeline card before settling", async () => {
    let finishPublish: ((result: unknown) => void) | undefined;
    publishAgentConversationWorkspaceMock.mockImplementation(
      () => new Promise<unknown>((resolve) => {
        finishPublish = resolve;
      }),
    );
    configurePublishPane();
    const actionbar = await openPublishPane();

    fireEvent.click(screen.getByTestId("agents-publish-confirm"));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Commit & Publish" }));
    fireEvent.click(await screen.findByTestId("agents-publish-dialog-close"));

    await waitFor(() =>
      expect(
        within(actionbar).getByRole("heading", { name: "Publishing workspace" }),
      ).toBeInTheDocument(),
    );
    const pipeline = screen.getByTestId("agents-publish-pipeline");
    expect(pipeline).toHaveClass("mt-0");
    expect(screen.queryByTestId("agents-publish-summaries")).not.toBeInTheDocument();

    const publishedWorkspace = conversationWorkspace({
      mode: "edit",
      publicationPushStatus: "pushed",
      publicationPrNumber: 78,
    });
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue(
      fullFreshness({ unpublishedCommitCount: 0 }),
    );
    await act(async () =>
      finishPublish?.({
        workspace: publishedWorkspace,
        commitSha: "commit-sha",
        pushed: true,
        createdPr: true,
        prNumber: 78,
        prUrl: "https://github.com/mock/project/pull/78",
      }),
    );
  });

  it("locks publish controls from persisted background publish state without a manual attempt", async () => {
    configurePublishPane({
      workspace: { publicationPushStatus: "pushing" },
    });
    useAgentArtifactUiStore.setState({
      artifactByConversationId: {
        "conversation-1": {
          ...DEFAULT_AGENT_ARTIFACT_UI_STATE,
          isOpen: true,
          activeTab: "publish",
        },
      },
    });

    const { queryClient } = renderAgentsView();
    selectSidebarConversationRow();

    const headerShortcut = await screen.findByRole("button", {
      name: "Publishing",
    });
    expect(headerShortcut).toBeDisabled();

    const actionbar = await screen.findByTestId(
      "agents-publish-actionbar",
      undefined,
      deferredHydrationTimeout,
    );
    await waitFor(() =>
      expect(
        within(actionbar).getByRole("heading", { name: "Publishing workspace" }),
      ).toBeInTheDocument(),
    );
    const publishButton = within(actionbar).getByRole("button", {
      name: "Publishing",
    });
    expect(publishButton).toBeDisabled();

    fireEvent.click(publishButton);

    expect(publishAgentConversationWorkspaceMock).not.toHaveBeenCalled();

    act(() => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace("conversation-1"),
        conversationWorkspace({
          mode: "edit",
          publicationPushStatus: "pushed",
          publicationPrNumber: 78,
        }),
      );
      queryClient.setQueryData(
        agentWorkspaceKeys.scopedFreshness("conversation-1", "full"),
        fullFreshness({ unpublishedCommitCount: 0 }),
      );
    });

    await waitFor(() =>
      expect(
        within(actionbar).getByRole("button", { name: "PR is up to date" }),
      ).toBeInTheDocument(),
    );
  });

  it("settles an unresolved publish from durable evidence after the publish pane unmounts", async () => {
    const baselineEvent: AgentConversationWorkspacePublicationEvent = {
      id: "baseline-event",
      conversationId: "conversation-1",
      step: "checking",
      status: "succeeded",
      summary: "Previous publish check",
      classification: null,
      createdAt: new Date(Date.now() - 1_000).toISOString(),
    };
    const publishedEvent: AgentConversationWorkspacePublicationEvent = {
      id: "published-event",
      conversationId: "conversation-1",
      step: "published",
      status: "succeeded",
      summary: "Published pull request",
      classification: null,
      createdAt: new Date(Date.now() + 1_000).toISOString(),
    };
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([
      baselineEvent,
    ]);
    publishAgentConversationWorkspaceMock.mockImplementation(
      () => new Promise(() => undefined),
    );
    configurePublishPane();
    const { queryClient } = renderAgentsView();
    selectSidebarConversationRow();
    fireEvent.click(await screen.findByTestId("agents-publish-workspace"));
    await screen.findByTestId(
      "agents-publish-actionbar",
      undefined,
      deferredHydrationTimeout,
    );
    fireEvent.click(screen.getByTestId("agents-publish-confirm"));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Commit & Publish" }));
    await waitFor(() =>
      expect(publishAgentConversationWorkspaceMock).toHaveBeenCalledWith(
        "conversation-1",
      ),
    );

    fireEvent.click(await screen.findByTestId("agents-publish-dialog-close"));
    fireEvent.click(screen.getByTestId("agents-artifact-pane-close"));
    await waitFor(() =>
      expect(screen.queryByTestId("agents-publish-pane")).not.toBeInTheDocument(),
    );

    const publishedWorkspace = conversationWorkspace({
      mode: "edit",
      publicationPushStatus: "pushed",
      publicationPrNumber: 78,
    });
    getAgentConversationWorkspaceMock.mockResolvedValue(publishedWorkspace);
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue(
      fullFreshness({ unpublishedCommitCount: 0 }),
    );
    act(() => {
      queryClient.setQueryData(
        agentWorkspaceKeys.publicationEvents("conversation-1"),
        [baselineEvent, publishedEvent],
      );
    });

    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith("Published #78", {
        description: "Untitled agent",
        duration: 8_000,
        id: "agent-workspace-operation:conversation-1:publish",
      }),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(await screen.findByTestId("agents-publish-workspace"));
    const currentActionbar = await screen.findByTestId(
      "agents-publish-actionbar",
      undefined,
      deferredHydrationTimeout,
    );
    expect(
      within(currentActionbar).getByRole("button", { name: "PR is up to date" }),
    ).toBeInTheDocument();
  });

  it("keeps loaded inline annotations while removing redundant sync summaries", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      },
      freshness: { unpublishedCommitCount: 0 },
    });
    getWorkspaceDiffMock.mockResolvedValue({
      filePath: reviewFile.path,
      language: "typescript",
      hunks: [],
      oldTotalLines: 1,
      newTotalLines: 1,
      isBinary: false,
    });
    getWorkspacePrAnnotationsMock.mockResolvedValue({
      prNumber: 78,
      headSha: "head-sha",
      annotations: [{
        id: "review-comment:1",
        source: "review_comment",
        path: reviewFile.path,
        side: "right",
        startLine: 1,
        endLine: 1,
        startColumn: null,
        endColumn: null,
        level: "comment",
        status: null,
        title: null,
        message: "Please adjust this line.",
        author: "octocat",
        checkName: null,
        url: null,
        isOutdated: false,
        createdAt: null,
      }],
      sourcesUnavailable: [{
        source: "check_runs",
        reason: "Missing checks permission",
      }],
    });
    getWorkspaceReviewHunkAnnotationsMock.mockResolvedValue({
      artifactId: "artifact-1",
      artifactVersion: 1,
      targetScope: "selected_source",
      headSha: "head-sha",
      diffFingerprint: "fingerprint-1",
      annotations: [{
        id: "workspace-review-hunk-1",
        conversationId: "conversation-1",
        projectId: "project-1",
        artifactId: "artifact-1",
        artifactVersion: 1,
        targetScope: "selected_source",
        headSha: "head-sha",
        diffFingerprint: "fingerprint-1",
        path: reviewFile.path,
        diffSource: "selected_source",
        hunkHeader: "@@ -1,1 +1,1 @@",
        oldStart: 1,
        oldLines: 1,
        newStart: 1,
        newLines: 1,
        title: "Review summary",
        message: "This hunk updates inline diffs.",
        level: "notice",
        createdByRunId: "run-1",
        createdAt: "2026-07-01T00:00:00Z",
      }],
    });

    await openPublishPane();

    expect(
      await screen.findByTestId("agents-pr-annotations-partial-warning"),
    ).toHaveTextContent("GitHub annotations partially unavailable");
    expect(await screen.findByTestId("file-diff-annotation-count")).toHaveTextContent("1");
    expect(screen.getByTestId("file-diff-hunk-annotation-count")).toHaveTextContent("1");
    expect(screen.queryByText(/GitHub annotation.*synced/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/workspace review note.*synced/i)).not.toBeInTheDocument();
    expect(screen.queryByText("Checking GitHub annotations...")).not.toBeInTheDocument();
  });

  it("opens the right-side publish pane from the Commit & Publish header shortcut", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));

    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId("agents-publish-workspace");
    expect(screen.queryByTestId("agents-artifact-pane")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    await waitFor(() =>
      expect(screen.getByTestId("agents-artifact-pane")).toBeInTheDocument()
    );
    expect(publishAgentConversationWorkspaceMock).not.toHaveBeenCalled();
  });

  it("keeps the right-side unstaged count and files synced to the live conversation summary", async () => {
    const firstFile: FileChange = {
      path: "src/First.tsx",
      status: "modified",
      additions: 3,
      deletions: 1,
      isGenerated: false,
    };
    const secondFile: FileChange = {
      path: "src/Second.tsx",
      status: "added",
      additions: 5,
      deletions: 0,
      isGenerated: false,
    };
    configurePublishPane({ changes: [firstFile] });
    getWorkspaceChangeSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 1, additions: 3, deletions: 1 },
    });
    getWorkspaceUnstagedChangesMock.mockResolvedValue([firstFile]);

    const { queryClient } = renderAgentsView();
    selectSidebarConversationRow();
    await screen.findByTestId(
      "diff-filter-trigger",
      undefined,
      deferredHydrationTimeout,
    );
    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    const pane = await screen.findByTestId("agents-artifact-pane");
    const actionbar = within(pane).getByTestId("agents-publish-actionbar");
    await waitFor(() =>
      expect(
        within(actionbar).getByTestId("agents-publish-change-facts"),
      ).toHaveTextContent("1 file"),
    );
    expect(within(actionbar).getByTestId("agents-publish-additions")).toHaveTextContent(
      "+3",
    );
    expect(within(actionbar).getByTestId("agents-publish-deletions")).toHaveTextContent(
      "−1",
    );
    await waitFor(() =>
      expect(within(pane).getByTestId("inline-diffs-file-count")).toHaveTextContent(
        "1",
      ),
    );
    expect(within(pane).getByTestId("diff-filter-trigger")).toHaveTextContent(
      "Unstaged (1 file)",
    );
    expect(getWorkspaceUnstagedChangesMock).toHaveBeenCalledTimes(1);

    getWorkspaceUnstagedChangesMock.mockResolvedValue([firstFile, secondFile]);
    act(() => {
      queryClient.setQueryData(
        agentWorkspaceKeys.changeSummary("conversation-1"),
        {
          supportsWorktreeModes: true,
          staged: { fileCount: 0, additions: 0, deletions: 0 },
          unstaged: { fileCount: 2, additions: 8, deletions: 1 },
        },
      );
    });

    await waitFor(() =>
      expect(within(pane).getByTestId("diff-filter-trigger")).toHaveTextContent(
        "Unstaged (2 files)",
      ),
    );
    await waitFor(() =>
      expect(within(pane).getByTestId("inline-diffs-file-count")).toHaveTextContent(
        "2",
      ),
    );
    expect(
      within(actionbar).getByTestId("agents-publish-change-facts"),
    ).toHaveTextContent("2 files");
    expect(within(actionbar).getByTestId("agents-publish-additions")).toHaveTextContent(
      "+8",
    );
    expect(within(actionbar).getByTestId("agents-publish-deletions")).toHaveTextContent(
      "−1",
    );
    expect(getWorkspaceUnstagedChangesMock).toHaveBeenCalledTimes(2);
  });

  it("falls back to loaded review facts when live worktree totals are unavailable", async () => {
    configurePublishPane({
      changes: [
        reviewFile,
        {
          path: "frontend/src/NewPanel.tsx",
          status: "added",
          additions: 6,
          deletions: 0,
          isGenerated: false,
        },
      ],
    });
    getWorkspaceChangeSummaryMock.mockResolvedValue({
      supportsWorktreeModes: false,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 0, additions: 0, deletions: 0 },
    });

    const actionbar = await openPublishPane();

    await waitFor(() =>
      expect(
        within(actionbar).getByTestId("agents-publish-change-facts"),
      ).toHaveTextContent("2 files"),
    );
    expect(within(actionbar).getByTestId("agents-publish-additions")).toHaveTextContent(
      "+7",
    );
    expect(within(actionbar).getByTestId("agents-publish-deletions")).toHaveTextContent(
      "−1",
    );
  });

  it("keeps the overflow action accessible and disclosed by the app tooltip", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
        autoPublishEnabled: false,
      },
      freshness: { hasUncommittedChanges: true },
    });

    const actionbar = await openPublishPane();
    const overflow = within(actionbar).getByRole("button", {
      name: "Publish actions",
    });

    fireEvent.focus(overflow);
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Publish actions");
    expect(within(actionbar).getByTestId("agents-pr-supervision-status")).toHaveTextContent(
      "Auto Publish paused",
    );
  });

  it("relocates publish history and automation without losing action-strip state", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
        autoPublishEnabled: false,
      },
    });
    listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([
      {
        id: "event-1",
        conversationId: "conversation-1",
        step: "published",
        status: "succeeded",
        summary: "Published pull request",
        classification: null,
        createdAt: "2026-07-23T15:00:00Z",
      },
    ]);

    const actionbar = await openPublishPane();
    const tabs = screen.getByTestId("agents-publish-tabs");
    expect(
      Array.from(tabs.querySelectorAll('[role="tab"]')).map((tab) =>
        tab.getAttribute("data-testid"),
      ),
    ).toEqual([
      "agents-publish-tab-changes",
      "agents-publish-tab-review",
      "agents-publish-tab-checks",
      "agents-publish-tab-history",
      "agents-publish-tab-automation",
    ]);
    expect(screen.queryByTestId("agents-publish-events")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-pr-supervision-controls"),
    ).not.toBeInTheDocument();
    expect(
      within(actionbar).getByTestId("agents-pr-supervision-status"),
    ).toHaveTextContent("Auto Publish paused");

    fireEvent.mouseDown(screen.getByTestId("agents-publish-tab-checks"), {
      button: 0,
    });
    expect(
      await screen.findByTestId("agents-publish-checks-shell"),
    ).toBeInTheDocument();

    fireEvent.mouseDown(screen.getByTestId("agents-publish-tab-history"), {
      button: 0,
    });
    expect(
      await screen.findByTestId("agents-publish-events"),
    ).toBeInTheDocument();

    fireEvent.mouseDown(screen.getByTestId("agents-publish-tab-automation"), {
      button: 0,
    });
    expect(
      await screen.findByTestId("agents-pr-supervision-controls"),
    ).toBeInTheDocument();
    expect(
      within(actionbar).getByTestId("agents-pr-supervision-status"),
    ).toHaveTextContent("Auto Publish paused");

    fireEvent.mouseDown(screen.getByTestId("agents-publish-tab-changes"), {
      button: 0,
    });
    expect(
      screen.getByTestId("agents-publish-content-automation"),
    ).toHaveAttribute("data-state", "inactive");
    expect(
      screen.getByTestId("agents-publish-content-checks"),
    ).toHaveAttribute("data-state", "inactive");
  });

  it("reuses fresh PR-detail cache data for the Checks badge and content", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      },
    });
    const cachedDetail = checksDetail([
      {
        name: "lint",
        status: "completed",
        conclusion: "failure",
        detailsUrl: null,
      },
      {
        name: "types",
        status: "in_progress",
        conclusion: null,
        detailsUrl: null,
      },
      {
        name: "unit",
        status: "completed",
        conclusion: "success",
        detailsUrl: null,
      },
    ]);

    const { queryClient } = renderAgentsView();
    queryClient.setQueryData(
      prKeys.detail({ projectId: "project-1", prNumber: 78 }),
      cachedDetail,
    );
    selectSidebarConversationRow();
    fireEvent.click(await screen.findByTestId("agents-publish-workspace"));
    await screen.findByTestId(
      "agents-publish-actionbar",
      undefined,
      deferredHydrationTimeout,
    );

    expect(
      within(screen.getByTestId("agents-publish-tab-checks")).getByLabelText(
        "1 failed and 1 pending checks",
      ),
    ).toHaveTextContent("2");
    expect(getPullRequestDetailMock).not.toHaveBeenCalled();

    fireEvent.mouseDown(screen.getByTestId("agents-publish-tab-checks"), {
      button: 0,
    });

    expect(await screen.findByText("lint")).toBeInTheDocument();
    expect(screen.getByText("types")).toBeInTheDocument();
    expect(screen.getByText("unit")).toBeInTheDocument();
    expect(getPullRequestDetailMock).not.toHaveBeenCalled();
  });

  it("suppresses the Checks attention badge when cached checks are all green", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      },
    });
    const { queryClient } = renderAgentsView();
    queryClient.setQueryData(
      prKeys.detail({ projectId: "project-1", prNumber: 78 }),
      checksDetail([
        {
          name: "unit",
          status: "completed",
          conclusion: "success",
          detailsUrl: null,
        },
      ]),
    );
    selectSidebarConversationRow();
    fireEvent.click(await screen.findByTestId("agents-publish-workspace"));
    await screen.findByTestId(
      "agents-publish-actionbar",
      undefined,
      deferredHydrationTimeout,
    );

    const checksTab = screen.getByTestId("agents-publish-tab-checks");
    expect(checksTab).toHaveTextContent("Checks");
    expect(
      within(checksTab).queryByLabelText(/failed and .* pending checks/),
    ).not.toBeInTheDocument();
  });

  it("paints the Checks shell before showing deferred loading work", async () => {
    configurePublishPane({
      workspace: {
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      },
    });
    getPullRequestDetailMock.mockImplementation(() => new Promise(() => {}));

    await openPublishPane();
    expect(getPullRequestDetailMock).not.toHaveBeenCalled();

    fireEvent.mouseDown(screen.getByTestId("agents-publish-tab-checks"), {
      button: 0,
    });

    expect(screen.getByTestId("agents-publish-checks-shell")).toBeInTheDocument();
    expect(screen.queryByText("Loading checks…")).not.toBeInTheDocument();
    expect(getPullRequestDetailMock).not.toHaveBeenCalled();

    expect(
      await screen.findByText("Loading checks…"),
    ).toBeInTheDocument();
    expect(getPullRequestDetailMock).toHaveBeenCalledTimes(1);
  });

  it("hides Checks for a source pull request without a published PR", async () => {
    configurePublishPane({
      workspace: {
        sourcePullRequest: {
          number: 77,
          url: "https://github.com/mock/project/pull/77",
          title: "Source PR",
          headRefName: "source/pr",
          baseRefName: "main",
          headRefOid: null,
        },
      },
    });

    await openPublishPane();

    expect(
      screen.queryByTestId("agents-publish-tab-checks"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-publish-content-checks"),
    ).not.toBeInTheDocument();
  });

  it("shows a composer workspace changes summary from the compact live path and loads files on expand", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceChangeSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 2, additions: 14, deletions: 2 },
    });
    getWorkspaceUnstagedChangesMock.mockResolvedValue([
      {
        path: "src/Foo.tsx",
        status: "modified",
        additions: 10,
        deletions: 2,
        isGenerated: false,
      },
      {
        path: "src/generated.ts",
        status: "added",
        additions: 4,
        deletions: 0,
        isGenerated: true,
      },
    ]);

    renderAgentsView();
    selectSidebarConversationRow();

    const changesToggle = await screen.findByTestId(
      "diff-filter-trigger",
      undefined,
      deferredHydrationTimeout,
    );
    expect(changesToggle).toHaveTextContent("Unstaged");
    expect(screen.getByTestId("agents-composer-workspace-changes-count")).toHaveTextContent(
      "2 files",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-additions")).toHaveTextContent(
      "+14",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-deletions")).toHaveTextContent(
      "−2",
    );
    fireEvent.focus(changesToggle);
    await waitFor(() =>
      expect(preloadAgentsArtifactPaneMock).toHaveBeenCalledTimes(1),
    );

    expect(
      screen.queryByTestId("agents-composer-workspace-changes-list"),
    ).not.toBeInTheDocument();
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
    expect(getWorkspaceUnstagedChangesMock).not.toHaveBeenCalled();
    expect(getWorkspaceStagedChangesMock).not.toHaveBeenCalled();

    fireEvent.click(changesToggle);

    await waitFor(() =>
      expect(getWorkspaceUnstagedChangesMock).toHaveBeenCalledWith("conversation-1"),
    );
    expect(
      await screen.findByTestId("agents-composer-workspace-file-src/Foo.tsx"),
    ).toBeInTheDocument();
    expect(
      await screen.findByTestId("agents-composer-workspace-file-src/generated.ts"),
    ).toHaveTextContent("Generated");
    expect(getWorkspaceDiffMock).not.toHaveBeenCalled();
    expect(getWorkspaceReviewMock).not.toHaveBeenCalled();
  });

  it("prefers unstaged files in the composer workspace summary when dirty files exist", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceChangeSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 1, additions: 7, deletions: 2 },
      unstaged: { fileCount: 1, additions: 3, deletions: 4 },
    });
    getWorkspaceStagedChangesMock.mockResolvedValue([
      {
        path: "src/Staged.tsx",
        status: "modified",
        additions: 7,
        deletions: 2,
        isGenerated: false,
      },
    ]);
    getWorkspaceUnstagedChangesMock.mockResolvedValue([
      {
        path: "src/Unstaged.tsx",
        status: "modified",
        additions: 3,
        deletions: 4,
        isGenerated: false,
      },
    ]);

    renderAgentsView();
    selectSidebarConversationRow();

    const changesToggle = await screen.findByTestId(
      "diff-filter-trigger",
      undefined,
      deferredHydrationTimeout,
    );
    expect(changesToggle).toHaveTextContent("Unstaged");
    expect(screen.getByTestId("agents-composer-workspace-changes-count")).toHaveTextContent(
      "1 file",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-additions")).toHaveTextContent(
      "+3",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-deletions")).toHaveTextContent(
      "−4",
    );
    expect(getWorkspaceStagedChangesMock).not.toHaveBeenCalled();
    expect(getWorkspaceUnstagedChangesMock).not.toHaveBeenCalled();

    fireEvent.click(changesToggle);

    await waitFor(() =>
      expect(getWorkspaceUnstagedChangesMock).toHaveBeenCalledWith("conversation-1"),
    );
    expect(
      await screen.findByTestId("agents-composer-workspace-file-src/Unstaged.tsx"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-composer-workspace-file-src/Workspace.tsx"),
    ).not.toBeInTheDocument();
  });

  it("prefers staged files in the composer workspace summary when no unstaged files exist", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceChangeSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 1, additions: 7, deletions: 2 },
      unstaged: { fileCount: 0, additions: 0, deletions: 0 },
    });
    getWorkspaceStagedChangesMock.mockResolvedValue([
      {
        path: "src/Staged.tsx",
        status: "modified",
        additions: 7,
        deletions: 2,
        isGenerated: false,
      },
    ]);
    getWorkspaceUnstagedChangesMock.mockResolvedValue([]);

    renderAgentsView();
    selectSidebarConversationRow();

    const changesToggle = await screen.findByTestId(
      "diff-filter-trigger",
      undefined,
      deferredHydrationTimeout,
    );
    expect(changesToggle).toHaveTextContent("Staged");
    expect(screen.getByTestId("agents-composer-workspace-changes-count")).toHaveTextContent(
      "1 file",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-additions")).toHaveTextContent(
      "+7",
    );
    expect(screen.getByTestId("agents-composer-workspace-changes-deletions")).toHaveTextContent(
      "−2",
    );
  });

  it("switches the composer context tray between tasks and changes", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    listAgentTasksMock.mockResolvedValue([
      {
        taskId: "task-1",
        taskNumber: 1,
        title: "Add runtime task output shim",
        state: "active",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: ["task-2"],
        availability: "ready",
        updatedAt: "2026-05-20T10:00:00Z",
      },
      {
        taskId: "task-2",
        taskNumber: 2,
        title: "Render ledger rows",
        state: "open",
        ownerAgent: null,
        blockedBy: ["task-1"],
        blocks: [],
        availability: "blocked",
        updatedAt: "2026-05-20T10:01:00Z",
      },
    ]);
    getWorkspaceChangeSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 1, additions: 10, deletions: 2 },
    });

    renderAgentsView();
    selectSidebarConversationRow();

    const taskToggle = await screen.findByTestId(
      "agents-composer-tasks-toggle",
      undefined,
      deferredHydrationTimeout,
    );
    const changesToggle = await screen.findByTestId("diff-filter-trigger");
    expect(taskToggle.compareDocumentPosition(changesToggle)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(taskToggle).toHaveTextContent("Tasks");
    expect(taskToggle).toHaveTextContent("0/2");
    expect(changesToggle).toHaveTextContent("Unstaged");

    fireEvent.click(screen.getByTestId("agents-composer-tasks-toggle"));

    await waitFor(() =>
      expect(screen.getByTestId("agents-composer-task-list")).toHaveTextContent(
        "Add runtime task output shim",
      ),
    );
    expect(screen.getByTestId("agents-composer-task-2")).toHaveTextContent(
      "blocked by #1",
    );
    expect(
      screen.queryByTestId("agents-composer-workspace-changes-list"),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("diff-filter-trigger"));

    expect(screen.getByTestId("agents-composer-workspace-changes-list")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-composer-task-list")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("diff-filter-trigger"));

    expect(screen.queryByTestId("agents-composer-context-tray-body")).not.toBeInTheDocument();
  });

  it("auto-expands the composer task ledger for live task updates", async () => {
    const scrollIntoViewMock = vi.fn();
    const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoViewMock,
    });
    try {
      const activeConversation = conversation({ agentMode: "edit" });
      mockAgentViewData(activeConversation);
      getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
      getAgentConversationRuntimeStatusesMock.mockResolvedValue({
        [activeConversation.id]: {
          conversationId: activeConversation.id,
          isRunning: true,
          agentStatus: "generating",
          primarySource: "workspace",
          summaryLabel: "Agent running",
          items: [],
        },
      });
      listAgentTasksMock.mockResolvedValue([
        {
          taskId: "task-1",
          taskNumber: 1,
          title: "Smoke the task ledger",
          state: "active",
          ownerAgent: "worker",
          blockedBy: [],
          blocks: [],
          availability: "ready",
          updatedAt: "2026-05-20T10:00:00Z",
        },
      ]);
      renderAgentsView();
      selectSidebarConversationRow();
      act(() => {
        useChatStore.setState({
          agentStatus: {
            [getAgentConversationStoreKey(activeConversation)]: "generating",
          },
        });
      });

      await screen.findByTestId(
        "agents-composer-task-list",
        undefined,
        deferredHydrationTimeout,
      );
      expect(screen.getByTestId("agents-composer-tasks-toggle")).toHaveAttribute(
        "aria-expanded",
        "true",
      );
      expect(screen.getByTestId("agents-composer-task-1")).toHaveTextContent(
        "Smoke the task ledger",
      );
      expect(screen.getByTestId("agents-composer-task-1")).toHaveStyle({
        backgroundColor: "var(--bg-hover)",
      });
      await waitFor(() =>
        expect(scrollIntoViewMock).toHaveBeenCalledWith(
          expect.objectContaining({
            block: "nearest",
            behavior: "smooth",
          }),
        ),
      );
    } finally {
      Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
        configurable: true,
        value: originalScrollIntoView,
      });
    }
  });

  it("shows a check icon when all tasks are done", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    listAgentTasksMock.mockResolvedValue([
      {
        taskId: "task-1",
        taskNumber: 1,
        title: "First task",
        state: "done",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:00:00Z",
      },
      {
        taskId: "task-2",
        taskNumber: 2,
        title: "Second task",
        state: "done",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:01:00Z",
      },
    ]);

    renderAgentsView();
    selectSidebarConversationRow();

    const toggle = await screen.findByTestId(
      "agents-composer-tasks-toggle",
      undefined,
      deferredHydrationTimeout,
    );
    expect(toggle).toHaveTextContent("Tasks");
    expect(screen.getByTestId("agents-composer-tasks-count")).toHaveTextContent("2");
    expect(toggle.querySelector("svg.lucide-check")).toBeInTheDocument();
    expect(toggle.querySelector("svg.lucide-loader-circle")).not.toBeInTheDocument();
  });

  it("shows a spinner icon when tasks are actively in progress", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    listAgentTasksMock.mockResolvedValue([
      {
        taskId: "task-1",
        taskNumber: 1,
        title: "Done task",
        state: "done",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:00:00Z",
      },
      {
        taskId: "task-2",
        taskNumber: 2,
        title: "Active task",
        state: "active",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:01:00Z",
      },
    ]);

    renderAgentsView();
    selectSidebarConversationRow();

    const toggle = await screen.findByTestId(
      "agents-composer-tasks-toggle",
      undefined,
      deferredHydrationTimeout,
    );
    expect(screen.getByTestId("agents-composer-tasks-count")).toHaveTextContent("1/2");
    expect(toggle.querySelector("svg.lucide-loader-circle")).toBeInTheDocument();
    expect(toggle.querySelector("svg.lucide-check")).not.toBeInTheDocument();
  });

  it("keeps the first active task visible in the collapsed task window", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    listAgentTasksMock.mockResolvedValue([
      {
        taskId: "task-1",
        taskNumber: 1,
        title: "Oldest active task",
        state: "active",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:00:00Z",
      },
      {
        taskId: "task-2",
        taskNumber: 2,
        title: "Second task",
        state: "done",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:01:00Z",
      },
      {
        taskId: "task-3",
        taskNumber: 3,
        title: "Third task",
        state: "open",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:02:00Z",
      },
      {
        taskId: "task-4",
        taskNumber: 4,
        title: "Fourth task",
        state: "open",
        ownerAgent: null,
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:03:00Z",
      },
    ]);

    renderAgentsView();
    selectSidebarConversationRow();

    const taskToggle = await screen.findByTestId(
      "agents-composer-tasks-toggle",
      undefined,
      deferredHydrationTimeout,
    );

    fireEvent.click(taskToggle);

    await waitFor(() =>
      expect(screen.getByTestId("agents-composer-task-list")).toBeInTheDocument(),
    );

    expect(screen.getByTestId("agents-composer-task-1")).toBeInTheDocument();
    expect(screen.getByTestId("agents-composer-task-2")).toBeInTheDocument();
    expect(screen.getByTestId("agents-composer-task-3")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-composer-task-4")).not.toBeInTheDocument();

    const showMoreButton = screen.getByTestId("agents-composer-tasks-show-more");
    expect(showMoreButton).toHaveTextContent("Show 1 more in this list");

    fireEvent.click(showMoreButton);

    expect(screen.getByTestId("agents-composer-task-4")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-composer-tasks-show-older")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-composer-tasks-show-more")).not.toBeInTheDocument();
  });

  it("reveals earlier tasks from the composer task window", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    listAgentTasksMock.mockResolvedValue([
      {
        taskId: "task-1",
        taskNumber: 1,
        title: "First task",
        state: "done",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:00:00Z",
      },
      {
        taskId: "task-2",
        taskNumber: 2,
        title: "Second task",
        state: "done",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:01:00Z",
      },
      {
        taskId: "task-3",
        taskNumber: 3,
        title: "Active task",
        state: "active",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:02:00Z",
      },
      {
        taskId: "task-4",
        taskNumber: 4,
        title: "Fourth task",
        state: "open",
        ownerAgent: null,
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-20T10:03:00Z",
      },
    ]);

    renderAgentsView();
    selectSidebarConversationRow();

    fireEvent.click(
      await screen.findByTestId(
        "agents-composer-tasks-toggle",
        undefined,
        deferredHydrationTimeout,
      ),
    );

    expect(screen.queryByTestId("agents-composer-task-1")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-composer-task-2")).toBeInTheDocument();
    expect(screen.getByTestId("agents-composer-task-3")).toHaveTextContent("Active task");

    const showOlderButton = screen.getByTestId("agents-composer-tasks-show-older");
    expect(showOlderButton).toHaveTextContent("Show 1 earlier in this list");

    fireEvent.click(showOlderButton);

    expect(screen.getByTestId("agents-composer-task-1")).toHaveTextContent("First task");
    expect(screen.queryByTestId("agents-composer-tasks-show-older")).not.toBeInTheDocument();
  });

  it("loads previous task lists as grouped history inside the tasks tray", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    listAgentTasksMock.mockResolvedValue([
      {
        taskId: "task-current",
        taskNumber: 1,
        title: "Current slice task",
        state: "active",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-23T10:00:00Z",
      },
    ]);
    listAgentTaskListsMock.mockResolvedValue([
      {
        listId: "list-current",
        listSequence: 2,
        taskCount: 1,
        openCount: 0,
        activeCount: 1,
        doneCount: 0,
        droppedCount: 0,
        createdAt: "2026-05-23T10:00:00Z",
        updatedAt: "2026-05-23T10:01:00Z",
      },
      {
        listId: "list-previous",
        listSequence: 1,
        taskCount: 2,
        openCount: 0,
        activeCount: 0,
        doneCount: 2,
        droppedCount: 0,
        createdAt: "2026-05-22T10:00:00Z",
        updatedAt: "2026-05-22T10:01:00Z",
      },
    ]);
    listAgentTaskListTasksMock.mockResolvedValue([
      {
        taskId: "task-previous-1",
        taskNumber: 1,
        title: "Previous slice task",
        state: "done",
        ownerAgent: "worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-05-22T10:00:00Z",
      },
    ]);

    renderAgentsView();
    selectSidebarConversationRow();

    const taskToggle = await screen.findByTestId(
      "agents-composer-tasks-toggle",
      undefined,
      deferredHydrationTimeout,
    );

    fireEvent.click(taskToggle);

    const previousListsToggle = await screen.findByTestId(
      "agents-composer-task-lists-show-previous",
    );
    expect(previousListsToggle).toHaveTextContent("Previous task lists");
    expect(previousListsToggle).toHaveTextContent("1");
    expect(screen.getByTestId("agents-composer-task-1")).toHaveTextContent(
      "Current slice task",
    );

    fireEvent.click(previousListsToggle);
    const previousSlice = await screen.findByTestId(
      "agents-composer-task-list-slice-list-previous",
    );
    expect(previousSlice).toHaveTextContent("Task list #1");
    expect(previousSlice).toHaveTextContent("2 tasks");
    expect(previousSlice).toHaveTextContent("Done");

    fireEvent.click(previousSlice.querySelector("button")!);

    await waitFor(() =>
      expect(listAgentTaskListTasksMock).toHaveBeenCalledWith({
        contextType: "conversation",
        contextId: "conversation-1",
        projectId: "project-1",
        listId: "list-previous",
        includeDone: true,
      }),
    );
    expect(
      await screen.findByTestId(
        "agents-composer-task-list-list-previous-task-1",
      ),
    ).toHaveTextContent("Previous slice task");

    fireEvent.click(previousSlice.querySelector("button")!);
    expect(
      screen.queryByTestId("agents-composer-task-list-slice-list-previous-tasks"),
    ).not.toBeInTheDocument();

    fireEvent.click(previousListsToggle);
    expect(
      screen.queryByTestId("agents-composer-task-list-slice-list-previous"),
    ).not.toBeInTheDocument();
  });

  it("opens the publish pane with a focused file request from the composer summary", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(conversationWorkspace({ mode: "edit" }));
    getWorkspaceChangeSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 1, additions: 10, deletions: 2 },
    });
    getWorkspaceUnstagedChangesMock.mockResolvedValue([
      {
        path: "src/Foo.tsx",
        status: "modified",
        additions: 10,
        deletions: 2,
        isGenerated: false,
      },
    ]);

    renderAgentsView();
    selectSidebarConversationRow();

    const changesToggle = await screen.findByTestId(
      "diff-filter-trigger",
      undefined,
      deferredHydrationTimeout,
    );
    fireEvent.click(changesToggle);
    await waitFor(() =>
      expect(getWorkspaceUnstagedChangesMock).toHaveBeenCalledWith("conversation-1"),
    );
    fireEvent.click(await screen.findByTestId("agents-composer-workspace-file-src/Foo.tsx"));

    const pane = await screen.findByTestId("agents-artifact-pane");
    expect(pane).toHaveAttribute("data-active-tab", "publish");
    expect(pane).toHaveAttribute("data-publish-focus-path", "src/Foo.tsx");
    expect(pane).toHaveAttribute("data-publish-focus-mode", "unstaged");
  });

  it("shows Update from base in the header shortcut when the workspace base moved", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
      })
    );
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "feature/agent-screen",
      baseDisplayName: "Current branch (feature/agent-screen)",
      targetRef: "origin/feature/agent-screen",
      capturedBaseCommit: "old-base",
      targetBaseCommit: "new-base",
      isBaseAhead: true,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
    });

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Update from feature/agent-screen"
      )
    );
  });

  it("shows Base unavailable in the header shortcut when backend blocks the saved base", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "edit",
        baseRef: "feature/deleted-base",
        baseDisplayName: "Current branch (feature/deleted-base)",
      })
    );
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "feature/deleted-base",
      baseDisplayName: "Current branch (feature/deleted-base)",
      targetRef: "",
      capturedBaseCommit: "old-base",
      targetBaseCommit: "",
      isBaseAhead: false,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
      baseStatus: "blocked",
      effectiveBaseRef: null,
      effectiveBaseDisplayName: null,
      baseBlockReason: "Saved base commit is not contained in the default branch",
    });

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Base unavailable"
      )
    );
  });

  it("shows Update from base in the header shortcut for ideation plan-branch workspaces", async () => {
    mockAgentViewData(conversation({ agentMode: "ideation" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "ideation",
        status: "missing",
        linkedIdeationSessionId: "session-1",
        linkedPlanBranchId: "plan-branch-1",
        publicationPrNumber: 90,
        publicationPrUrl: "https://github.com/mock/project/pull/90",
        publicationPrStatus: "Open",
        publicationPushStatus: "pushed",
        baseRef: "main",
        baseDisplayName: "Project default (main)",
      })
    );
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "feature/agent-screen",
      baseDisplayName: "Current branch (feature/agent-screen)",
      targetRef: "origin/feature/agent-screen",
      capturedBaseCommit: "old-base",
      targetBaseCommit: "new-base",
      isBaseAhead: true,
      hasUncommittedChanges: false,
      unpublishedCommitCount: null,
    });

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Update from feature/agent-screen"
      )
    );
    expect(screen.getByTestId("agents-publish-workspace")).toBeEnabled();
  });

  it("shows merged terminal state instead of Update from base in the header shortcut", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "edit",
        baseRef: "feature/agent-screen",
        baseDisplayName: "Current branch (feature/agent-screen)",
        publicationPrNumber: 91,
        publicationPrStatus: "merged",
        publicationPushStatus: "pushed",
      })
    );

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Merged"
      )
    );
    expect(screen.getByTestId("agents-publish-workspace")).not.toHaveTextContent(
      "Update from feature/agent-screen"
    );
    expect(getAgentConversationWorkspaceFreshnessMock).not.toHaveBeenCalled();
  });

  it("shows Published in the header shortcut when the workspace branch is already current", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock.mockResolvedValue(
      conversationWorkspace({
        mode: "edit",
        publicationPushStatus: "pushed",
        publicationPrNumber: 78,
      })
    );
    getAgentConversationWorkspaceFreshnessMock.mockResolvedValue({
      conversationId: "conversation-1",
      baseRef: "feature/agent-screen",
      baseDisplayName: "Current branch (feature/agent-screen)",
      targetRef: "origin/feature/agent-screen",
      capturedBaseCommit: "base-sha",
      targetBaseCommit: "base-sha",
      isBaseAhead: false,
      hasUncommittedChanges: false,
      unpublishedCommitCount: 0,
    });

    renderAgentsView();
    selectSidebarConversationRow();

    await waitFor(() =>
      expect(screen.getByTestId("agents-publish-workspace")).toHaveTextContent(
        "Published"
      )
    );
  });

  it("relies on the backend to route fixable publish failures into the workspace agent conversation", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock
      .mockResolvedValueOnce(conversationWorkspace({ mode: "edit" }))
      .mockResolvedValueOnce(
        conversationWorkspace({ mode: "edit", publicationPushStatus: "needs_agent" })
      );
    publishAgentConversationWorkspaceMock.mockRejectedValue(
      "Failed to commit: typecheck failed"
    );
    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId("agents-publish-workspace");
    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    await screen.findByTestId("agents-publish-confirm");
    fireEvent.click(screen.getByTestId("agents-publish-confirm"));

    await waitFor(() => expect(getAgentConversationWorkspaceMock).toHaveBeenCalledTimes(3));
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
    expect(toastErrorMock).toHaveBeenCalledWith(
      "Publish failed. Sent the error to the agent to fix.",
      {
        closeButton: true,
        description: "Untitled agent • Failed to commit: typecheck failed",
        dismissible: true,
        duration: 12_000,
        id: "agent-workspace-operation:conversation-1:publish",
      },
    );
  });

  it("does not send operational publish failures to the workspace agent", async () => {
    mockAgentViewData(conversation({ agentMode: "edit" }));
    getAgentConversationWorkspaceMock
      .mockResolvedValueOnce(conversationWorkspace({ mode: "edit" }))
      .mockResolvedValueOnce(
        conversationWorkspace({ mode: "edit", publicationPushStatus: "failed" })
      );
    publishAgentConversationWorkspaceMock.mockRejectedValue(
      "GitHub integration is not available"
    );
    renderAgentsView();
    selectSidebarConversationRow();

    await screen.findByTestId("agents-publish-workspace");
    fireEvent.click(screen.getByTestId("agents-publish-workspace"));

    await screen.findByTestId("agents-publish-confirm");
    fireEvent.click(screen.getByTestId("agents-publish-confirm"));

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "Failed to publish branch",
        {
          closeButton: true,
          description: "Untitled agent • GitHub integration is not available",
          dismissible: true,
          duration: 12_000,
          id: "agent-workspace-operation:conversation-1:publish",
        },
      ),
    );
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
  });

});
