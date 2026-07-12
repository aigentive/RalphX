import {
  getAgentsViewTestMocks,
  mockHarnessProviders,
  mockAgentSidebarData,
  mockAgentViewData,
  renderAgentsView,
  resetAgentSessionState,
  setupAgentsViewTest,
} from "./AgentsView.testSetup";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode, SetStateAction } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import type { AgentProvidersSettingsResponse } from "@/api/harness-providers";
import type { BranchBaseOption } from "@/components/shared/branchBaseOptions";
import { chatKeys } from "@/hooks/useChat";
import { FEATURE_FLAGS_QUERY_KEY } from "@/hooks/useFeatureFlags";
import { personaKeys } from "@/hooks/usePersonas";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import {
  agentProjectFixture as project,
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
} from "./agentsTestFixtures";
import { agentJiraIssueKeys } from "./agentJiraIssueQueries";
import { agentLinearIssueKeys } from "./agentLinearIssueQueries";
import { LINKED_SETUP_FAILURE_MARKER } from "./agentStartErrors";
import { useStartAgentConversation } from "./useStartAgentConversation";

const {
  archiveConversationMock,
  createAutomationDraftMock,
  createConversationMock,
  getTicketAssociationsMock,
  integratedChatPanelRenderMock,
  loadBranchBaseOptionsMock,
  loadPullRequestBaseOptionsMock,
  listAgentConversationWorkspacesByProjectMock,
  listConversationsMock,
  listIdeationSessionsMock,
  spawnConversationSessionNamerMock,
  startAgentConversationMock,
  updateAutomationSetupMock,
  useHarnessProvidersMock,
  useConversationMock,
  useProjectAgentConversationsMock,
  useProjectsMock,
} = getAgentsViewTestMocks();

const providerUpdatedAt = new Date().toISOString();

function prPickerBranchOption(): BranchBaseOption {
  return {
    key: "pull_request:42:feature/pr-picker",
    label: "#42 Add PR picker",
    detail: "feature/pr-picker -> main",
    source: "pull_request",
    selection: {
      kind: "local_branch",
      ref: "feature/pr-picker",
      displayName: "PR #42: Add PR picker",
      sourcePullRequest: {
        number: 42,
        title: "Add PR picker",
        url: "https://github.com/owner/repo/pull/42",
        headRefName: "feature/pr-picker",
        headRefOid: "abc123",
        baseRefName: "main",
      },
    },
  };
}

function agentProviderSettings(
  overrides: Partial<AgentProvidersSettingsResponse> = {},
): AgentProvidersSettingsResponse {
  const settings: AgentProvidersSettingsResponse = {
    defaultProvider: "codex",
    requiresOnboarding: false,
    providers: [
      {
        provider: "codex",
        enabled: true,
        isDefault: true,
        model: "gpt-5.5",
        effort: "xhigh",
        approvalPolicy: "never",
        sandboxMode: "danger-full-access",
        claudePermissionMode: null,
        claudeDangerouslySkipPermissions: false,
        claudeAllowDangerouslySkipPermissions: false,
        available: true,
        binaryFound: true,
        binaryPath: "/opt/homebrew/bin/codex",
        status: "Available codex detected.",
        error: null,
        missingCoreExecFeatures: [],
        supportsFastMode: true,
        fastModeSupportedModels: ["gpt-5.5", "gpt-5.4"],
        updatedAt: providerUpdatedAt,
      },
      {
        provider: "claude",
        enabled: true,
        isDefault: false,
        model: "sonnet",
        effort: "medium",
        approvalPolicy: null,
        sandboxMode: null,
        claudePermissionMode: "default",
        claudeDangerouslySkipPermissions: false,
        claudeAllowDangerouslySkipPermissions: false,
        available: true,
        binaryFound: true,
        binaryPath: "/usr/local/bin/claude",
        status: "Available claude detected.",
        error: null,
        missingCoreExecFeatures: [],
        supportsFastMode: false,
        fastModeSupportedModels: [],
        updatedAt: providerUpdatedAt,
      },
    ],
  };
  return { ...settings, ...overrides };
}

describe("AgentsView start conversation", () => {
  beforeEach(setupAgentsViewTest);

  it("defaults to the starter composer when no conversation is selected", async () => {
    mockAgentViewData();

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );
    expect(screen.getByTestId("agents-start-heading")).toHaveTextContent("Start your agent");
    expect(screen.getByTestId("agents-start-heading-word")).toHaveTextContent("agent");
    expect(screen.getByTestId("agents-start-project")).toBeInTheDocument();
    expect(screen.getByTestId("agents-start-base")).toBeInTheDocument();
    expect(screen.getByTestId("agent-composer-runtime-pill")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-start-new-project")).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    expect(screen.getByTestId("agents-start-new-project")).toBeInTheDocument();
    expect(screen.queryByTestId("integrated-chat-panel")).not.toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    // Workflow modes live on the Mode chip popover, not the "+" action menu.
    await userEvent.click(screen.getByTestId("agents-start-mode-chip"));
    expect(screen.getByTestId("agents-start-mode-edit")).toBeInTheDocument();
  });

  it("prefills and consumes a pending start conversation draft", async () => {
    mockAgentViewData();
    useAgentSessionStore.getState().setStartConversationDraft({
      projectId: "project-1",
      content: "replace ideation command with agent composer",
      mode: "edit",
      composerIntegrationReferences: [
        {
          provider: "clickup",
          kind: "clickup",
          id: "TASK-123",
          key: "TASK-123",
          title: "Demo task",
        },
      ],
      composerArtifactReferences: [
        {
          kind: "plan",
          artifactId: "plan-artifact-1",
          title: "Runtime Plan",
          sessionId: "session-1",
          version: 2,
          status: "approved",
        },
      ],
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-textarea")).toHaveValue(
        "replace ideation command with agent composer"
      )
    );
    expect(screen.getByTestId("agents-start-mode-chip")).toHaveTextContent("Agent");
    expect(
      screen.getByTestId("agent-composer-reference-pill-integration:clickup:TASK-123")
    ).toHaveTextContent("Demo task");
    const planReferencePill = screen.getByTestId(
      "agent-composer-reference-pill-artifact:plan:plan-artifact-1",
    );
    expect(planReferencePill).toHaveTextContent("Runtime Plan");
    expect(planReferencePill).toHaveTextContent("Approved");
    expect(planReferencePill).toHaveTextContent("v2");
    expect(useAgentSessionStore.getState().startConversationDraft).toBeNull();
  });

  it("restores unsent starter composer text and attachments from the draft store", async () => {
    mockAgentViewData();
    const file = new File(["draft"], "notes.md", { type: "text/markdown" });
    useChatStore
      .getState()
      .setComposerDraftContent("agents:start", "continue this draft");
    useChatStore.getState().setComposerDraftAttachments("agents:start", [
      {
        id: "starter-attachment-1",
        file,
        fileName: "notes.md",
        fileSize: 5,
        mimeType: "text/markdown",
      },
    ]);

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-textarea")).toHaveValue(
        "continue this draft"
      )
    );
    expect(screen.getByTestId("chat-attachment-gallery")).toHaveTextContent("notes.md");
  });

  it("restores a persisted selected conversation even when it is outside the first sidebar page", async () => {
    const restoredConversation = conversation({
      id: "conversation-restored",
      title: "Older restored agent",
      contextId: "project-1",
    });
    useProjectsMock.mockReturnValue({
      data: [project],
      isLoading: false,
    });
    useProjectAgentConversationsMock.mockReturnValue({
      data: [],
      conversations: [],
      isLoading: false,
      isSuccess: true,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    mockAgentSidebarData([restoredConversation]);
    useConversationMock.mockImplementation((conversationId: string | null) => ({
      data:
        conversationId === "conversation-restored"
          ? {
              conversation: restoredConversation,
              messages: [],
            }
          : null,
      isLoading: false,
    }));
    resetAgentSessionState({
      selectedProjectId: null,
      selectedConversationId: "conversation-restored",
    });

    renderAgentsView();

    expect(await screen.findByTestId("integrated-chat-panel")).toBeInTheDocument();
    expect(screen.getByTestId("agents-session-conversation-restored")).toHaveTextContent(
      "Older restored agent"
    );
  });

  it("deselects a clicked sidebar conversation on second click and shows the starter", async () => {
    const firstConversation = conversation({
      id: "conversation-1",
      title: "Current agent",
      contextId: "project-1",
      projectId: "project-1",
    });
    const clickedConversation = conversation({
      id: "conversation-older",
      title: "Older clicked agent",
      contextId: "project-1",
      projectId: "project-1",
      createdAt: "2026-04-20T09:00:00Z",
      updatedAt: "2026-04-20T09:00:00Z",
    });
    useProjectsMock.mockReturnValue({
      data: [project],
      isLoading: false,
    });
    useProjectAgentConversationsMock.mockReturnValue({
      data: [firstConversation],
      conversations: [firstConversation],
      isLoading: false,
      isSuccess: true,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    mockAgentSidebarData([firstConversation, clickedConversation]);
    useConversationMock.mockImplementation((conversationId: string | null) => ({
      data:
        conversationId === firstConversation.id
          ? {
              conversation: firstConversation,
              messages: [],
            }
          : null,
      isLoading: false,
    }));
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: firstConversation.id,
    });

    renderAgentsView();

    const clickedRow = await screen.findByTestId("agents-session-conversation-older");
    const clickedButton = clickedRow.querySelector("button");
    expect(clickedButton).not.toBeNull();
    fireEvent.click(clickedButton as HTMLButtonElement);

    await waitFor(() =>
      expect(useAgentSessionStore.getState().selectedConversationId).toBe(
        clickedConversation.id
      )
    );
    expect(clickedButton).toHaveAttribute("aria-current", "true");
    expect(screen.queryByTestId("agents-start-composer")).not.toBeInTheDocument();

    fireEvent.click(clickedButton);

    await waitFor(() =>
      expect(useAgentSessionStore.getState().selectedConversationId).toBeNull()
    );
    expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument();
  });

  it("restores starter composer text and attachments after visiting another conversation", async () => {
    const existingConversation = conversation({
      id: "conversation-existing",
      title: "Existing agent",
      contextId: "project-1",
      projectId: "project-1",
    });
    useProjectsMock.mockReturnValue({
      data: [project],
      isLoading: false,
    });
    useProjectAgentConversationsMock.mockReturnValue({
      data: [existingConversation],
      conversations: [existingConversation],
      isLoading: false,
      isSuccess: true,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    mockAgentSidebarData([existingConversation]);
    useConversationMock.mockImplementation((conversationId: string | null) => ({
      data:
        conversationId === existingConversation.id
          ? {
              conversation: existingConversation,
              messages: [],
            }
          : null,
      isLoading: false,
    }));
    resetAgentSessionState({
      selectedProjectId: null,
      selectedConversationId: null,
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );
    const file = new File(["draft"], "starter-draft.txt", {
      type: "text/plain",
    });
    fireEvent.change(screen.getByTestId("attachment-file-input"), {
      target: { files: [file] },
    });
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "keep this starter draft" },
    });
    expect(screen.getByText("starter-draft.txt")).toBeInTheDocument();

    const row = await screen.findByTestId("agents-session-conversation-existing");
    const button = row.querySelector("button");
    expect(button).not.toBeNull();
    fireEvent.click(button as HTMLButtonElement);
    await waitFor(() =>
      expect(screen.queryByTestId("agents-start-composer")).not.toBeInTheDocument()
    );

    fireEvent.click(button as HTMLButtonElement);
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );

    expect(screen.getByTestId("agents-start-textarea")).toHaveValue(
      "keep this starter draft"
    );
    expect(screen.getByText("starter-draft.txt")).toBeInTheDocument();
  });

  it("starts a new conversation directly from the starter composer and triggers the session namer", async () => {
    const invalidateSpy = vi.spyOn(QueryClient.prototype, "invalidateQueries");
    mockAgentViewData();

    const { queryClient } = renderAgentsView();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "fix agent landing flow" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          content: "fix agent landing flow",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
          mode: "edit",
          base: expect.objectContaining({
            kind: "project_default",
            ref: "main",
            branchMode: "isolated",
          }),
        })
      )
    );
    expect(createConversationMock).not.toHaveBeenCalled();
    expect(startAgentConversationMock.mock.calls[0]?.[0]).not.toHaveProperty(
      "conversationId"
    );
    expect(startAgentConversationMock.mock.calls[0]?.[0]).not.toHaveProperty(
      "personaId",
    );
    await waitFor(() =>
      expect(spawnConversationSessionNamerMock).toHaveBeenCalledWith(
        "conversation-2",
        "fix agent landing flow",
        "codex"
      )
    );
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    expect(screen.queryByTestId("agents-start-composer")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-conversation-workspace-line")).toHaveTextContent(
      "agent-conversation-2"
    );
    expect(useAgentSessionStore.getState().selectedConversationId).toBe("conversation-2");
    expect(
      useAgentSessionStore.getState().artifactByConversationId["conversation-2"]
    ).toEqual(
      expect.objectContaining({
        isOpen: false,
        activeTab: "plan",
      })
    );
    expect(queryClient.getQueryData(["chat", "conversations", "conversation-2"])).toEqual({
      conversation: expect.objectContaining({ id: "conversation-2" }),
      messages: [
        expect.objectContaining({
          conversationId: "conversation-2",
          role: "user",
          content: "fix agent landing flow",
        }),
      ],
    });
    expect(
      queryClient.getQueryData(["agents", "conversation-workspace", "conversation-2"])
    ).toEqual(expect.objectContaining({ conversationId: "conversation-2" }));
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        queryKey: ["agents", "project-conversations", "project-1"],
      })
    );
    invalidateSpy.mockRestore();
  });

  it("threads a selected active persona to the start invoke only when personas are enabled", async () => {
    mockAgentViewData();
    const { queryClient } = renderAgentsView();
    queryClient.setQueryData(FEATURE_FLAGS_QUERY_KEY, {
      activityPage: true,
      extensibilityPage: true,
      ideationPage: false,
      automationsPage: true,
      battleMode: true,
      teamMode: false,
      atlassianOauth: false,
      ticketingDashboard: false,
      agentPersonas: true,
    });
    queryClient.setQueryData(personaKeys.list(), [
      {
        id: "persona-reviewer",
        slug: "reviewer-voice",
        name: "Reviewer Voice",
        description: "Careful reviews",
        content: "# Reviewer",
        status: "active",
        version: 1,
        contentHash: "persona-hash",
        sourceSessionId: null,
        createdAt: "2026-01-01T00:00:00Z",
        updatedAt: "2026-01-01T00:00:00Z",
      },
    ]);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Choose persona" })).toBeInTheDocument(),
    );
    await userEvent.click(screen.getByRole("button", { name: "Choose persona" }));
    await userEvent.click(screen.getByRole("menuitemradio", { name: "Reviewer Voice" }));
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "Review the current changes" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({ personaId: "persona-reviewer" }),
      ),
    );
  });

  it("starts a new conversation with Team enabled", async () => {
    mockAgentViewData();

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument(),
    );
    fireEvent.click(screen.getByTestId("agents-start-team-control"));
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "coordinate this implementation" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          content: "coordinate this implementation",
          teamIntent: { coordinationMode: "rx_native_team" },
        }),
      ),
    );
  });

  it("starts a selected pull request in isolated branch mode by default", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    loadPullRequestBaseOptionsMock.mockResolvedValue([prPickerBranchOption()]);

    renderAgentsView();

    await user.click(await screen.findByTestId("agents-start-base"));
    await user.click(screen.getByRole("tab", { name: /PRs/i }));
    await waitFor(() =>
      expect(loadPullRequestBaseOptionsMock).toHaveBeenCalledWith({
        projectId: "project-1",
        query: "",
      })
    );
    const prOption = await screen.findByText("#42 Add PR picker");
    const prOptionButton = prOption.closest("button");
    expect(prOptionButton).not.toBeNull();
    await user.click(prOptionButton as HTMLButtonElement);
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent("#42 Add PR picker")
    );
    expect(
      screen.getByRole("switch", { name: /Use isolated branch/i })
    ).toHaveAttribute("aria-checked", "true");
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "review this PR" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          content: "review this PR",
          base: expect.objectContaining({
            kind: "local_branch",
            branchMode: "isolated",
            ref: "feature/pr-picker",
            displayName: "PR #42: Add PR picker",
            sourcePullRequest: expect.objectContaining({
              number: 42,
              headRefName: "feature/pr-picker",
              baseRefName: "main",
              headRefOid: "abc123",
            }),
          }),
        })
      )
    );
  });

  it("starts a selected pull request in linked branch mode after explicit opt-out", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    loadPullRequestBaseOptionsMock.mockResolvedValue([prPickerBranchOption()]);

    renderAgentsView();

    await user.click(await screen.findByTestId("agents-start-base"));
    await user.click(screen.getByRole("tab", { name: /PRs/i }));
    const prOption = await screen.findByText("#42 Add PR picker");
    await user.click(prOption.closest("button") as HTMLButtonElement);
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent("#42 Add PR picker")
    );

    const isolatedSwitch = screen.getByRole("switch", {
      name: /Use isolated branch/i,
    });
    expect(isolatedSwitch).toHaveAttribute("aria-checked", "true");
    await user.click(isolatedSwitch);
    expect(isolatedSwitch).toHaveAttribute("aria-checked", "false");

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "review this PR directly" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          content: "review this PR directly",
          base: expect.objectContaining({
            kind: "local_branch",
            branchMode: "linked",
            ref: "feature/pr-picker",
            sourcePullRequest: expect.objectContaining({
              number: 42,
              headRefName: "feature/pr-picker",
            }),
          }),
        })
      )
    );
  });

  it("shows linked setup failure inline and retries the same no-file start with isolation", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    loadPullRequestBaseOptionsMock.mockResolvedValue([prPickerBranchOption()]);
    startAgentConversationMock
      .mockRejectedValueOnce(
        new Error(
          `${LINKED_SETUP_FAILURE_MARKER} Selected branch 'feature/pr-picker' is already checked out; choose isolated branch mode`
        )
      )
      .mockResolvedValueOnce({
        conversation: conversation({
          id: "conversation-linked-retry",
          contextId: "project-1",
          title: null,
        }),
        workspace: conversationWorkspace({
          conversationId: "conversation-linked-retry",
        }),
        sendResult: {
          conversationId: "conversation-linked-retry",
          agentRunId: "run-linked-retry",
          isNewConversation: true,
          wasQueued: false,
          queuedAsPending: false,
          queuedMessageId: null,
        },
      });

    renderAgentsView();

    await user.click(await screen.findByTestId("agents-start-base"));
    await user.click(screen.getByRole("tab", { name: /PRs/i }));
    const prOption = await screen.findByText("#42 Add PR picker");
    await user.click(prOption.closest("button") as HTMLButtonElement);
    const isolatedSwitch = screen.getByRole("switch", {
      name: /Use isolated branch/i,
    });
    await user.click(isolatedSwitch);
    expect(isolatedSwitch).toHaveAttribute("aria-checked", "false");

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "review this PR directly" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() => expect(startAgentConversationMock).toHaveBeenCalledTimes(1));
    expect(createConversationMock).not.toHaveBeenCalled();
    const linkedPayload = startAgentConversationMock.mock.calls[0]?.[0];
    expect(linkedPayload).not.toHaveProperty("conversationId");
    expect(linkedPayload).toEqual(
      expect.objectContaining({
        content: "review this PR directly",
        base: expect.objectContaining({
          ref: "feature/pr-picker",
          branchMode: "linked",
          sourcePullRequest: expect.objectContaining({
            number: 42,
            headRefName: "feature/pr-picker",
          }),
        }),
      })
    );
    await waitFor(() =>
      expect(useAgentSessionStore.getState().startConversationFailure).toEqual(
        expect.objectContaining({
          kind: "linked_setup",
          message: expect.stringContaining("feature/pr-picker"),
        })
      )
    );
    await waitFor(() =>
      expect(useAgentSessionStore.getState().selectedConversationId).toBeNull()
    );

    const linkedError = await screen.findByTestId("agents-start-linked-setup-error");
    expect(linkedError).toHaveTextContent("Linked branch setup failed");
    expect(linkedError).toHaveTextContent("Selected branch 'feature/pr-picker'");
    expect(linkedError).toHaveTextContent("Branch isolation creates a separate RalphX branch");
    expect(screen.getByTestId("agents-start-textarea")).toHaveValue(
      "review this PR directly"
    );

    await user.click(screen.getByTestId("agents-start-linked-setup-retry"));

    await waitFor(() => expect(startAgentConversationMock).toHaveBeenCalledTimes(2));
    const retryPayload = startAgentConversationMock.mock.calls[1]?.[0];
    expect(retryPayload).not.toHaveProperty("conversationId");
    expect(retryPayload).toEqual(
      expect.objectContaining({
        content: "review this PR directly",
        base: expect.objectContaining({
          ref: "feature/pr-picker",
          branchMode: "isolated",
          sourcePullRequest: expect.objectContaining({
            number: 42,
            headRefName: "feature/pr-picker",
          }),
        }),
      })
    );
    await waitFor(() =>
      expect(useAgentSessionStore.getState().selectedConversationId).toBe(
        "conversation-linked-retry"
      )
    );
  });

  it("forces isolated branch mode when starting from the current project branch", async () => {
    mockAgentViewData();
    const currentBranchOptions: BranchBaseOption[] = [
      {
        key: "project_default:main",
        label: "Project default (main)",
        detail: "Configured project base branch",
        source: "project",
        selection: {
          kind: "project_default",
          ref: "main",
          displayName: "Project default (main)",
        },
      },
      {
        key: "current_branch:feature/current",
        label: "Current branch (feature/current)",
        detail: "Currently checked out in the project root",
        source: "current",
        selection: {
          kind: "current_branch",
          ref: "feature/current",
          displayName: "Current branch (feature/current)",
        },
      },
    ];
    loadBranchBaseOptionsMock.mockResolvedValue({
      options: currentBranchOptions,
      selectedKey: "project_default:main",
    });
    resetAgentSessionState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options: currentBranchOptions,
          selectedKey: "current_branch:feature/current",
          loadedAt: "2026-05-08T00:00:00.000Z",
        },
      },
      lastBranchBaseSelectionByProjectId: {
        "project-1": "current_branch:feature/current",
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
        "Current branch (feature/current)"
      )
    );
    await userEvent.click(screen.getByTestId("agents-start-base"));
    const isolatedSwitch = screen.getByRole("switch", {
      name: /Use isolated branch/i,
    });
    expect(isolatedSwitch).toBeDisabled();
    expect(isolatedSwitch).toHaveAttribute("aria-checked", "true");

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "start from current branch" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          content: "start from current branch",
          base: expect.objectContaining({
            kind: "current_branch",
            branchMode: "isolated",
            ref: "feature/current",
          }),
        })
      )
    );
  });

  it("keeps the project default base when a ticket reference has linked PRs", async () => {
    mockAgentViewData();
    getTicketAssociationsMock.mockResolvedValue({
      tasks: [],
      proposals: [],
      sessions: [],
      conversations: [],
      pullRequests: [
        {
          id: "https://github.com/owner/repo/pull/88",
          title: "PR #88",
          subtitle: "feature/ticket-pr",
          status: "open",
          active: true,
          deepLink: {
            view: "agents",
            id: "conversation-ticket",
            projectId: "project-1",
          },
          branchName: "feature/ticket-pr",
          baseRef: "main",
          prNumber: 88,
          prUrl: "https://github.com/owner/repo/pull/88",
        },
      ],
      checks: [],
      qa: [],
      specs: [],
    });
    useAgentSessionStore.getState().setStartConversationDraft({
      projectId: "project-1",
      content: "continue the ticket work",
      mode: "edit",
      composerIntegrationReferences: [
        {
          provider: "atlassian",
          kind: "jira",
          id: "10088",
          key: "RX-88",
          title: "Ticket with PR",
        },
      ],
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
        "Project default (main)"
      )
    );
    expect(getTicketAssociationsMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          content: "continue the ticket work",
          base: expect.objectContaining({
            kind: "project_default",
            branchMode: "isolated",
            ref: "main",
            displayName: "Project default (main)",
          }),
          composerIntegrationReferences: [
            expect.objectContaining({
              provider: "atlassian",
              kind: "jira",
              id: "10088",
              key: "RX-88",
            }),
          ],
        })
      )
    );
  });

  it("preserves a remembered branch base when a ticket reference is attached", async () => {
    mockAgentViewData();
    const branchOptions: BranchBaseOption[] = [
      {
        key: "project_default:main",
        label: "Project default (main)",
        detail: "Configured project base branch",
        source: "project",
        selection: {
          kind: "project_default",
          ref: "main",
          displayName: "Project default (main)",
        },
      },
      {
        key: "local_branch:develop",
        label: "develop",
        detail: "Local branch",
        source: "local",
        selection: {
          kind: "local_branch",
          ref: "develop",
          displayName: "develop",
        },
      },
    ];
    resetAgentSessionState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options: branchOptions,
          selectedKey: "local_branch:develop",
          loadedAt: "2026-05-08T00:00:00.000Z",
        },
      },
      lastBranchBaseSelectionByProjectId: {
        "project-1": "local_branch:develop",
      },
    });
    useAgentSessionStore.getState().setStartConversationDraft({
      projectId: "project-1",
      content: "continue without a linked PR",
      mode: "plan",
      composerIntegrationReferences: [
        {
          provider: "linear",
          kind: "linear",
          id: "lin-99",
          key: "ENG-99",
          title: "Ticket without PR",
        },
      ],
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent("develop")
    );
    expect(getTicketAssociationsMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          content: "continue without a linked PR",
          mode: "plan",
          base: expect.objectContaining({
            kind: "local_branch",
            branchMode: "isolated",
            ref: "develop",
            displayName: "develop",
          }),
          composerIntegrationReferences: [
            expect.objectContaining({
              provider: "linear",
              kind: "linear",
              id: "lin-99",
              key: "ENG-99",
            }),
          ],
        })
      )
    );
  });

  it("forces isolated branch mode for Review PR starts with PR metadata", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    loadPullRequestBaseOptionsMock.mockResolvedValue([prPickerBranchOption()]);

    renderAgentsView();

    await user.click(screen.getByTestId("agents-start-mode-chip"));
    await user.click(screen.getByTestId("agents-start-mode-review_pr"));
    await user.click(await screen.findByTestId("agents-start-base"));
    await user.click(screen.getByRole("tab", { name: /PRs/i }));
    await user.click(await screen.findByText("#42 Add PR picker"));
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent("#42 Add PR picker")
    );

    const isolatedSwitch = screen.getByRole("switch", {
      name: /Use isolated branch/i,
    });
    expect(isolatedSwitch).toBeDisabled();
    expect(isolatedSwitch).toHaveAttribute("aria-checked", "true");

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "review selected PR" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          mode: "review_pr",
          content: "review selected PR",
          base: expect.objectContaining({
            kind: "local_branch",
            branchMode: "isolated",
            ref: "feature/pr-picker",
            sourcePullRequest: expect.objectContaining({
              number: 42,
              headRefName: "feature/pr-picker",
            }),
          }),
        })
      )
    );
  });

  it("blocks Review PR starts without PR metadata", async () => {
    const user = userEvent.setup();
    mockAgentViewData();

    renderAgentsView();

    await user.click(screen.getByTestId("agents-start-mode-chip"));
    await user.click(screen.getByTestId("agents-start-mode-review_pr"));
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-mode-chip")).toHaveTextContent(
        "Review PR"
      )
    );
    await user.type(
      screen.getByTestId("agents-start-textarea"),
      "review without a PR"
    );
    await user.click(screen.getByTestId("agents-start-submit"));

    expect(await screen.findByText("Select a pull request to review.")).toBeInTheDocument();
    expect(startAgentConversationMock).not.toHaveBeenCalled();
  });

  it("keeps a selected pull request visible across later start-from searches", async () => {
    const user = userEvent.setup();
    let pullRequestSearches = 0;
    mockAgentViewData();
    loadPullRequestBaseOptionsMock.mockImplementation(() => {
      pullRequestSearches += 1;
      return Promise.resolve(
        pullRequestSearches === 1 ? [prPickerBranchOption()] : []
      );
    });

    renderAgentsView();

    await user.click(await screen.findByTestId("agents-start-base"));
    await user.click(screen.getByRole("tab", { name: /PRs/i }));
    await user.click(await screen.findByText("#42 Add PR picker"));
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent("#42 Add PR picker")
    );

    await user.type(screen.getByPlaceholderText(/Search pull requests/i), "missing");

    await waitFor(() => expect(pullRequestSearches).toBe(2));
    expect(screen.getAllByText("#42 Add PR picker").length).toBeGreaterThan(1);
  });

  it("shows pull request search failures in the start composer", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    loadPullRequestBaseOptionsMock.mockRejectedValue(
      new Error("GitHub search failed")
    );

    renderAgentsView();

    await user.click(await screen.findByTestId("agents-start-base"));
    await user.click(screen.getByRole("tab", { name: /PRs/i }));

    expect(await screen.findByText("GitHub search failed")).toBeInTheDocument();
  });

  it("paints the conversation shell before the no-file agent start resolves", async () => {
    mockAgentViewData();
    const resolvedConversation = conversation({
      id: "conversation-resolved-no-file",
      contextId: "project-1",
      title: null,
    });
    let resolveStart:
      | ((value: Awaited<ReturnType<typeof startAgentConversationMock>>) => void)
      | null = null;
    startAgentConversationMock.mockReturnValue(
      new Promise((resolve) => {
        resolveStart = resolve;
      })
    );

    const { queryClient } = renderAgentsView();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "start without waiting" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    const optimisticConversationId =
      useAgentSessionStore.getState().selectedConversationId;
    expect(optimisticConversationId).toMatch(/^optimistic-conversation:/);
    expect(integratedChatPanelRenderMock).toHaveBeenLastCalledWith(
      expect.objectContaining({
        conversationIdOverride: optimisticConversationId,
        storeContextKeyOverride: `project:${optimisticConversationId}`,
        agentProcessContextIdOverride: optimisticConversationId,
        sendOptions: expect.objectContaining({
          conversationId: optimisticConversationId,
        }),
      })
    );
    expect(
      queryClient.getQueryData(["chat", "conversations", optimisticConversationId])
    ).toEqual({
      conversation: expect.objectContaining({
        id: optimisticConversationId,
        contextType: "project",
        contextId: "project-1",
      }),
      messages: [
        expect.objectContaining({
          conversationId: optimisticConversationId,
          role: "user",
          content: "start without waiting",
        }),
      ],
    });
    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          content: "start without waiting",
        })
      )
    );
    expect(createConversationMock).not.toHaveBeenCalled();
    expect(startAgentConversationMock.mock.calls[0]?.[0]).not.toHaveProperty(
      "conversationId"
    );

    resolveStart?.({
      conversation: resolvedConversation,
      workspace: conversationWorkspace({
        conversationId: "conversation-resolved-no-file",
      }),
      sendResult: {
        conversationId: "conversation-resolved-no-file",
        agentRunId: "run-resolved-no-file",
        isNewConversation: true,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });

    await waitFor(() =>
      expect(useAgentSessionStore.getState().selectedConversationId).toBe(
        "conversation-resolved-no-file"
      )
    );
  });

  it("renders a queued starter prompt and paused explanation when global execution is paused", async () => {
    mockAgentViewData();
    useUiStore.getState().setExecutionPaused(true);
    const queuedPrompt = `build ${"queued feature ".repeat(40)}final`;
    const pausedConversation = conversation({
      id: "conversation-paused",
      contextId: "project-1",
      title: null,
    });
    startAgentConversationMock.mockResolvedValue({
      conversation: pausedConversation,
      workspace: conversationWorkspace({
        conversationId: "conversation-paused",
      }),
      sendResult: {
        conversationId: "conversation-paused",
        agentRunId: "",
        isNewConversation: false,
        wasQueued: true,
        queuedAsPending: false,
        queuedMessageId: "queued-paused-start",
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-paused-banner")).toHaveTextContent(
        "Execution is paused"
      )
    );
    expect(screen.getByTestId("agents-start-submit")).toHaveTextContent("Queue Prompt");

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: queuedPrompt },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          content: queuedPrompt,
        })
      )
    );
    expect(createConversationMock).not.toHaveBeenCalled();
    expect(startAgentConversationMock.mock.calls[0]?.[0]).not.toHaveProperty(
      "conversationId"
    );
    await waitFor(() =>
      expect(useChatStore.getState().queuedMessages["project:conversation-paused"]).toEqual([
        expect.objectContaining({
          id: "queued-paused-start",
          content: queuedPrompt,
          isEditing: false,
        }),
      ])
    );

    const queuedEmptyState = await screen.findByTestId("agents-paused-queued-empty-state");
    expect(queuedEmptyState).toHaveTextContent("Execution is paused");
    expect(queuedEmptyState).toHaveTextContent(
      "This prompt will start when execution resumes."
    );
    const queuedPromptPreview = screen.getByTestId("agents-paused-queued-prompt");
    expect(queuedPromptPreview).not.toHaveTextContent(queuedPrompt);
    expect(queuedPromptPreview.textContent).toMatch(/^build queued feature/);
    expect(queuedPromptPreview.textContent).toMatch(/\.\.\.$/);
  });

  it("starts with the remembered runtime when the project has a valid runtime preference", async () => {
    mockAgentViewData();
    resetAgentSessionState({
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "claude",
          modelId: "opus",
          effort: "high",
        },
      },
    });

    renderAgentsView();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "use the remembered runtime" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          providerHarness: "claude",
          modelId: "opus",
          logicalEffort: "high",
        })
      )
    );
  });

  it("starts with a remembered GPT-5.6 runtime when Codex reports the model alias", async () => {
    mockAgentViewData();
    const baseSettings = agentProviderSettings();
    mockHarnessProviders(
      agentProviderSettings({
        providers: [
          {
            ...baseSettings.providers[0]!,
            supportedEfforts: ["low", "medium", "high", "xhigh", "max", "ultra"],
            supportedModelAliases: ["gpt-5.6-terra"],
          },
          baseSettings.providers[1]!,
        ],
      }),
    );
    resetAgentSessionState({
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "codex",
          modelId: "gpt-5.6-terra",
          effort: "ultra",
        },
      },
    });

    renderAgentsView();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "use remembered GPT-5.6 runtime" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          providerHarness: "codex",
          modelId: "gpt-5.6-terra",
          logicalEffort: "ultra",
        })
      )
    );
  });

  it("does not overwrite a remembered GPT-5.6 runtime when aliases are unavailable", async () => {
    mockAgentViewData();
    resetAgentSessionState({
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "codex",
          modelId: "gpt-5.6-terra",
          effort: "ultra",
        },
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveTextContent(
        "gpt-5.5"
      )
    );
    expect(useAgentSessionStore.getState().lastRuntimeByProjectId["project-1"]).toEqual({
      provider: "codex",
      modelId: "gpt-5.6-terra",
      effort: "ultra",
    });
  });

  it("revalidates stale GPT-5.6 runtime aliases before starting", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: 0 },
        mutations: { retry: false },
      },
    });
    const seededConversation = conversation({
      id: "conversation-stale-gpt56",
      contextId: "project-1",
      title: null,
    });
    startAgentConversationMock.mockResolvedValue({
      conversation: seededConversation,
      workspace: conversationWorkspace({
        conversationId: "conversation-stale-gpt56",
      }),
      sendResult: {
        conversationId: "conversation-stale-gpt56",
        agentRunId: "run-stale-gpt56",
        isNewConversation: true,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(
      () =>
        useStartAgentConversation({
          handleAutoManagedTitle: vi.fn(),
          invalidateProjectConversations: vi.fn().mockResolvedValue(undefined),
          queryClient,
          selectConversation: vi.fn(),
          setActiveConversation: useChatStore.getState().setActiveConversation,
          setFocusedProject: vi.fn(),
          setOptimisticConversationsById: vi.fn(),
          setOptimisticSelectedConversationId: vi.fn(),
          setOptimisticWorkspacesByConversationId: vi.fn(),
          setRuntimeForConversation: vi.fn(),
        }),
      { wrapper }
    );

    await result.current({
      projectId: "project-1",
      content: "start after aliases changed",
      runtime: {
        provider: "codex",
        modelId: "gpt-5.6-terra",
        effort: "ultra",
      },
      runtimeProviderContext: {
        supportedEfforts: ["low", "medium", "high", "xhigh"],
        supportedModelAliases: ["gpt-5.5"],
      },
      mode: "edit",
      base: null,
      files: [],
    });

    expect(startAgentConversationMock).toHaveBeenCalledWith(
      expect.objectContaining({
        providerHarness: "codex",
        modelId: "gpt-5.5",
        logicalEffort: "xhigh",
      })
    );
  });

  it("falls back to the default runtime when the remembered provider is no longer valid", async () => {
    mockAgentViewData();
    resetAgentSessionState({
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "removed-provider" as never,
          modelId: "retired-model",
          effort: "high",
        },
      },
    });

    renderAgentsView();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "recover runtime defaults" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
        })
      )
    );
  });

  it("falls back to an enabled and validated provider when the remembered provider is unavailable", async () => {
    mockAgentViewData();
    mockHarnessProviders(
      agentProviderSettings({
        defaultProvider: "claude",
        providers: [
          {
            ...agentProviderSettings().providers[0]!,
            enabled: false,
            isDefault: false,
            available: false,
            binaryFound: true,
            status: "Codex is disabled",
            error: null,
          },
          {
            ...agentProviderSettings().providers[1]!,
            enabled: true,
            isDefault: true,
            available: true,
          },
        ],
      }),
    );
    resetAgentSessionState({
      lastRuntimeByProjectId: {
        "project-1": {
          provider: "codex",
          modelId: "gpt-5.5",
          effort: "xhigh",
        },
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveTextContent(
        "sonnet",
      )
    );
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "use available provider" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          providerHarness: "claude",
          modelId: "sonnet",
          logicalEffort: "medium",
        })
      )
    );
  });

  it("disables unavailable providers in the starter runtime selector", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    mockHarnessProviders(
      agentProviderSettings({
        providers: [
          agentProviderSettings().providers[0]!,
          {
            ...agentProviderSettings().providers[1]!,
            enabled: false,
            available: false,
            binaryFound: false,
            status: "Claude CLI not found",
            error: "Claude CLI not found",
          },
        ],
      }),
    );

    renderAgentsView();

    await user.click(screen.getByTestId("agent-composer-runtime-pill"));
    await user.click(screen.getByTestId("agent-composer-runtime-provider-claude"));

    expect(screen.getByText("Claude is not enabled")).toBeInTheDocument();
    expect(screen.getByText("Enable this provider in settings to use its models.")).toBeInTheDocument();
  });

  it("blocks new agent runs when no provider is enabled and validated", async () => {
    mockAgentViewData();
    mockHarnessProviders(
      agentProviderSettings({
        defaultProvider: null,
        requiresOnboarding: true,
        providers: agentProviderSettings().providers.map((providerSetting) => ({
          ...providerSetting,
          enabled: false,
          available: false,
          isDefault: false,
          status: `${providerSetting.provider} disabled`,
          error: null,
        })),
      }),
    );

    renderAgentsView();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "should not start" },
    });

    expect(screen.getByTestId("agents-start-submit")).toBeDisabled();
    expect(screen.getByTestId("agents-start-provider-status")).toHaveTextContent(
      "Enable in Settings.",
    );
    fireEvent.click(screen.getByTestId("agents-start-provider-status-settings"));

    expect(startAgentConversationMock).not.toHaveBeenCalled();
    expect(useUiStore.getState().activeModal).toBe("settings");
    expect(useUiStore.getState().modalContext).toEqual({ section: "providers" });
  });

  it("remembers runtime changes made on the starter composer before creating a conversation", async () => {
    mockAgentViewData();

    renderAgentsView();

    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    await userEvent.click(screen.getByTestId("agent-composer-runtime-provider-claude"));
    await userEvent.click(screen.getByTestId("agents-start-model-opus"));
    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    await userEvent.click(screen.getByTestId("agents-start-effort-max"));

    await waitFor(() =>
      expect(useAgentSessionStore.getState().lastRuntimeByProjectId["project-1"]).toEqual({
        provider: "claude",
        modelId: "opus",
        effort: "max",
      })
    );

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "persist this runtime" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          providerHarness: "claude",
          modelId: "opus",
          logicalEffort: "max",
        })
      )
    );
  });

  it("shows Fable in the starter model selector when refreshed Claude capabilities report it", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    const snapshotSettings = agentProviderSettings();
    const refreshedSettings = agentProviderSettings({
      providers: [
        snapshotSettings.providers[0]!,
        {
          ...snapshotSettings.providers[1]!,
          supportedModelAliases: ["sonnet", "opus", "haiku", "fable"],
          supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
        },
      ],
    });
    useHarnessProvidersMock.mockImplementation(
      (options?: { refreshRuntime?: boolean }) => {
        const settings = options?.refreshRuntime ? refreshedSettings : snapshotSettings;
        return {
          settings,
          providers: settings.providers,
          isLoading: false,
          isPlaceholderData: false,
          isError: false,
          error: null,
          refetchProviders: vi.fn(),
          updateProviderAsync: vi.fn(),
          isUpdating: false,
          updateError: null,
        };
      }
    );

    renderAgentsView();

    await user.click(screen.getByTestId("agent-composer-runtime-pill"));
    await user.click(screen.getByTestId("agent-composer-runtime-provider-claude"));

    expect(useHarnessProvidersMock).toHaveBeenCalledWith({ refreshRuntime: true });
    expect(await screen.findByTestId("agents-start-model-fable")).toBeInTheDocument();

    await user.click(screen.getByTestId("agents-start-model-fable"));

    expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveTextContent("fable");
  });

  it("shows manage models link in the runtime selector popover", async () => {
    mockAgentViewData();

    renderAgentsView();

    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));

    expect(screen.getByText("Manage models in Settings")).toBeInTheDocument();
  });

  it("paints the attachment-backed conversation shell after seeding before the heavy agent start resolves", async () => {
    mockAgentViewData();
    const seededConversation = conversation({
      id: "conversation-seeded",
      contextId: "project-1",
      title: null,
    });
    let resolveStart:
      | ((value: Awaited<ReturnType<typeof startAgentConversationMock>>) => void)
      | null = null;
    createConversationMock.mockResolvedValue(seededConversation);
    startAgentConversationMock.mockReturnValue(
      new Promise((resolve) => {
        resolveStart = resolve;
      })
    );
    vi.mocked(invoke).mockResolvedValue({ id: "attachment-seeded" });

    const { queryClient } = renderAgentsView();

    fireEvent.change(screen.getByTestId("attachment-file-input"), {
      target: {
        files: [new File(["patch"], "change.txt", { type: "text/plain" })],
      },
    });
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "fix agent landing flow" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(createConversationMock).toHaveBeenCalledWith("project", "project-1")
    );
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    expect(
      queryClient.getQueryData(["chat", "conversations", "conversation-seeded"])
    ).toEqual({
      conversation: expect.objectContaining({ id: "conversation-seeded" }),
      messages: [
        expect.objectContaining({
          conversationId: "conversation-seeded",
          role: "user",
          content: "fix agent landing flow",
        }),
      ],
    });
    expect(
      useChatStore.getState().agentStatus["project:conversation-seeded"]
    ).toBe("generating");
    expect(
      useChatStore.getState().isSending["project:conversation-seeded"]
    ).toBe(true);
    expect(
      useChatStore.getState().agentActivityLabels["project:conversation-seeded"]
    ).toBe("Setup workspace");
    expect(
      useAgentSessionStore.getState().runtimeByConversationId["conversation-seeded"]
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "xhigh",
    });
    expect(useAgentSessionStore.getState().selectedConversationId).toBe(
      "conversation-seeded"
    );
    expect(integratedChatPanelRenderMock).toHaveBeenLastCalledWith(
      expect.objectContaining({
        conversationIdOverride: "conversation-seeded",
        storeContextKeyOverride: "project:conversation-seeded",
        agentProcessContextIdOverride: "conversation-seeded",
        sendOptions: expect.objectContaining({
          conversationId: "conversation-seeded",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
        }),
      })
    );
    expect(startAgentConversationMock).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: "conversation-seeded",
        content: "fix agent landing flow",
      })
    );

    resolveStart?.({
      conversation: seededConversation,
      workspace: conversationWorkspace({
        conversationId: "conversation-seeded",
      }),
      sendResult: {
        conversationId: "conversation-seeded",
        agentRunId: "run-seeded",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });

    await waitFor(() =>
      expect(spawnConversationSessionNamerMock).toHaveBeenCalledWith(
        "conversation-seeded",
        "fix agent landing flow",
        "codex"
      )
    );
  });

  it("stores selected references on the first optimistic message before a conversation exists", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: Infinity },
        mutations: { retry: false },
      },
    });
    const seededConversation = conversation({
      id: "conversation-seeded-references",
      contextId: "project-1",
      title: null,
    });
    let resolveStart:
      | ((value: Awaited<ReturnType<typeof startAgentConversationMock>>) => void)
      | null = null;
    startAgentConversationMock.mockReturnValue(
      new Promise((resolve) => {
        resolveStart = resolve;
      })
    );
    const setOptimisticSelectedConversationId = vi.fn();
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(
      () =>
        useStartAgentConversation({
          handleAutoManagedTitle: vi.fn(),
          invalidateProjectConversations: vi.fn().mockResolvedValue(undefined),
          queryClient,
          selectConversation: vi.fn(),
          setActiveConversation: useChatStore.getState().setActiveConversation,
          setFocusedProject: vi.fn(),
          setOptimisticConversationsById: vi.fn(),
          setOptimisticSelectedConversationId,
          setOptimisticWorkspacesByConversationId: vi.fn(),
          setRuntimeForConversation: vi.fn(),
        }),
      { wrapper }
    );

    const startPromise = result.current({
      projectId: "project-1",
      content: "start with references",
      runtime: {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "xhigh",
      },
      mode: "edit",
      base: null,
      files: [],
      composerProjectReferences: [{ path: "src/main.ts", kind: "file" }],
      composerIntegrationReferences: [
        {
          provider: "atlassian",
          kind: "jira",
          id: "RX-42",
          key: "RX-42",
          title: "Fix composer references",
        },
      ],
      composerArtifactReferences: [
        {
          kind: "plan",
          artifactId: "artifact-1",
          title: "Implementation Plan",
        },
      ],
    });

    await waitFor(() =>
      expect(setOptimisticSelectedConversationId).toHaveBeenCalled()
    );
    const optimisticConversationId =
      setOptimisticSelectedConversationId.mock.calls[0]?.[0];
    const optimisticMessage = queryClient.getQueryData<{
      messages: Array<{ metadata: string | null }>;
    }>(["chat", "conversations", optimisticConversationId])?.messages[0];
    expect(JSON.parse(optimisticMessage?.metadata ?? "{}")).toEqual({
      composer_project_references: [{ path: "src/main.ts", kind: "file" }],
      composer_integration_references: [
        {
          provider: "atlassian",
          kind: "jira",
          id: "RX-42",
          key: "RX-42",
          title: "Fix composer references",
        },
      ],
      composer_artifact_references: [
        {
          kind: "plan",
          artifactId: "artifact-1",
          title: "Implementation Plan",
        },
      ],
    });

    expect(createConversationMock).not.toHaveBeenCalled();
    resolveStart?.({
      conversation: seededConversation,
      workspace: conversationWorkspace({
        conversationId: "conversation-seeded-references",
      }),
      sendResult: {
        conversationId: "conversation-seeded-references",
        agentRunId: "run-seeded-references",
        isNewConversation: true,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });
    await startPromise;
  });

  it("invalidates the resolved Jira issue query after starting with a Jira reference", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: 0 },
        mutations: { retry: false },
      },
    });
    const invalidateQueriesSpy = vi.spyOn(queryClient, "invalidateQueries");
    const seededConversation = conversation({
      id: "conversation-with-jira",
      contextId: "project-1",
      title: null,
    });
    createConversationMock.mockResolvedValue(seededConversation);
    startAgentConversationMock.mockResolvedValue({
      conversation: seededConversation,
      workspace: conversationWorkspace({
        conversationId: "conversation-with-jira",
      }),
      sendResult: {
        conversationId: "conversation-with-jira",
        agentRunId: "run-with-jira",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const onJiraLinked = vi.fn();
    const { result } = renderHook(
      () =>
        useStartAgentConversation({
          handleAutoManagedTitle: vi.fn(),
          invalidateProjectConversations: vi.fn().mockResolvedValue(undefined),
          queryClient,
          selectConversation: vi.fn(),
          setActiveConversation: useChatStore.getState().setActiveConversation,
          setFocusedProject: vi.fn(),
          setOptimisticConversationsById: vi.fn(),
          setOptimisticSelectedConversationId: vi.fn(),
          setOptimisticWorkspacesByConversationId: vi.fn(),
          setRuntimeForConversation: vi.fn(),
          onJiraLinked,
        }),
      { wrapper }
    );

    await result.current({
      projectId: "project-1",
      content: "start with jira",
      runtime: {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "xhigh",
      },
      mode: "edit",
      base: null,
      files: [],
      composerIntegrationReferences: [
        {
          provider: "atlassian",
          kind: "jira",
          id: "RX-42",
          key: "RX-42",
          title: "Fix composer references",
        },
      ],
    });

    expect(invalidateQueriesSpy).toHaveBeenCalledWith({
      queryKey: agentJiraIssueKeys.issue("conversation-with-jira"),
    });
    expect(onJiraLinked).toHaveBeenCalledWith("conversation-with-jira");
    expect(
      useAgentSessionStore.getState().artifactByConversationId["conversation-with-jira"]
    ).toEqual(
      expect.objectContaining({
        isOpen: true,
        activeTab: "jira",
      })
    );
  });

  it("opens and invalidates the Linear tab after starting with a Linear reference", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: 0 },
        mutations: { retry: false },
      },
    });
    const invalidateQueriesSpy = vi.spyOn(queryClient, "invalidateQueries");
    const seededConversation = conversation({
      id: "conversation-with-linear",
      contextId: "project-1",
      title: null,
    });
    createConversationMock.mockResolvedValue(seededConversation);
    startAgentConversationMock.mockResolvedValue({
      conversation: seededConversation,
      workspace: conversationWorkspace({
        conversationId: "conversation-with-linear",
      }),
      sendResult: {
        conversationId: "conversation-with-linear",
        agentRunId: "run-with-linear",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const onLinearLinked = vi.fn();
    const { result } = renderHook(
      () =>
        useStartAgentConversation({
          handleAutoManagedTitle: vi.fn(),
          invalidateProjectConversations: vi.fn().mockResolvedValue(undefined),
          queryClient,
          selectConversation: vi.fn(),
          setActiveConversation: useChatStore.getState().setActiveConversation,
          setFocusedProject: vi.fn(),
          setOptimisticConversationsById: vi.fn(),
          setOptimisticSelectedConversationId: vi.fn(),
          setOptimisticWorkspacesByConversationId: vi.fn(),
          setRuntimeForConversation: vi.fn(),
          onLinearLinked,
        }),
      { wrapper }
    );

    await result.current({
      projectId: "project-1",
      content: "start with linear",
      runtime: {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "xhigh",
      },
      mode: "edit",
      base: null,
      files: [],
      composerIntegrationReferences: [
        {
          provider: "linear",
          kind: "linear",
          id: "LIN-42",
          key: "LIN-42",
          title: "Fix composer references",
        },
      ],
    });

    expect(invalidateQueriesSpy).toHaveBeenCalledWith({
      queryKey: agentLinearIssueKeys.issue("conversation-with-linear"),
    });
    expect(onLinearLinked).toHaveBeenCalledWith("conversation-with-linear");
    expect(
      useAgentSessionStore.getState().artifactByConversationId["conversation-with-linear"]
    ).toEqual(
      expect.objectContaining({
        isOpen: true,
        activeTab: "linear",
      })
    );
  });

  it("hides the attachment-backed seeded draft when the seeded agent start fails", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: 0 },
        mutations: { retry: false },
      },
    });
    const seededConversation = conversation({
      id: "conversation-failed-start",
      contextId: "project-1",
      title: null,
    });
    createConversationMock.mockResolvedValue(seededConversation);
    startAgentConversationMock.mockRejectedValue(new Error("backend unavailable"));
    vi.mocked(invoke).mockResolvedValue({ id: "attachment-failed-start" });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(
      () =>
        useStartAgentConversation({
          handleAutoManagedTitle: vi.fn(),
          invalidateProjectConversations: vi.fn().mockResolvedValue(undefined),
          queryClient,
          selectConversation: vi.fn(),
          setActiveConversation: useChatStore.getState().setActiveConversation,
          setFocusedProject: vi.fn(),
          setOptimisticConversationsById: vi.fn(),
          setOptimisticSelectedConversationId: vi.fn(),
          setOptimisticWorkspacesByConversationId: vi.fn(),
          setRuntimeForConversation: vi.fn(),
        }),
      { wrapper }
    );

    await expect(
      result.current({
        projectId: "project-1",
        content: "start then fail",
        runtime: {
          provider: "codex",
          modelId: "gpt-5.5",
          effort: "xhigh",
        },
        mode: "edit",
        base: null,
        files: [new File(["draft"], "draft.txt", { type: "text/plain" })],
      })
    ).rejects.toThrow("backend unavailable");

    expect(createConversationMock).toHaveBeenCalledWith("project", "project-1");
    expect(invoke).toHaveBeenCalledWith("upload_chat_attachment", {
      input: expect.objectContaining({
        conversationId: "conversation-failed-start",
        fileName: "draft.txt",
      }),
    });
    expect(
      queryClient.getQueryData(["chat", "conversations", "conversation-failed-start"])
    ).toBeUndefined();
    expect(
      useChatStore.getState().agentStatus["project:conversation-failed-start"]
    ).toBeUndefined();
    expect(
      useChatStore.getState().isSending["project:conversation-failed-start"]
    ).toBeUndefined();
    expect(
      useChatStore.getState().agentActivityLabels["project:conversation-failed-start"]
    ).toBeUndefined();
  });

  it("moves running state when the backend resolves a different conversation id", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: 0 },
        mutations: { retry: false },
      },
    });
    const handleAutoManagedTitle = vi.fn();
    createConversationMock.mockResolvedValue(
      conversation({
        id: "conversation-seeded-remap",
        contextId: "project-1",
        title: null,
      })
    );
    startAgentConversationMock.mockResolvedValue({
      conversation: conversation({
        id: "conversation-resolved-remap",
        contextId: "project-1",
        title: null,
      }),
      workspace: conversationWorkspace({
        conversationId: "conversation-resolved-remap",
      }),
      sendResult: {
        conversationId: "conversation-resolved-remap",
        agentRunId: "run-remap",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    let optimisticSelectedConversationId: string | null = null;
    const setOptimisticSelectedConversationId = vi.fn(
      (next: SetStateAction<string | null>) => {
        optimisticSelectedConversationId =
          typeof next === "function"
            ? next(optimisticSelectedConversationId)
            : next;
      }
    );
    const { result } = renderHook(
      () =>
        useStartAgentConversation({
          handleAutoManagedTitle,
          invalidateProjectConversations: vi.fn().mockResolvedValue(undefined),
          queryClient,
          selectConversation: vi.fn(),
          setActiveConversation: useChatStore.getState().setActiveConversation,
          setFocusedProject: vi.fn(),
          setOptimisticConversationsById: vi.fn(),
          setOptimisticSelectedConversationId,
          setOptimisticWorkspacesByConversationId: vi.fn(),
          setRuntimeForConversation: vi.fn(),
        }),
      { wrapper }
    );

    await result.current({
      projectId: "project-1",
      content: "start then remap",
      runtime: {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "xhigh",
      },
      mode: "edit",
      base: null,
      files: [],
    });

    expect(
      useChatStore.getState().agentStatus["project:conversation-seeded-remap"]
    ).toBeUndefined();
    expect(
      useChatStore.getState().isSending["project:conversation-seeded-remap"]
    ).toBeUndefined();
    expect(
      useChatStore.getState().agentStatus["project:conversation-resolved-remap"]
    ).toBe("generating");
    expect(
      queryClient.getQueryData(["chat", "conversations", "conversation-resolved-remap"])
    ).toEqual({
      conversation: expect.objectContaining({ id: "conversation-resolved-remap" }),
      messages: [
        expect.objectContaining({
          conversationId: "conversation-resolved-remap",
          role: "user",
          content: "start then remap",
        }),
      ],
    });
    expect(
      queryClient.getQueryData(
        chatKeys.conversationSummary("conversation-resolved-remap")
      )
    ).toEqual(expect.objectContaining({ id: "conversation-resolved-remap" }));
    expect(optimisticSelectedConversationId).toBe(
      "conversation-resolved-remap"
    );
    expect(handleAutoManagedTitle).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: "conversation-resolved-remap",
        content: "start then remap",
      })
    );
  });

  it("falls back to the project default when the remembered branch selection is empty", async () => {
    mockAgentViewData();
    resetAgentSessionState({
      lastBranchBaseSelectionByProjectId: {
        "project-1": "",
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );
    expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
      "Project default (main)"
    );
  });

  it("renders remembered branch base options before hover and refreshes with a loading state on intent", async () => {
    mockAgentViewData();
    let resolveBranchOptions:
      | ((result: { options: BranchBaseOption[]; selectedKey: string }) => void)
      | null = null;
    loadBranchBaseOptionsMock.mockReturnValue(
      new Promise((resolve) => {
        resolveBranchOptions = resolve;
      })
    );
    resetAgentSessionState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options: [
            {
              key: "project_default:main",
              label: "Project default (main)",
              detail: "Configured project base branch",
              source: "project",
              selection: {
                kind: "project_default",
                ref: "main",
                displayName: "Project default (main)",
              },
            },
            {
              key: "local_branch:feature/cached",
              label: "feature/cached",
              detail: "Local branch",
              source: "local",
              selection: {
                kind: "local_branch",
                ref: "feature/cached",
                displayName: "feature/cached",
              },
            },
          ],
          selectedKey: "local_branch:feature/cached",
          loadedAt: "2026-05-08T00:00:00.000Z",
        },
      },
      lastBranchBaseSelectionByProjectId: {
        "project-1": "local_branch:feature/cached",
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );
    await new Promise((resolve) => window.setTimeout(resolve, 0));

    expect(screen.getByTestId("agents-start-base")).toHaveTextContent("feature/cached");
    expect(loadBranchBaseOptionsMock).not.toHaveBeenCalled();
    expect(listIdeationSessionsMock).not.toHaveBeenCalled();
    expect(listConversationsMock).not.toHaveBeenCalled();
    expect(listAgentConversationWorkspacesByProjectMock).not.toHaveBeenCalled();

    await userEvent.click(screen.getByTestId("agents-start-base"));

    await waitFor(() =>
      expect(loadBranchBaseOptionsMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          workingDirectory: "/tmp/ralphx",
        })
      )
    );
    expect(screen.getByText("Refreshing branches...")).toBeInTheDocument();
    expect(screen.getAllByText("feature/cached").length).toBeGreaterThan(0);

    await userEvent.click(screen.getByText("Project default (main)"));
    expect(
      useAgentSessionStore.getState().lastBranchBaseSelectionByProjectId["project-1"]
    ).toBe("project_default:main");
    expect(
      useAgentSessionStore.getState().branchBaseCacheByProjectId["project-1"]?.selectedKey
    ).toBe("project_default:main");

    resolveBranchOptions?.({
      options: [
        {
          key: "project_default:main",
          label: "Project default (main)",
          detail: "Configured project base branch",
          source: "project",
          selection: {
            kind: "project_default",
            ref: "main",
            displayName: "Project default (main)",
          },
        },
      ],
      selectedKey: "project_default:main",
    });
  });

  it("starts a chat-mode conversation from the selected base and shows its workspace", async () => {
    mockAgentViewData();
    startAgentConversationMock.mockResolvedValue({
      conversation: conversation({
        id: "conversation-chat",
        contextId: "project-1",
        title: "Branch question",
        agentMode: "chat",
      }),
      workspace: {
        conversationId: "conversation-chat",
        projectId: "project-1",
        mode: "chat",
        baseRefKind: "project_default",
        baseRef: "main",
        baseDisplayName: "Project default (main)",
        baseCommit: null,
        branchName: "ralphx/demo/agent-conversation-chat",
        worktreePath: "/tmp/ralphx/conversation-chat",
        linkedIdeationSessionId: null,
        linkedPlanBranchId: null,
        publicationPrNumber: null,
        publicationPrUrl: null,
        publicationPrStatus: null,
        publicationPushStatus: null,
        status: "active",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      sendResult: {
        conversationId: "conversation-chat",
        agentRunId: "run-chat",
        isNewConversation: true,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });

    renderAgentsView();

    await userEvent.click(screen.getByTestId("agents-start-mode-chip"));
    await userEvent.click(screen.getByTestId("agents-start-mode-chat"));
    expect(screen.getByTestId("agents-start-base")).toBeInTheDocument();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "what branch am I on?" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          content: "what branch am I on?",
          mode: "chat",
          base: expect.objectContaining({
            kind: "project_default",
            ref: "main",
          }),
        })
      )
    );
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    expect(screen.getByTestId("agents-conversation-workspace-line")).toHaveTextContent(
      "agent-conversation-chat"
    );
  });

  it("starts automation mode by creating a bound setup conversation before sending", async () => {
    mockAgentViewData();
    createAutomationDraftMock.mockResolvedValue({
      automation: {
        id: "automation-setup-flow",
        projectId: "project-1",
        name: "set up a weekly dependency cleanup automation",
        status: "draft",
        pausedReasonCode: null,
        pausedReasonDetail: null,
        goalPrompt: "",
        setupConversationId: "automation-setup-conversation",
        providerHarness: "codex",
        modelId: "gpt-5.5",
        logicalEffort: "xhigh",
        runMode: "edit",
        baseRefKind: "project_default",
        baseRef: "main",
        baseDisplayName: "Project default (main)",
        baseSourcePullRequestJson: null,
        goalItemsJson: null,
        chainMode: "merged_base",
        completionSignal: "pr_merged",
        maxRuns: 25,
        maxConsecutiveFailures: 3,
        firstRunPrompt: null,
        setupAnalysisSummary: null,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      setupConversationId: "automation-setup-conversation",
    });
    startAgentConversationMock.mockResolvedValue({
      conversation: conversation({
        id: "automation-setup-conversation",
        contextId: "project-1",
        title: "Automation setup",
        agentMode: "automation",
        automationId: "automation-setup-flow",
        automationRunId: null,
      }),
      workspace: null,
      sendResult: {
        conversationId: "automation-setup-conversation",
        agentRunId: "run-automation-setup",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });

    renderAgentsView();

    await userEvent.click(screen.getByTestId("agents-start-mode-chip"));
    await userEvent.click(screen.getByTestId("agents-start-mode-automation"));
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "set up a weekly dependency cleanup automation" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(createAutomationDraftMock).toHaveBeenCalledWith({
        projectId: "project-1",
        name: "set up a weekly dependency cleanup automation",
      })
    );
    await waitFor(() =>
      expect(updateAutomationSetupMock).toHaveBeenCalledWith(
        "automation-setup-conversation",
        {
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
        },
      ),
    );
    expect(createConversationMock).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          content: "set up a weekly dependency cleanup automation",
          conversationId: "automation-setup-conversation",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
          mode: "automation",
        })
      )
    );
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    expect(useAgentSessionStore.getState().selectedConversationId).toBe(
      "automation-setup-conversation",
    );
  });

  it("blocks active conversation sends when the conversation provider is not validated", async () => {
    mockAgentViewData();
    mockHarnessProviders(
      agentProviderSettings({
        providers: [
          {
            ...agentProviderSettings().providers[0]!,
            enabled: true,
            available: false,
            binaryFound: false,
            status: "Codex CLI not found",
            error: "Codex CLI not found",
          },
          agentProviderSettings().providers[1]!,
        ],
      }),
    );
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );
    fireEvent.change(screen.getByLabelText("Message input"), {
      target: { value: "continue this run" },
    });

    expect(screen.getByTestId("agents-conversation-submit")).toBeDisabled();
    expect(screen.getByTestId("agents-conversation-provider-status")).toHaveTextContent(
      "Codex CLI not found",
    );
    fireEvent.click(screen.getByTestId("agents-conversation-provider-status-settings"));

    expect(useUiStore.getState().activeModal).toBe("settings");
    expect(useUiStore.getState().modalContext).toEqual({ section: "providers" });
  });

  it("archives the selected conversation, clears the active view, and refreshes archived counts", async () => {
    const user = userEvent.setup();
    const invalidateSpy = vi.spyOn(QueryClient.prototype, "invalidateQueries");
    mockAgentViewData();
    resetAgentSessionState({
      selectedProjectId: "project-1",
      selectedConversationId: "conversation-1",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument()
    );

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(await screen.findByText("Archive session"));
    await user.click(screen.getByRole("button", { name: "Archive session" }));

    await waitFor(() =>
      expect(archiveConversationMock).toHaveBeenCalledWith("conversation-1", {
        closePullRequest: false,
      })
    );
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument()
    );
    expect(screen.queryByTestId("integrated-chat-panel")).not.toBeInTheDocument();
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        queryKey: ["agents", "project-conversations", "project-1", "archived-count"],
        refetchType: "active",
      })
    );

    invalidateSpy.mockRestore();
  });

  it("uploads starter attachments against a seeded conversation before sending the first message", async () => {
    mockAgentViewData();
    createConversationMock.mockResolvedValue(
      conversation({ id: "conversation-seeded", contextId: "project-1" })
    );
    startAgentConversationMock.mockResolvedValue({
      conversation: conversation({ id: "conversation-seeded", contextId: "project-1" }),
      workspace: {
        conversationId: "conversation-seeded",
        projectId: "project-1",
        mode: "edit",
        baseRefKind: "project_default",
        baseRef: "main",
        baseDisplayName: "Project default (main)",
        baseCommit: null,
        branchName: "ralphx/demo/agent-conversation-seeded",
        worktreePath: "/tmp/ralphx/conversation-seeded",
        linkedIdeationSessionId: null,
        linkedPlanBranchId: null,
        publicationPrNumber: null,
        publicationPrUrl: null,
        publicationPrStatus: null,
        publicationPushStatus: null,
        status: "active",
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      sendResult: {
        conversationId: "conversation-seeded",
        agentRunId: "run-2",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });
    vi.mocked(invoke).mockResolvedValue({ id: "attachment-1" });

    const createObjectURL = vi.fn(() => "blob:starter-image-preview");
    const originalCreateObjectURL = URL.createObjectURL;
    Object.defineProperty(URL, "createObjectURL", {
      value: createObjectURL,
      configurable: true,
    });

    const { queryClient } = renderAgentsView();

    const fileInput = screen.getByTestId("attachment-file-input");
    const file = new File(["draft"], "screenshot.png", { type: "image/png" });

    try {
      fireEvent.change(fileInput, {
        target: { files: [file] },
      });
      fireEvent.change(screen.getByTestId("agents-start-textarea"), {
        target: { value: "review this note" },
      });
      fireEvent.click(screen.getByTestId("agents-start-submit"));

      await waitFor(() =>
        expect(createConversationMock).toHaveBeenCalledWith("project", "project-1")
      );
      await waitFor(() =>
        expect(invoke).toHaveBeenCalledWith("upload_chat_attachment", {
          input: expect.objectContaining({
            conversationId: "conversation-seeded",
            fileName: "screenshot.png",
            mimeType: "image/png",
          }),
        })
      );
      await waitFor(() =>
        expect(startAgentConversationMock).toHaveBeenCalledWith(
          expect.objectContaining({
            projectId: "project-1",
            content: "review this note",
            conversationId: "conversation-seeded",
            providerHarness: "codex",
            modelId: "gpt-5.5",
            logicalEffort: "xhigh",
            mode: "edit",
          })
        )
      );
      await waitFor(() =>
        expect(
          queryClient.getQueryData(["chat", "conversations", "conversation-seeded"])
        ).toEqual({
          conversation: expect.objectContaining({ id: "conversation-seeded" }),
          messages: [
            expect.objectContaining({
              conversationId: "conversation-seeded",
              role: "user",
              content: "review this note",
              attachments: [
                expect.objectContaining({
                  fileName: "screenshot.png",
                  fileSize: 5,
                  mimeType: "image/png",
                  previewUrl: "blob:starter-image-preview",
                }),
              ],
            }),
          ],
        })
      );
      expect(createObjectURL).toHaveBeenCalledWith(file);
    } finally {
      Object.defineProperty(URL, "createObjectURL", {
        value: originalCreateObjectURL,
        configurable: true,
      });
    }
  });

});
