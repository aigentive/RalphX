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
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import type { AgentProvidersSettingsResponse } from "@/api/harness-providers";
import type { BranchBaseOption } from "@/components/shared/branchBaseOptions";
import { chatKeys, invalidateConversationDataQueries } from "@/hooks/useChat";
import { FEATURE_FLAGS_QUERY_KEY } from "@/hooks/useFeatureFlags";
import { personaKeys } from "@/hooks/usePersonas";
import { ticketingKeys } from "@/hooks/useTicketing";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import {
  agentProjectFixture as project,
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
  renderWithAgentProviders,
} from "./agentsTestFixtures";
import { AgentsStartComposer } from "./AgentsStartComposer";
import { agentJiraIssueKeys } from "./agentJiraIssueQueries";
import { agentLinearIssueKeys } from "./agentLinearIssueQueries";
import {
  LINKED_SETUP_FAILURE_MARKER,
  MCP_SETUP_PREFLIGHT_MARKER,
} from "./agentStartErrors";
import { useStartAgentConversation } from "./useStartAgentConversation";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

function enabledFeatureFlags(overrides: Record<string, boolean> = {}) {
  return {
    activityPage: true,
    extensibilityPage: true,
    automationsPage: true,
    atlassianOauth: false,
    ticketingDashboard: false,
    agentPersonas: false,
    agentConversationTeam: false,
    agentConversationWorkflows: false,
    ...overrides,
  };
}

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
  startWorkFromTicketMock,
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

function claudeWorkspaceEditRoleDefault() {
  return {
    role: "workspace_edit",
    source: "global_ui",
    value: {
      provider: "claude",
      model: "opus",
      effort: "high",
      service_tier: "provider_default",
      coordination_mode: "solo",
      persona_id: null,
      approval_policy: null,
      sandbox_mode: null,
    },
  } as const;
}

function codexRoleDefault(role: string) {
  return {
    role,
    source: "global_ui",
    value: {
      provider: "codex",
      model: "gpt-5.5",
      effort: "xhigh",
      service_tier: "standard",
      coordination_mode: "solo",
      persona_id: null,
      approval_policy: "never",
      sandbox_mode: "danger-full-access",
    },
  } as const;
}

describe("AgentsView start conversation", () => {
  beforeEach(setupAgentsViewTest);

  it("defaults to the starter composer when no conversation is selected", async () => {
    mockAgentViewData();

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument(),
    );
    expect(screen.getByTestId("agents-start-heading")).toHaveTextContent(
      "Start your agent",
    );
    expect(screen.getByTestId("agents-start-heading-word")).toHaveTextContent(
      "agent",
    );
    expect(screen.getByTestId("agents-start-project")).toBeInTheDocument();
    expect(screen.getByTestId("agents-start-base")).toBeInTheDocument();
    expect(
      screen.getByTestId("agent-composer-runtime-pill"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-start-new-project"),
    ).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    expect(
      screen.queryByTestId("agents-start-new-project"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("integrated-chat-panel"),
    ).not.toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    // Workflow modes live on the Mode chip popover, not the "+" action menu.
    await userEvent.click(screen.getByTestId("agents-start-mode-chip"));
    expect(screen.getByTestId("agents-start-mode-edit")).toBeInTheDocument();
  });

  it("initializes an ordinary new run from the persisted mode preference", async () => {
    mockAgentViewData();
    resetAgentSessionState({ defaultStartMode: "plan" });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-mode-chip")).toHaveTextContent(
        "Plan",
      ),
    );
  });

  it("shows Autopilot only when the capability is enabled and never offers Ideation", async () => {
    mockAgentViewData();
    const { queryClient } = renderAgentsView();

    await userEvent.click(screen.getByTestId("agents-start-mode-chip"));
    await userEvent.click(
      screen.getByRole("button", { name: "Show more modes" }),
    );
    expect(
      screen.queryByTestId("agents-start-mode-autopilot"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-start-mode-ideation"),
    ).not.toBeInTheDocument();
    await userEvent.keyboard("{Escape}");

    queryClient.setQueryData(
      FEATURE_FLAGS_QUERY_KEY,
      enabledFeatureFlags({ agentConversationAutopilot: true }),
    );
    await userEvent.click(screen.getByTestId("agents-start-mode-chip"));
    await userEvent.click(
      screen.getByRole("button", { name: "Show more modes" }),
    );
    expect(
      screen.getByTestId("agents-start-mode-autopilot"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-start-mode-ideation"),
    ).not.toBeInTheDocument();
  });

  it("shows Persona only when enabled and preserves a consumed locked project across project-query churn", async () => {
    const atlas = { ...project, id: "project-atlas", name: "Atlas" };
    mockAgentViewData();
    useProjectsMock.mockReturnValue({
      data: [project, atlas],
      isLoading: false,
    });
    resetAgentSessionState({
      startConversationDraft: {
        projectId: "project-atlas",
        projectLocked: true,
        content: "",
        mode: "persona_builder",
      },
    });
    const view = renderAgentsView();

    await screen.findByTestId("persona-build-banner");
    expect(useAgentSessionStore.getState().startConversationDraft).toBeNull();
    expect(screen.getByTestId("agents-start-project")).toHaveTextContent(
      "Atlas",
    );
    expect(screen.getByTestId("agents-start-project")).toBeDisabled();
    expect(
      screen.getByLabelText("Persona build project is locked"),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByTestId("agents-start-mode-chip"));
    expect(
      screen.queryByTestId("agents-start-mode-persona_builder"),
    ).not.toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    useProjectsMock.mockReturnValue({
      data: [{ ...project }, { ...atlas }],
      isLoading: false,
    });
    view.queryClient.setQueryData(
      FEATURE_FLAGS_QUERY_KEY,
      enabledFeatureFlags({ agentPersonas: true }),
    );
    await userEvent.click(screen.getByTestId("agents-start-mode-chip"));
    expect(
      screen.getByTestId("agents-start-mode-persona_builder"),
    ).toHaveTextContent("Persona");
    await userEvent.keyboard("{Escape}");
    expect(screen.getByTestId("agents-start-project")).toHaveTextContent(
      "Atlas",
    );
    expect(
      screen.queryByTestId("agents-start-capability"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Choose persona" }),
    ).not.toBeInTheDocument();
    await userEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    expect(
      screen.getByRole("button", { name: "Add files" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Add folder" }),
    ).toBeInTheDocument();
  });

  it("keeps standalone reachable with zero projects and prevents project-only controls", async () => {
    mockAgentViewData();
    useProjectsMock.mockReturnValue({ data: [], isLoading: false });
    const { queryClient } = renderAgentsView();
    queryClient.setQueryData(
      FEATURE_FLAGS_QUERY_KEY,
      enabledFeatureFlags({
        standaloneConversations: true,
        agentPersonas: true,
        agentConversationTeam: true,
      }),
    );

    await screen.findByTestId("agents-start-project");
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-project")).not.toBeDisabled(),
    );
    await userEvent.click(screen.getByTestId("agents-start-project"));
    expect(
      screen.getByTestId("agents-start-project-standalone"),
    ).toHaveTextContent("No project (standalone)");
    await userEvent.click(
      screen.getByTestId("agents-start-project-standalone"),
    );

    expect(screen.getByText("Runs in a private workspace")).toBeInTheDocument();
    expect(screen.getByTestId("agents-start-mode-chip")).toHaveTextContent(
      "Ask",
    );
    expect(
      screen.getByText(
        /Project-requiring modes are unavailable without a project/,
      ),
    ).toBeInTheDocument();
    expect(screen.queryByTestId("agents-start-base")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-start-capability"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Choose persona" }),
    ).not.toBeInTheDocument();

    await userEvent.click(screen.getByTestId("agents-start-mode-chip"));
    expect(screen.getByTestId("agents-start-mode-edit")).toBeDisabled();
    expect(screen.getAllByText("Requires a project").length).toBeGreaterThan(0);
    await userEvent.keyboard("{Escape}");

    await userEvent.click(screen.getByTestId("agent-composer-actions-menu"));
    expect(
      screen.queryByRole("button", { name: "Add folder" }),
    ).not.toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "/" },
    });
    expect(
      screen.queryByTestId("agent-composer-command-menu"),
    ).not.toBeInTheDocument();
  });

  it("keeps the zero-project picker disabled when standalone is flag-off", async () => {
    mockAgentViewData();
    useProjectsMock.mockReturnValue({ data: [], isLoading: false });
    renderAgentsView();

    expect(await screen.findByTestId("agents-start-project")).toBeDisabled();
    expect(
      screen.queryByTestId("agents-start-project-standalone"),
    ).not.toBeInTheDocument();
  });

  it("prefills and consumes an explicit start draft ahead of the saved default", async () => {
    mockAgentViewData();
    useAgentSessionStore.getState().setDefaultStartMode("plan");
    loadBranchBaseOptionsMock.mockResolvedValueOnce({
      options: [
        {
          key: "project_default:main",
          label: "Project default (main)",
          source: "project",
          selection: {
            kind: "project_default",
            ref: "main",
            displayName: "Project default (main)",
          },
        },
        {
          key: "local_branch:feature/TASK-123-existing",
          label: "feature/TASK-123-existing",
          source: "local",
          selection: {
            kind: "local_branch",
            ref: "feature/TASK-123-existing",
            displayName: "feature/TASK-123-existing",
          },
        },
      ],
      selectedKey: "project_default:main",
    });
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
        "replace ideation command with agent composer",
      ),
    );
    expect(screen.getByTestId("agents-start-mode-chip")).toHaveTextContent(
      "Agent",
    );
    expect(
      screen.getByTestId(
        "agent-composer-reference-pill-integration:clickup:TASK-123",
      ),
    ).toHaveTextContent("Demo task");
    const planReferencePill = screen.getByTestId(
      "agent-composer-reference-pill-artifact:plan:plan-artifact-1",
    );
    expect(planReferencePill).toHaveTextContent("Runtime Plan");
    expect(planReferencePill).toHaveTextContent("Approved");
    expect(planReferencePill).toHaveTextContent("v2");
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
        "feature/TASK-123-existing",
      ),
    );
    expect(useAgentSessionStore.getState().startConversationDraft).toBeNull();
  });

  it("preselects Automation drafts without creating an automation before submission", async () => {
    mockAgentViewData();
    useAgentSessionStore.getState().setStartConversationDraft({
      projectId: "project-1",
      content: "",
      mode: "automation",
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-mode-chip")).toHaveTextContent(
        "Automation",
      ),
    );
    expect(screen.getByTestId("agents-start-textarea")).toHaveAttribute(
      "placeholder",
      "Describe your goal for the new automation",
    );
    expect(screen.getByTestId("agents-start-submit")).toHaveTextContent(
      "Setup Automation",
    );
    expect(createAutomationDraftMock).not.toHaveBeenCalled();
    expect(startAgentConversationMock).not.toHaveBeenCalled();
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
        "continue this draft",
      ),
    );
    expect(screen.getByTestId("chat-attachment-gallery")).toHaveTextContent(
      "notes.md",
    );
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

    expect(
      await screen.findByTestId("integrated-chat-panel"),
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-session-conversation-restored"),
    ).toHaveTextContent("Older restored agent");
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

    const clickedRow = await screen.findByTestId(
      "agents-session-conversation-older",
    );
    const clickedButton = clickedRow.querySelector("button");
    expect(clickedButton).not.toBeNull();
    fireEvent.click(clickedButton as HTMLButtonElement);

    await waitFor(() =>
      expect(useAgentSessionStore.getState().selectedConversationId).toBe(
        clickedConversation.id,
      ),
    );
    expect(clickedButton).toHaveAttribute("aria-current", "true");
    expect(
      screen.queryByTestId("agents-start-composer"),
    ).not.toBeInTheDocument();

    fireEvent.click(clickedButton);

    await waitFor(() =>
      expect(useAgentSessionStore.getState().selectedConversationId).toBeNull(),
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
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument(),
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

    const row = await screen.findByTestId(
      "agents-session-conversation-existing",
    );
    const button = row.querySelector("button");
    expect(button).not.toBeNull();
    fireEvent.click(button as HTMLButtonElement);
    await waitFor(() =>
      expect(
        screen.queryByTestId("agents-start-composer"),
      ).not.toBeInTheDocument(),
    );

    fireEvent.click(button as HTMLButtonElement);
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument(),
    );

    expect(screen.getByTestId("agents-start-textarea")).toHaveValue(
      "keep this starter draft",
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
        }),
      ),
    );
    expect(createConversationMock).not.toHaveBeenCalled();
    expect(startAgentConversationMock.mock.calls[0]?.[0]).not.toHaveProperty(
      "conversationId",
    );
    expect(startAgentConversationMock.mock.calls[0]?.[0]).not.toHaveProperty(
      "personaId",
    );
    await waitFor(() =>
      expect(spawnConversationSessionNamerMock).toHaveBeenCalledWith(
        "conversation-2",
        "fix agent landing flow",
        "codex",
      ),
    );
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument(),
    );
    expect(
      screen.queryByTestId("agents-start-composer"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByTestId("agents-conversation-workspace-line"),
    ).toHaveTextContent("agent-conversation-2");
    expect(useAgentSessionStore.getState().selectedConversationId).toBe(
      "conversation-2",
    );
    expect(
      useAgentSessionStore.getState().artifactByConversationId[
        "conversation-2"
      ],
    ).toEqual(
      expect.objectContaining({
        isOpen: false,
        activeTab: "plan",
      }),
    );
    expect(
      queryClient.getQueryData(["chat", "conversations", "conversation-2"]),
    ).toEqual({
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
      queryClient.getQueryData([
        "agents",
        "conversation-workspace",
        "conversation-2",
      ]),
    ).toEqual(expect.objectContaining({ conversationId: "conversation-2" }));
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        queryKey: ["agents", "project-conversations", "project-1"],
      }),
    );
    invalidateSpy.mockRestore();
  });

  it("threads a selected active persona to the start invoke only when personas are enabled", async () => {
    mockAgentViewData();
    vi.mocked(invoke).mockImplementation((command) =>
      command === "get_ui_feature_flags"
        ? Promise.resolve(enabledFeatureFlags({ agentPersonas: true }))
        : Promise.resolve(undefined),
    );
    const { queryClient } = renderAgentsView();
    queryClient.setQueryData(
      personaKeys.list({ type: "globalAndProject", projectId: "project-1" }),
      [
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
          projectId: null,
          createdAt: "2026-01-01T00:00:00Z",
          updatedAt: "2026-01-01T00:00:00Z",
        },
      ],
    );

    await userEvent.click(await screen.findByTestId("agent-composer-runtime-pill"));
    await userEvent.click(
      screen.getByTestId("agent-composer-runtime-persona-menu-trigger"),
    );
    await userEvent.click(
      screen.getByTestId("agents-start-persona-persona-reviewer"),
    );
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

  it("omits picker overrides when starting from the untouched role default", async () => {
    mockAgentViewData();
    resetAgentSessionState({ lastRuntimeByProjectId: {} });
    vi.mocked(invoke).mockImplementation((command) =>
      command === "get_start_composer_role_default"
        ? Promise.resolve(claudeWorkspaceEditRoleDefault())
        : Promise.resolve(undefined),
    );
    renderAgentsView();

    await waitFor(() =>
      expect(
        screen.getByTestId("agent-composer-runtime-pill"),
      ).toHaveTextContent("opus"),
    );
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "use the current role default" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledOnce(),
    );
    const startInput = startAgentConversationMock.mock.calls[0]?.[0];
    expect(startInput).not.toHaveProperty("providerHarness");
    expect(startInput).not.toHaveProperty("modelId");
    expect(startInput).not.toHaveProperty("logicalEffort");
    expect(startInput).not.toHaveProperty("codexFastMode");
    expect(startInput).not.toHaveProperty("capabilityIntent");
    expect(startInput).not.toHaveProperty("personaId");
  });

  it("starts with the visible Codex runtime while the Claude role default is still loading", async () => {
    mockAgentViewData();
    resetAgentSessionState({ lastRuntimeByProjectId: {} });
    let resolveRoleDefault:
      | ((value: ReturnType<typeof claudeWorkspaceEditRoleDefault>) => void)
      | null = null;
    const pendingRoleDefault = new Promise<
      ReturnType<typeof claudeWorkspaceEditRoleDefault>
    >((resolve) => {
      resolveRoleDefault = resolve;
    });
    vi.mocked(invoke).mockImplementation((command) =>
      command === "get_start_composer_role_default"
        ? pendingRoleDefault
        : Promise.resolve(undefined),
    );
    renderAgentsView();

    expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveTextContent(
      "gpt-5.5",
    );
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "keep the visible Codex runtime" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledOnce(),
    );
    expect(startAgentConversationMock).toHaveBeenCalledWith(
      expect.objectContaining({
        providerHarness: "codex",
        modelId: "gpt-5.5",
        logicalEffort: "xhigh",
      }),
    );

    resolveRoleDefault?.(claudeWorkspaceEditRoleDefault());
  });

  it("does not reuse a remembered runtime after the workspace mode changes", async () => {
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
    vi.mocked(invoke).mockImplementation((command) =>
      command === "get_start_composer_role_default"
        ? Promise.resolve(codexRoleDefault("workspace_plan"))
        : Promise.resolve(undefined),
    );
    renderAgentsView();

    await userEvent.click(screen.getByTestId("agents-start-mode-chip"));
    await userEvent.click(screen.getByTestId("agents-start-mode-plan"));
    await waitFor(() =>
      expect(
        screen.getByTestId("agent-composer-runtime-pill"),
      ).toHaveTextContent("gpt-5.5"),
    );
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "plan with the plan role default" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledOnce(),
    );
    const startInput = startAgentConversationMock.mock.calls[0]?.[0];
    expect(startInput).not.toHaveProperty("providerHarness");
    expect(startInput).not.toHaveProperty("modelId");
    expect(startInput).not.toHaveProperty("logicalEffort");
    expect(
      useAgentSessionStore.getState().lastRuntimeByProjectId["project-1"],
    ).not.toEqual({
      provider: "claude",
      modelId: "opus",
      effort: "high",
    });
  });

  it("cannot submit while the role default reset is refetching", async () => {
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
    const roleDefault = {
      role: "workspace_edit",
      source: "project_ui",
      value: {
        provider: "codex",
        model: "gpt-5.5",
        effort: "xhigh",
        service_tier: "standard",
        coordination_mode: "solo",
        persona_id: null,
        approval_policy: "never",
        sandbox_mode: "danger-full-access",
      },
    };
    let resolveRefetch: ((value: typeof roleDefault) => void) | null = null;
    const pendingRefetch = new Promise<typeof roleDefault>((resolve) => {
      resolveRefetch = resolve;
    });
    let roleDefaultCalls = 0;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_start_composer_role_default") {
        roleDefaultCalls += 1;
        return roleDefaultCalls === 1
          ? Promise.resolve(roleDefault)
          : pendingRefetch;
      }
      return Promise.resolve(undefined);
    });
    renderAgentsView();

    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    const reset = await screen.findByRole("button", {
      name: "Reset runtime to current role default",
    });
    await waitFor(() => expect(reset).toBeEnabled());
    await userEvent.click(reset);
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "start while the reset is loading" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    expect(startAgentConversationMock).not.toHaveBeenCalled();
    expect(
      useAgentSessionStore.getState().lastRuntimeByProjectId["project-1"],
    ).toEqual({
      provider: "claude",
      modelId: "opus",
      effort: "high",
    });

    resolveRefetch?.(roleDefault);
    await waitFor(() =>
      expect(
        useAgentSessionStore.getState().lastRuntimeByProjectId["project-1"],
      ).toBeUndefined(),
    );

    fireEvent.click(screen.getByTestId("agents-start-submit"));
    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledOnce(),
    );
    const startInput = startAgentConversationMock.mock.calls[0]?.[0];
    expect(startInput).not.toHaveProperty("providerHarness");
    expect(startInput).not.toHaveProperty("modelId");
    expect(startInput).not.toHaveProperty("logicalEffort");
  });

  it("keeps the visible runtime override when reset role default refetch fails", async () => {
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
    const roleDefault = {
      role: "workspace_edit",
      source: "project_ui",
      value: {
        provider: "codex",
        model: "gpt-5.5",
        effort: "xhigh",
        service_tier: "standard",
        coordination_mode: "solo",
        persona_id: null,
        approval_policy: "never",
        sandbox_mode: "danger-full-access",
      },
    };
    let roleDefaultCalls = 0;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_start_composer_role_default") {
        roleDefaultCalls += 1;
        return roleDefaultCalls === 1
          ? Promise.resolve(roleDefault)
          : Promise.reject(new Error("role default unavailable"));
      }
      return Promise.resolve(undefined);
    });
    renderAgentsView();

    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    const reset = await screen.findByRole("button", {
      name: "Reset runtime to current role default",
    });
    await waitFor(() => expect(reset).toBeEnabled());
    await userEvent.click(reset);

    await waitFor(() =>
      expect(screen.getByText("role default unavailable")).toBeInTheDocument(),
    );
    expect(
      useAgentSessionStore.getState().lastRuntimeByProjectId["project-1"],
    ).toEqual({
      provider: "claude",
      modelId: "opus",
      effort: "high",
    });

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "send after failed reset" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledOnce(),
    );
    expect(startAgentConversationMock).toHaveBeenCalledWith(
      expect.objectContaining({
        providerHarness: "claude",
        modelId: "opus",
        logicalEffort: "high",
      }),
    );
  });

  it("starts a new conversation with Team enabled", async () => {
    mockAgentViewData();

    const { queryClient } = renderAgentsView();
    queryClient.setQueryData(FEATURE_FLAGS_QUERY_KEY, {
      activityPage: true,
      extensibilityPage: true,
      automationsPage: true,
      atlassianOauth: false,
      ticketingDashboard: false,
      agentPersonas: false,
      agentConversationTeam: true,
      agentConversationWorkflows: false,
    });

    await waitFor(() =>
      expect(
        screen.getByTestId("agent-composer-runtime-pill"),
      ).toBeInTheDocument(),
    );
    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    await userEvent.click(
      screen.getByRole("button", { name: /^Capabilities,/ }),
    );
    expect(
      screen.getByTestId("agents-start-capability-rx_native_team"),
    ).toHaveTextContent(
      "Let this agent delegate to RalphX teammates when it helps; it may also work alone.",
    );
    await userEvent.click(
      screen.getByTestId("agents-start-capability-rx_native_team"),
    );
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "coordinate this implementation" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          content: "coordinate this implementation",
          capabilityIntent: { coordinationMode: "rx_native_team" },
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
      }),
    );
    const prOption = await screen.findByText("#42 Add PR picker");
    const prOptionButton = prOption.closest("button");
    expect(prOptionButton).not.toBeNull();
    await user.click(prOptionButton as HTMLButtonElement);
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
        "#42 Add PR picker",
      ),
    );
    expect(
      screen.getByRole("switch", { name: /Use isolated branch/i }),
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
        }),
      ),
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
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
        "#42 Add PR picker",
      ),
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
        }),
      ),
    );
  });

  it("shows linked setup failure inline and retries the same no-file start with isolation", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    loadPullRequestBaseOptionsMock.mockResolvedValue([prPickerBranchOption()]);
    startAgentConversationMock
      .mockRejectedValueOnce(
        new Error(
          `${LINKED_SETUP_FAILURE_MARKER} Selected branch 'feature/pr-picker' is already checked out; choose isolated branch mode`,
        ),
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

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledTimes(1),
    );
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
      }),
    );
    await waitFor(() =>
      expect(useAgentSessionStore.getState().startConversationFailure).toEqual(
        expect.objectContaining({
          kind: "linked_setup",
          message: expect.stringContaining("feature/pr-picker"),
        }),
      ),
    );
    await waitFor(() =>
      expect(useAgentSessionStore.getState().selectedConversationId).toBeNull(),
    );

    const linkedError = await screen.findByTestId(
      "agents-start-linked-setup-error",
    );
    expect(linkedError).toHaveTextContent("Linked branch setup failed");
    expect(linkedError).toHaveTextContent(
      "Selected branch 'feature/pr-picker'",
    );
    expect(linkedError).toHaveTextContent(
      "Branch isolation creates a separate RalphX branch",
    );
    expect(screen.getByTestId("agents-start-textarea")).toHaveValue(
      "review this PR directly",
    );

    await user.click(screen.getByTestId("agents-start-linked-setup-retry"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledTimes(2),
    );
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
      }),
    );
    await waitFor(() =>
      expect(useAgentSessionStore.getState().selectedConversationId).toBe(
        "conversation-linked-retry",
      ),
    );
  });

  it("shows in-app MCP recovery without terminal guidance and opens the exact settings card", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    startAgentConversationMock.mockRejectedValueOnce(
      new Error(
        `${MCP_SETUP_PREFLIGHT_MARKER}{"provider":"claude","server_id":"ralphx","scope":"user","conflict_kind":"ambiguous_reserved_id","repair_status":"manual_only"}`,
      ),
    );

    renderAgentsView();
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "keep this draft" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    const recovery = await screen.findByTestId("agents-start-mcp-setup-error");
    expect(recovery).toHaveTextContent("MCP setup needs attention");
    expect(recovery).not.toHaveTextContent("claude mcp remove");
    expect(screen.getByTestId("agents-start-textarea")).toHaveValue(
      "keep this draft",
    );
    expect(useAgentSessionStore.getState().selectedConversationId).toBeNull();
    expect(useAgentSessionStore.getState().startConversationFailure).toEqual(
      expect.objectContaining({ kind: "mcp_setup", serverId: "ralphx" }),
    );

    await user.click(screen.getByRole("button", { name: "Open MCP settings" }));
    expect(useUiStore.getState().activeModal).toBe("settings");
    expect(useUiStore.getState().modalContext).toEqual({
      section: "mcp",
      provider: "claude",
      serverId: "ralphx",
      scope: "user",
    });
  });

  it("retries Claude MCP cleanup and automatically replays the original start once", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    startAgentConversationMock.mockRejectedValueOnce(
      new Error(
        `${MCP_SETUP_PREFLIGHT_MARKER}{"provider":"claude","server_id":"ralphx","scope":"user","conflict_kind":"legacy_repair_failed","repair_status":"failed"}`,
      ),
    );

    renderAgentsView();
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "keep this cleanup draft" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await screen.findByTestId("agents-start-mcp-setup-error");
    expect(screen.getByRole("button", { name: "Retry cleanup" })).toBeEnabled();

    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockResolvedValueOnce({ changed: true });
    await user.click(screen.getByRole("button", { name: "Retry cleanup" }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "retry_legacy_mcp_registration_repair",
        {
          input: {
            provider: "claude",
            serverId: "ralphx",
            scope: "user",
          },
        },
      ),
    );
    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledTimes(2),
    );
    expect(startAgentConversationMock.mock.calls[1]?.[0]).toEqual(
      expect.objectContaining({ content: "keep this cleanup draft" }),
    );
    expect(screen.queryByText(/Start the agent again/)).not.toBeInTheDocument();
    expect(useAgentSessionStore.getState().startConversationFailure).toBeNull();
  });

  it("snapshots files and folders before MCP cleanup, gates Send, and replays those references once", async () => {
    const user = userEvent.setup();
    const file = new File(["same bytes"], "recovery.txt", {
      type: "text/plain",
    });
    const folder = {
      id: "folder-recovery",
      folderPath: "/work/recovery",
      displayName: "recovery",
    };
    useChatStore.getState().setComposerDraftFolders("agents:start", [folder]);
    let finishCleanup: ((value: { changed: boolean }) => void) | null = null;
    const onSubmit = vi
      .fn()
      .mockRejectedValueOnce(
        new Error(
          `${MCP_SETUP_PREFLIGHT_MARKER}{"provider":"claude","server_id":"ralphx","scope":"user","conflict_kind":"legacy_repair_failed","repair_status":"failed"}`,
        ),
      )
      .mockResolvedValueOnce(undefined);

    renderWithAgentProviders(
      <AgentsStartComposer
        projects={[project]}
        defaultProjectId={project.id}
        defaultRuntime={{
          provider: "codex",
          modelId: "gpt-5.5",
          effort: "xhigh",
        }}
        isLoadingProjects={false}
        isSubmitting={false}
        modelRegistry={{
          claude: [],
          codex: [
            {
              id: "gpt-5.5",
              label: "gpt-5.5",
              menuLabel: "gpt-5.5",
              defaultEffort: "xhigh",
              supportedEfforts: ["xhigh"],
            },
          ],
        }}
        onSubmit={onSubmit}
      />,
    );
    fireEvent.change(screen.getByTestId("attachment-file-input"), {
      target: { files: [file] },
    });
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "replay with context" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));
    await screen.findByTestId("agents-start-mcp-setup-error");

    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finishCleanup = resolve;
        }),
    );
    await user.click(screen.getByRole("button", { name: "Retry cleanup" }));
    expect(
      screen.getByRole("button", { name: "Retrying cleanup…" }),
    ).toBeDisabled();
    expect(screen.getByTestId("agents-start-submit")).toBeDisabled();
    fireEvent.click(screen.getByTestId("agents-start-submit"));
    expect(onSubmit).toHaveBeenCalledTimes(1);

    finishCleanup?.({ changed: true });
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(2));
    const replay = onSubmit.mock.calls[1]?.[0];
    expect(replay.files).toHaveLength(1);
    expect(replay.files[0]).toBe(file);
    expect(replay.folders).toEqual([{ ...folder }]);
  });

  it("keeps the MCP recovery actionable when cleanup retry fails", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    startAgentConversationMock.mockRejectedValueOnce(
      new Error(
        `${MCP_SETUP_PREFLIGHT_MARKER}{"provider":"claude","server_id":"ralphx","scope":"user","conflict_kind":"legacy_repair_failed","repair_status":"failed"}`,
      ),
    );

    renderAgentsView();
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "keep this failed cleanup draft" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));
    await screen.findByTestId("agents-start-mcp-setup-error");

    vi.mocked(invoke).mockRejectedValueOnce(new Error("cleanup unavailable"));
    await user.click(screen.getByRole("button", { name: "Retry cleanup" }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "retry_legacy_mcp_registration_repair",
        expect.anything(),
      ),
    );
    expect(
      screen.getByTestId("agents-start-mcp-setup-error"),
    ).toHaveTextContent("Retry cleanup");
    expect(screen.getByTestId("agents-start-textarea")).toHaveValue(
      "keep this failed cleanup draft",
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
        "Current branch (feature/current)",
      ),
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
        }),
      ),
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
        "Project default (main)",
      ),
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
        }),
      ),
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
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
        "develop",
      ),
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
        }),
      ),
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
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
        "#42 Add PR picker",
      ),
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
        }),
      ),
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
        "Review PR",
      ),
    );
    await user.type(
      screen.getByTestId("agents-start-textarea"),
      "review without a PR",
    );
    await user.click(screen.getByTestId("agents-start-submit"));

    expect(
      await screen.findByText("Select a pull request to review."),
    ).toBeInTheDocument();
    expect(startAgentConversationMock).not.toHaveBeenCalled();
  });

  it("keeps a selected pull request visible across later start-from searches", async () => {
    const user = userEvent.setup();
    let pullRequestSearches = 0;
    mockAgentViewData();
    loadPullRequestBaseOptionsMock.mockImplementation(() => {
      pullRequestSearches += 1;
      return Promise.resolve(
        pullRequestSearches === 1 ? [prPickerBranchOption()] : [],
      );
    });

    renderAgentsView();

    await user.click(await screen.findByTestId("agents-start-base"));
    await user.click(screen.getByRole("tab", { name: /PRs/i }));
    await user.click(await screen.findByText("#42 Add PR picker"));
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
        "#42 Add PR picker",
      ),
    );

    await user.type(
      screen.getByPlaceholderText(/Search pull requests/i),
      "missing",
    );

    await waitFor(() => expect(pullRequestSearches).toBe(2));
    expect(screen.getAllByText("#42 Add PR picker").length).toBeGreaterThan(1);
  });

  it("shows pull request search failures in the start composer", async () => {
    const user = userEvent.setup();
    mockAgentViewData();
    loadPullRequestBaseOptionsMock.mockRejectedValue(
      new Error("GitHub search failed"),
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
      | ((
          value: Awaited<ReturnType<typeof startAgentConversationMock>>,
        ) => void)
      | null = null;
    startAgentConversationMock.mockReturnValue(
      new Promise((resolve) => {
        resolveStart = resolve;
      }),
    );

    const { queryClient } = renderAgentsView();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "start without waiting" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument(),
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
      }),
    );
    expect(
      queryClient.getQueryData([
        "chat",
        "conversations",
        optimisticConversationId,
      ]),
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
        }),
      ),
    );
    expect(createConversationMock).not.toHaveBeenCalled();
    expect(startAgentConversationMock.mock.calls[0]?.[0]).not.toHaveProperty(
      "conversationId",
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
        "conversation-resolved-no-file",
      ),
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
      expect(
        screen.getByTestId("agents-start-paused-banner"),
      ).toHaveTextContent("Execution is paused"),
    );
    expect(screen.getByTestId("agents-start-submit")).toHaveTextContent(
      "Queue Prompt",
    );

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: queuedPrompt },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          content: queuedPrompt,
        }),
      ),
    );
    expect(createConversationMock).not.toHaveBeenCalled();
    expect(startAgentConversationMock.mock.calls[0]?.[0]).not.toHaveProperty(
      "conversationId",
    );
    await waitFor(() =>
      expect(
        useChatStore.getState().queuedMessages["project:conversation-paused"],
      ).toEqual([
        expect.objectContaining({
          id: "queued-paused-start",
          content: queuedPrompt,
          isEditing: false,
        }),
      ]),
    );

    const queuedEmptyState = await screen.findByTestId(
      "agents-paused-queued-empty-state",
    );
    expect(queuedEmptyState).toHaveTextContent("Execution is paused");
    expect(queuedEmptyState).toHaveTextContent(
      "This prompt will start when execution resumes.",
    );
    const queuedPromptPreview = screen.getByTestId(
      "agents-paused-queued-prompt",
    );
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
        }),
      ),
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
            supportedEfforts: [
              "low",
              "medium",
              "high",
              "xhigh",
              "max",
              "ultra",
            ],
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
        }),
      ),
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
      expect(
        screen.getByTestId("agent-composer-runtime-pill"),
      ).toHaveTextContent("gpt-5.5"),
    );
    expect(
      useAgentSessionStore.getState().lastRuntimeByProjectId["project-1"],
    ).toEqual({
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
      { wrapper },
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
      }),
    );
  });

  it.each([
    { provider: "claude", modelId: "sonnet", effort: "medium" },
    { provider: "codex", modelId: "gpt-5.5", effort: "xhigh" },
  ] as const)(
    "seeds and starts a standalone chat with $provider runtime and no project or Team intent",
    async ({ provider, modelId, effort }) => {
      const queryClient = new QueryClient({
        defaultOptions: {
          queries: { retry: false, gcTime: 0 },
          mutations: { retry: false },
        },
      });
      const standaloneConversation = conversation({
        id: "standalone-1",
        contextType: "standalone",
        contextId: "standalone-1",
        projectId: null,
        agentMode: "chat",
      });
      createConversationMock.mockResolvedValue(standaloneConversation);
      startAgentConversationMock.mockResolvedValue({
        conversation: standaloneConversation,
        workspace: null,
        sendResult: {
          conversationId: "standalone-1",
          agentRunId: "run-standalone-1",
          isNewConversation: false,
          wasQueued: true,
          queuedAsPending: false,
          queuedMessageId: "queued-standalone-1",
        },
      });
      const selectConversation = vi.fn();
      const handleAutoManagedTitle = vi.fn();
      const wrapper = ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={queryClient}>
          {children}
        </QueryClientProvider>
      );
      const { result } = renderHook(
        () =>
          useStartAgentConversation({
            handleAutoManagedTitle,
            invalidateProjectConversations: vi
              .fn()
              .mockResolvedValue(undefined),
            queryClient,
            selectConversation,
            setActiveConversation:
              useChatStore.getState().setActiveConversation,
            setFocusedProject: vi.fn(),
            setOptimisticConversationsById: vi.fn(),
            setOptimisticSelectedConversationId: vi.fn(),
            setOptimisticWorkspacesByConversationId: vi.fn(),
            setRuntimeForConversation: vi.fn(),
          }),
        { wrapper },
      );

      await result.current({
        projectId: null,
        content: "Explore privately",
        runtime: { provider, modelId, effort },
        mode: "edit",
        base: null,
        files: [],
        capabilityIntent: { coordinationMode: "rx_native_team" },
        composerProjectReferences: [
          {
            projectId: "stale-project",
            projectName: "Stale project",
          },
        ],
      });

      expect(createConversationMock).toHaveBeenCalledWith("standalone", null);
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          content: "Explore privately",
          conversationId: "standalone-1",
          mode: "chat",
          providerHarness: provider,
          modelId,
          logicalEffort: effort,
        }),
      );
      const startInput = startAgentConversationMock.mock.calls[0]?.[0];
      expect(startInput).not.toHaveProperty("projectId");
      expect(startInput).not.toHaveProperty("capabilityIntent");
      expect(startInput).not.toHaveProperty("teamIntent");
      expect(startInput).not.toHaveProperty("composerProjectReferences");
      expect(selectConversation).toHaveBeenCalledWith(null, "standalone-1");
      expect(
        useChatStore.getState().queuedMessages["standalone:standalone-1"],
      ).toEqual([
        expect.objectContaining({
          id: "queued-standalone-1",
          content: "Explore privately",
        }),
      ]);
      expect(handleAutoManagedTitle).toHaveBeenCalledWith(
        expect.objectContaining({ targetProjectId: null }),
      );
    },
  );

  it.each([
    { provider: "claude", modelId: "sonnet", effort: "medium" },
    { provider: "codex", modelId: "gpt-5.5", effort: "xhigh" },
  ] as const)(
    "starts a Global persona builder with $provider, locked provenance, folders, and no project or Team intent",
    async ({ provider, modelId, effort }) => {
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false, gcTime: 0 } },
      });
      const builderConversation = conversation({
        id: "builder-global-1",
        contextType: "standalone",
        contextId: "builder-global-1",
        projectId: null,
        agentMode: "persona_builder",
      });
      createConversationMock.mockResolvedValue(builderConversation);
      startAgentConversationMock.mockResolvedValue({
        conversation: builderConversation,
        workspace: null,
        sendResult: {
          conversationId: "builder-global-1",
          agentRunId: "run-builder-1",
          isNewConversation: false,
          wasQueued: false,
          queuedAsPending: false,
          queuedMessageId: null,
        },
      });
      vi.mocked(invoke).mockImplementation(async (command) => {
        if (command === "add_conversation_folder_reference") {
          return {
            id: "folder-ref-1",
            conversationId: "builder-global-1",
            folderPath: "/context/docs",
            displayName: "docs",
            createdAt: "2026-07-17T00:00:00Z",
          };
        }
        throw new Error(`Unexpected command: ${command}`);
      });
      const wrapper = ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={queryClient}>
          {children}
        </QueryClientProvider>
      );
      const { result } = renderHook(
        () =>
          useStartAgentConversation({
            handleAutoManagedTitle: vi.fn(),
            invalidateProjectConversations: vi
              .fn()
              .mockResolvedValue(undefined),
            queryClient,
            selectConversation: vi.fn(),
            setActiveConversation:
              useChatStore.getState().setActiveConversation,
            setFocusedProject: vi.fn(),
            setOptimisticConversationsById: vi.fn(),
            setOptimisticSelectedConversationId: vi.fn(),
            setOptimisticWorkspacesByConversationId: vi.fn(),
            setRuntimeForConversation: vi.fn(),
          }),
        { wrapper },
      );

      await result.current({
        projectId: null,
        content: "Refine the review voice",
        runtime: { provider, modelId, effort },
        mode: "persona_builder",
        sourcePersonaId: "persona-reviewer",
        base: null,
        files: [],
        folders: [{ folderPath: "/context/docs", displayName: "docs" }],
        capabilityIntent: { coordinationMode: "rx_native_team" },
      });

      expect(createConversationMock).toHaveBeenCalledWith(
        "standalone",
        null,
        undefined,
        "persona_builder",
      );
      expect(invoke).toHaveBeenCalledWith("add_conversation_folder_reference", {
        input: {
          conversationId: "builder-global-1",
          folderPath: "/context/docs",
          displayName: "docs",
        },
      });
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          conversationId: "builder-global-1",
          mode: "persona_builder",
          providerHarness: provider,
          sourcePersonaId: "persona-reviewer",
        }),
      );
      const startInput = startAgentConversationMock.mock.calls[0]?.[0];
      expect(startInput).not.toHaveProperty("projectId");
      expect(startInput).not.toHaveProperty("capabilityIntent");
      expect(startInput).not.toHaveProperty("teamIntent");
    },
  );

  it.each([
    { provider: "claude", modelId: "sonnet", effort: "medium" },
    { provider: "codex", modelId: "gpt-5.5", effort: "xhigh" },
  ] as const)(
    "starts a project persona builder with $provider runtime",
    async ({ provider, modelId, effort }) => {
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false, gcTime: 0 } },
      });
      const builderConversation = conversation({
        id: "builder-project-1",
        contextType: "project",
        contextId: "project-1",
        projectId: "project-1",
        agentMode: "persona_builder",
      });
      startAgentConversationMock.mockResolvedValue({
        conversation: builderConversation,
        workspace: conversationWorkspace({
          conversationId: "builder-project-1",
          mode: "persona_builder",
        }),
        sendResult: {
          conversationId: "builder-project-1",
          agentRunId: "run-builder-project-1",
          isNewConversation: true,
          wasQueued: false,
          queuedAsPending: false,
          queuedMessageId: null,
        },
      });
      const wrapper = ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={queryClient}>
          {children}
        </QueryClientProvider>
      );
      const { result } = renderHook(
        () =>
          useStartAgentConversation({
            handleAutoManagedTitle: vi.fn(),
            invalidateProjectConversations: vi
              .fn()
              .mockResolvedValue(undefined),
            queryClient,
            selectConversation: vi.fn(),
            setActiveConversation:
              useChatStore.getState().setActiveConversation,
            setFocusedProject: vi.fn(),
            setOptimisticConversationsById: vi.fn(),
            setOptimisticSelectedConversationId: vi.fn(),
            setOptimisticWorkspacesByConversationId: vi.fn(),
            setRuntimeForConversation: vi.fn(),
          }),
        { wrapper },
      );

      await result.current({
        projectId: "project-1",
        content: "Build a project reviewer",
        runtime: { provider, modelId, effort },
        mode: "persona_builder",
        base: null,
        files: [],
      });

      expect(createConversationMock).not.toHaveBeenCalled();
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          content: "Build a project reviewer",
          mode: "persona_builder",
          providerHarness: provider,
          modelId,
          logicalEffort: effort,
        }),
      );
    },
  );

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
        }),
      ),
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
      expect(
        screen.getByTestId("agent-composer-runtime-pill"),
      ).toHaveTextContent("sonnet"),
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
        }),
      ),
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
    await user.click(screen.getByRole("button", { name: /^Provider,/ }));
    await user.click(
      screen.getByTestId("agent-composer-runtime-provider-claude"),
    );

    expect(screen.getByText("Claude is not enabled")).toBeInTheDocument();
    expect(
      screen.getByText("Enable this provider in settings to use its models."),
    ).toBeInTheDocument();
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
    expect(
      screen.getByTestId("agents-start-provider-status"),
    ).toHaveTextContent("Enable in Settings.");
    fireEvent.click(
      screen.getByTestId("agents-start-provider-status-settings"),
    );

    expect(startAgentConversationMock).not.toHaveBeenCalled();
    expect(useUiStore.getState().activeModal).toBe("settings");
    expect(useUiStore.getState().modalContext).toEqual({
      section: "providers",
    });
  });

  it("remembers runtime changes made on the starter composer before creating a conversation", async () => {
    mockAgentViewData();

    renderAgentsView();

    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    await userEvent.click(screen.getByRole("button", { name: /^Provider,/ }));
    await userEvent.click(
      screen.getByTestId("agent-composer-runtime-provider-claude"),
    );
    await userEvent.click(screen.getByRole("button", { name: /^Model,/ }));
    await userEvent.click(screen.getByTestId("agents-start-model-opus"));
    await userEvent.click(screen.getByRole("button", { name: /^Effort,/ }));
    await userEvent.click(screen.getByTestId("agents-start-effort-max"));

    await waitFor(() =>
      expect(
        useAgentSessionStore.getState().lastRuntimeByProjectMode["project-1:edit"],
      ).toEqual({
        provider: "claude",
        modelId: "opus",
        effort: "max",
      }),
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
        }),
      ),
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
        const settings = options?.refreshRuntime
          ? refreshedSettings
          : snapshotSettings;
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
      },
    );

    renderAgentsView();

    await user.click(screen.getByTestId("agent-composer-runtime-pill"));
    await user.click(screen.getByRole("button", { name: /^Provider,/ }));
    await user.click(
      screen.getByTestId("agent-composer-runtime-provider-claude"),
    );
    await user.click(screen.getByRole("button", { name: /^Model,/ }));

    expect(useHarnessProvidersMock).toHaveBeenCalledWith({
      refreshRuntime: true,
    });
    expect(
      await screen.findByTestId("agents-start-model-fable"),
    ).toBeInTheDocument();

    await user.click(screen.getByTestId("agents-start-model-fable"));

    expect(screen.getByTestId("agent-composer-runtime-pill")).toHaveTextContent(
      "fable",
    );
  });

  it("shows manage models link in the runtime selector popover", async () => {
    mockAgentViewData();

    renderAgentsView();

    await userEvent.click(screen.getByTestId("agent-composer-runtime-pill"));
    await userEvent.click(screen.getByRole("button", { name: /^Model,/ }));

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
      | ((
          value: Awaited<ReturnType<typeof startAgentConversationMock>>,
        ) => void)
      | null = null;
    createConversationMock.mockResolvedValue(seededConversation);
    startAgentConversationMock.mockReturnValue(
      new Promise((resolve) => {
        resolveStart = resolve;
      }),
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
      expect(createConversationMock).toHaveBeenCalledWith(
        "project",
        "project-1",
      ),
    );
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument(),
    );
    expect(
      queryClient.getQueryData([
        "chat",
        "conversations",
        "conversation-seeded",
      ]),
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
      useChatStore.getState().agentStatus["project:conversation-seeded"],
    ).toBe("generating");
    expect(
      useChatStore.getState().isSending["project:conversation-seeded"],
    ).toBe(true);
    expect(
      useChatStore.getState().agentActivityLabels[
        "project:conversation-seeded"
      ],
    ).toBe("Setup workspace");
    expect(
      useAgentSessionStore.getState().runtimeByConversationId[
        "conversation-seeded"
      ],
    ).toEqual({
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "xhigh",
    });
    expect(useAgentSessionStore.getState().selectedConversationId).toBe(
      "conversation-seeded",
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
      }),
    );
    expect(startAgentConversationMock).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: "conversation-seeded",
        content: "fix agent landing flow",
      }),
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
        "codex",
      ),
    );
  });

  it("stores selected references on the seeded optimistic folder message", async () => {
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command === "add_conversation_folder_reference") {
        const input = (
          args as {
            input: {
              conversationId: string;
              folderPath: string;
              displayName: string;
            };
          }
        ).input;
        return Promise.resolve({
          id: "folder-1",
          ...input,
          createdAt: "2026-07-20T07:00:00Z",
        });
      }
      return Promise.resolve(undefined);
    });
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
    createConversationMock.mockResolvedValue(seededConversation);
    let resolveStart:
      | ((
          value: Awaited<ReturnType<typeof startAgentConversationMock>>,
        ) => void)
      | null = null;
    startAgentConversationMock.mockReturnValue(
      new Promise((resolve) => {
        resolveStart = resolve;
      }),
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
      { wrapper },
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
      folders: [{ folderPath: "/work/brand-kit", displayName: "brand-kit" }],
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
      expect(setOptimisticSelectedConversationId).toHaveBeenCalled(),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("add_conversation_folder_reference", {
        input: {
          conversationId: "conversation-seeded-references",
          folderPath: "/work/brand-kit",
          displayName: "brand-kit",
        },
      }),
    );
    const optimisticMessage = queryClient.getQueryData<{
      messages: Array<{ metadata: string | null }>;
    }>(["chat", "conversations", "conversation-seeded-references"])
      ?.messages[0];
    expect(JSON.parse(optimisticMessage?.metadata ?? "{}")).toEqual({
      composer_folder_references: [
        {
          folderPath: "/work/brand-kit",
          displayName: "brand-kit",
        },
      ],
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

    expect(createConversationMock).toHaveBeenCalledWith("project", "project-1");
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
      { wrapper },
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

    expect(startWorkFromTicketMock).toHaveBeenCalledWith(
      expect.objectContaining({
        ticketRef: { provider: "jira", id: "RX-42", key: "RX-42" },
      }),
    );

    expect(invalidateQueriesSpy).toHaveBeenCalledWith({
      queryKey: agentJiraIssueKeys.issue("conversation-with-jira"),
    });
    expect(onJiraLinked).toHaveBeenCalledWith("conversation-with-jira");
    expect(
      useAgentSessionStore.getState().artifactByConversationId[
        "conversation-with-jira"
      ],
    ).toEqual(
      expect.objectContaining({
        isOpen: true,
        activeTab: "jira",
      }),
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
      { wrapper },
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
      useAgentSessionStore.getState().artifactByConversationId[
        "conversation-with-linear"
      ],
    ).toEqual(
      expect.objectContaining({
        isOpen: true,
        activeTab: "linear",
      }),
    );
  });

  it("opens and invalidates the ClickUp tab after starting with a ClickUp reference", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: 0 },
        mutations: { retry: false },
      },
    });
    const invalidateQueriesSpy = vi.spyOn(queryClient, "invalidateQueries");
    const seededConversation = conversation({
      id: "conversation-with-clickup",
      contextId: "project-1",
      title: null,
    });
    createConversationMock.mockResolvedValue(seededConversation);
    startAgentConversationMock.mockResolvedValue({
      conversation: seededConversation,
      workspace: conversationWorkspace({
        conversationId: "conversation-with-clickup",
      }),
      sendResult: {
        conversationId: "conversation-with-clickup",
        agentRunId: "run-with-clickup",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
    const onClickUpLinked = vi.fn();
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
          onClickUpLinked,
        }),
      { wrapper },
    );

    await result.current({
      projectId: "project-1",
      content: "start with clickup",
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
          provider: "clickup",
          kind: "clickup",
          id: "task-42",
          key: "CU-42",
          title: "Restore rich artifact details",
        },
      ],
    });

    expect(startWorkFromTicketMock).toHaveBeenCalledWith(
      expect.objectContaining({
        ticketRef: { provider: "clickup", id: "task-42", key: "CU-42" },
      }),
    );
    expect(invalidateQueriesSpy).toHaveBeenCalledWith({
      queryKey: ticketingKeys.conversationTicket("conversation-with-clickup"),
    });
    expect(onClickUpLinked).toHaveBeenCalledWith("conversation-with-clickup");
    expect(
      useAgentSessionStore.getState().artifactByConversationId[
        "conversation-with-clickup"
      ],
    ).toEqual(
      expect.objectContaining({
        isOpen: true,
        activeTab: "clickup",
      }),
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
    startAgentConversationMock.mockRejectedValue(
      new Error("backend unavailable"),
    );
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
      { wrapper },
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
      }),
    ).rejects.toThrow("backend unavailable");

    expect(createConversationMock).toHaveBeenCalledWith("project", "project-1");
    expect(invoke).toHaveBeenCalledWith("upload_chat_attachment", {
      input: expect.objectContaining({
        conversationId: "conversation-failed-start",
        fileName: "draft.txt",
      }),
    });
    expect(invoke).toHaveBeenCalledWith("abort_seeded_agent_conversation", {
      conversationId: "conversation-failed-start",
    });
    expect(
      queryClient.getQueryData([
        "chat",
        "conversations",
        "conversation-failed-start",
      ]),
    ).toBeUndefined();
    expect(
      useChatStore.getState().agentStatus["project:conversation-failed-start"],
    ).toBeUndefined();
    expect(
      useChatStore.getState().isSending["project:conversation-failed-start"],
    ).toBeUndefined();
    expect(
      useChatStore.getState().agentActivityLabels[
        "project:conversation-failed-start"
      ],
    ).toBeUndefined();
  });

  it("reveals and invalidates a seeded conversation when abort reports it already started", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: 0 },
        mutations: { retry: false },
      },
    });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const selectConversation = vi.fn();
    const seededConversation = conversation({
      id: "conversation-survived-abort",
      contextId: "project-1",
      title: null,
    });
    createConversationMock.mockResolvedValue(seededConversation);
    startAgentConversationMock.mockRejectedValue(
      new Error("start response was lost"),
    );
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "upload_chat_attachment") {
        return Promise.resolve({ id: "attachment-survived-start" });
      }
      if (command === "abort_seeded_agent_conversation") {
        return Promise.reject(
          new Error(
            "SEEDED_AGENT_CONVERSATION_ALREADY_STARTED: conversation `conversation-survived-abort` has already started",
          ),
        );
      }
      return Promise.resolve(undefined);
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
          selectConversation,
          setActiveConversation: useChatStore.getState().setActiveConversation,
          setFocusedProject: vi.fn(),
          setOptimisticConversationsById: vi.fn(),
          setOptimisticSelectedConversationId: vi.fn(),
          setOptimisticWorkspacesByConversationId: vi.fn(),
          setRuntimeForConversation: vi.fn(),
        }),
      { wrapper },
    );

    await expect(
      result.current({
        projectId: "project-1",
        content: "recover the surviving start",
        runtime: {
          provider: "codex",
          modelId: "gpt-5.5",
          effort: "xhigh",
        },
        mode: "edit",
        base: null,
        files: [new File(["draft"], "draft.txt", { type: "text/plain" })],
      }),
    ).rejects.toThrow("start response was lost");

    expect(selectConversation).toHaveBeenCalledWith(
      "project-1",
      "conversation-survived-abort",
    );
    expect(
      queryClient.getQueryData([
        "chat",
        "conversations",
        "conversation-survived-abort",
      ]),
    ).toBeDefined();
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["agents", "sidebar-conversations"],
    });
    expect(invalidateConversationDataQueries).toHaveBeenCalledWith(
      queryClient,
      "conversation-survived-abort",
    );
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
      }),
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
      },
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
      { wrapper },
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
      useChatStore.getState().agentStatus["project:conversation-seeded-remap"],
    ).toBeUndefined();
    expect(
      useChatStore.getState().isSending["project:conversation-seeded-remap"],
    ).toBeUndefined();
    expect(
      useChatStore.getState().agentStatus[
        "project:conversation-resolved-remap"
      ],
    ).toBe("generating");
    expect(
      queryClient.getQueryData([
        "chat",
        "conversations",
        "conversation-resolved-remap",
      ]),
    ).toEqual({
      conversation: expect.objectContaining({
        id: "conversation-resolved-remap",
      }),
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
        chatKeys.conversationSummary("conversation-resolved-remap"),
      ),
    ).toEqual(expect.objectContaining({ id: "conversation-resolved-remap" }));
    expect(optimisticSelectedConversationId).toBe(
      "conversation-resolved-remap",
    );
    expect(handleAutoManagedTitle).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: "conversation-resolved-remap",
        content: "start then remap",
      }),
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
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument(),
    );
    expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
      "Project default (main)",
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
      }),
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
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument(),
    );
    await new Promise((resolve) => window.setTimeout(resolve, 0));

    expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
      "feature/cached",
    );
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
        }),
      ),
    );
    expect(screen.getByText("Refreshing branches...")).toBeInTheDocument();
    expect(screen.getAllByText("feature/cached").length).toBeGreaterThan(0);

    await userEvent.click(screen.getByText("Project default (main)"));
    expect(
      useAgentSessionStore.getState().lastBranchBaseSelectionByProjectId[
        "project-1"
      ],
    ).toBe("project_default:main");
    expect(
      useAgentSessionStore.getState().branchBaseCacheByProjectId["project-1"]
        ?.selectedKey,
    ).toBe("project_default:main");

    // The refresh lands only now, carrying the pre-click preferred key. It must
    // repopulate the option list without reverting the explicit pick.
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
      selectedKey: "project_default:main",
      degraded: { planBranches: false, agentBranches: false },
      knownBranchRefs: ["main", "feature/cached"],
    });

    await waitFor(() =>
      expect(screen.queryByText("Refreshing branches...")).not.toBeInTheDocument(),
    );
    expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
      "Project default (main)",
    );
    expect(
      useAgentSessionStore.getState().lastBranchBaseSelectionByProjectId[
        "project-1"
      ],
    ).toBe("project_default:main");
    expect(
      useAgentSessionStore.getState().branchBaseCacheByProjectId["project-1"]
        ?.selectedKey,
    ).toBe("project_default:main");
  });

  it("starts from the explicitly picked agent branch when the branch refresh resolves late", async () => {
    mockAgentViewData();
    startAgentConversationMock.mockResolvedValue({
      conversation: conversation({
        id: "conversation-agent-base",
        contextId: "project-1",
        title: "Fix workspace repair loops",
      }),
      workspace: conversationWorkspace({
        conversationId: "conversation-agent-base",
      }),
    });
    let resolveBranchOptions:
      | ((result: unknown) => void)
      | null = null;
    loadBranchBaseOptionsMock.mockReturnValue(
      new Promise((resolve) => {
        resolveBranchOptions = resolve;
      }),
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
              key: "local_branch:ralphx/ralphx/agent-6c5acefd",
              label: "Fix workspace repair loops",
              detail: "ralphx/ralphx/agent-6c5acefd",
              source: "agent",
              selection: {
                kind: "local_branch",
                ref: "ralphx/ralphx/agent-6c5acefd",
                displayName: "Fix workspace repair loops",
              },
            },
          ],
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
      lastBranchBaseSelectionByProjectId: {
        "project-1": "project_default:main",
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument(),
    );

    // Opening the picker kicks off the slow refresh; the cached list is
    // clickable immediately, which is exactly the reported race window.
    await userEvent.click(screen.getByTestId("agents-start-base"));
    await waitFor(() => expect(loadBranchBaseOptionsMock).toHaveBeenCalled());

    await userEvent.click(screen.getByText("Fix workspace repair loops"));
    expect(screen.getByTestId("agents-start-base")).toHaveTextContent(
      "Fix workspace repair loops",
    );

    await userEvent.click(screen.getByTestId("agents-start-mode-chip"));
    await userEvent.click(screen.getByTestId("agents-start-mode-plan"));

    // Refresh resolves last, still preferring the pre-click project default.
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
        {
          key: "local_branch:ralphx/ralphx/agent-6c5acefd",
          label: "Fix workspace repair loops",
          detail: "ralphx/ralphx/agent-6c5acefd",
          source: "agent",
          selection: {
            kind: "local_branch",
            ref: "ralphx/ralphx/agent-6c5acefd",
            displayName: "Fix workspace repair loops",
          },
        },
      ],
      selectedKey: "project_default:main",
      degraded: { planBranches: false, agentBranches: false },
      knownBranchRefs: ["main", "ralphx/ralphx/agent-6c5acefd"],
    });
    await waitFor(() =>
      expect(screen.queryByText("Refreshing branches...")).not.toBeInTheDocument(),
    );

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "continue the repair loop fix" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          projectId: "project-1",
          mode: "plan",
          base: expect.objectContaining({
            kind: "local_branch",
            ref: "ralphx/ralphx/agent-6c5acefd",
            branchMode: "isolated",
          }),
        }),
      ),
    );
    expect(startAgentConversationMock).not.toHaveBeenCalledWith(
      expect.objectContaining({
        base: expect.objectContaining({ kind: "project_default" }),
      }),
    );
  });

  it("blocks the start when an explicitly picked base cannot be re-resolved", async () => {
    mockAgentViewData();
    const cachedOptions = [
      {
        key: "project_default:main",
        label: "Project default (main)",
        detail: "Configured project base branch",
        source: "project" as const,
        selection: {
          kind: "project_default" as const,
          ref: "main",
          displayName: "Project default (main)",
        },
      },
      {
        key: "local_branch:ralphx/ralphx/agent-6c5acefd",
        label: "Fix workspace repair loops",
        detail: "ralphx/ralphx/agent-6c5acefd",
        source: "agent" as const,
        selection: {
          kind: "local_branch" as const,
          ref: "ralphx/ralphx/agent-6c5acefd",
          displayName: "Fix workspace repair loops",
        },
      },
    ];
    // The refresh drops the agent branch, and so does the submit-time retry.
    let resolveBranchOptions: ((result: unknown) => void) | null = null;
    loadBranchBaseOptionsMock
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveBranchOptions = resolve;
        }),
      )
      .mockResolvedValue({
        options: [cachedOptions[0]],
        selectedKey: "project_default:main",
        degraded: { planBranches: false, agentBranches: false },
        knownBranchRefs: ["main"],
      });
    resetAgentSessionState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options: cachedOptions,
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
      lastBranchBaseSelectionByProjectId: {
        "project-1": "project_default:main",
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument(),
    );
    await userEvent.click(screen.getByTestId("agents-start-base"));
    await waitFor(() => expect(loadBranchBaseOptionsMock).toHaveBeenCalled());
    await userEvent.click(screen.getByText("Fix workspace repair loops"));

    resolveBranchOptions?.({
      options: [cachedOptions[0]],
      selectedKey: "project_default:main",
      degraded: { planBranches: false, agentBranches: false },
      knownBranchRefs: ["main"],
    });
    await waitFor(() =>
      expect(screen.queryByText("Refreshing branches...")).not.toBeInTheDocument(),
    );

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "continue the repair loop fix" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(
        screen.getByText(/could not be resolved/i),
      ).toBeInTheDocument(),
    );
    // Exactly one retry: the initial refresh plus one submit-time re-resolve.
    expect(loadBranchBaseOptionsMock).toHaveBeenCalledTimes(2);
    expect(screen.getByText(/ralphx\/ralphx\/agent-6c5acefd/)).toBeInTheDocument();
    expect(startAgentConversationMock).not.toHaveBeenCalled();
  });

  it("starts after a successful retry resolve of a dropped base branch", async () => {
    mockAgentViewData();
    startAgentConversationMock.mockResolvedValue({
      conversation: conversation({
        id: "conversation-agent-retry",
        contextId: "project-1",
        title: "Fix workspace repair loops",
      }),
      workspace: conversationWorkspace({
        conversationId: "conversation-agent-retry",
      }),
    });
    const projectDefaultOption = {
      key: "project_default:main",
      label: "Project default (main)",
      detail: "Configured project base branch",
      source: "project" as const,
      selection: {
        kind: "project_default" as const,
        ref: "main",
        displayName: "Project default (main)",
      },
    };
    const agentBranchOption = {
      key: "local_branch:ralphx/ralphx/agent-6c5acefd",
      label: "Fix workspace repair loops",
      detail: "ralphx/ralphx/agent-6c5acefd",
      source: "agent" as const,
      selection: {
        kind: "local_branch" as const,
        ref: "ralphx/ralphx/agent-6c5acefd",
        displayName: "Fix workspace repair loops",
      },
    };
    // First load drops the branch; the submit-time retry restores it.
    let resolveBranchOptions: ((result: unknown) => void) | null = null;
    loadBranchBaseOptionsMock
      .mockReturnValueOnce(
        new Promise((resolve) => {
          resolveBranchOptions = resolve;
        }),
      )
      .mockResolvedValue({
        options: [projectDefaultOption, agentBranchOption],
        selectedKey: "project_default:main",
        degraded: { planBranches: false, agentBranches: false },
        knownBranchRefs: ["main", "ralphx/ralphx/agent-6c5acefd"],
      });
    resetAgentSessionState({
      branchBaseCacheByProjectId: {
        "project-1": {
          options: [projectDefaultOption, agentBranchOption],
          selectedKey: "project_default:main",
          loadedAt: "2026-07-31T00:00:00.000Z",
        },
      },
      lastBranchBaseSelectionByProjectId: {
        "project-1": "project_default:main",
      },
    });

    renderAgentsView();

    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument(),
    );
    await userEvent.click(screen.getByTestId("agents-start-base"));
    await waitFor(() => expect(loadBranchBaseOptionsMock).toHaveBeenCalled());
    await userEvent.click(screen.getByText("Fix workspace repair loops"));

    resolveBranchOptions?.({
      options: [projectDefaultOption],
      selectedKey: "project_default:main",
      degraded: { planBranches: false, agentBranches: false },
      knownBranchRefs: ["main"],
    });
    await waitFor(() =>
      expect(screen.queryByText("Refreshing branches...")).not.toBeInTheDocument(),
    );

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "continue the repair loop fix" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          base: expect.objectContaining({
            kind: "local_branch",
            ref: "ralphx/ralphx/agent-6c5acefd",
          }),
        }),
      ),
    );
    expect(screen.queryByText(/could not be resolved/i)).not.toBeInTheDocument();
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
        }),
      ),
    );
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument(),
    );
    expect(
      screen.getByTestId("agents-conversation-workspace-line"),
    ).toHaveTextContent("agent-conversation-chat");
  });

  it("starts automation mode from the selected current branch", async () => {
    mockAgentViewData();
    vi.mocked(invoke).mockImplementation((command) =>
      command === "get_start_composer_role_default"
        ? Promise.resolve(codexRoleDefault("workspace_automation"))
        : Promise.resolve(undefined),
    );
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
        key: "current_branch:feature/automation-base",
        label: "Current branch (feature/automation-base)",
        detail: "Currently checked out in the project root",
        source: "current",
        selection: {
          kind: "current_branch",
          ref: "feature/automation-base",
          displayName: "Current branch (feature/automation-base)",
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
          selectedKey: "current_branch:feature/automation-base",
          loadedAt: "2026-07-20T00:00:00.000Z",
        },
      },
      lastBranchBaseSelectionByProjectId: {
        "project-1": "current_branch:feature/automation-base",
      },
    });
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
    await userEvent.click(
      screen.getByRole("button", { name: "Show more modes" }),
    );
    await userEvent.click(screen.getByTestId("agents-start-mode-automation"));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("get_start_composer_role_default", {
        input: { projectId: "project-1", mode: "automation" },
      }),
    );
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "set up a weekly dependency cleanup automation" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(createAutomationDraftMock).toHaveBeenCalledWith({
        projectId: "project-1",
        name: "set up a weekly dependency cleanup automation",
        base: {
          kind: "current_branch",
          branchMode: "isolated",
          ref: "feature/automation-base",
          displayName: "Current branch (feature/automation-base)",
        },
      }),
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
          mode: "automation",
        }),
      ),
    );
    const startInput = startAgentConversationMock.mock.calls[0]?.[0];
    expect(startInput).not.toHaveProperty("providerHarness");
    expect(startInput).not.toHaveProperty("modelId");
    expect(startInput).not.toHaveProperty("logicalEffort");
    await waitFor(() =>
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument(),
    );
    expect(useAgentSessionStore.getState().selectedConversationId).toBe(
      "automation-setup-conversation",
    );
  });

  it("carries trusted auto-finalize from the Automations entry into draft creation", async () => {
    mockAgentViewData();
    useAgentSessionStore.getState().setStartConversationDraft({
      projectId: "project-1",
      content: "",
      mode: "automation",
      automationAuthoringMode: "trusted_auto_finalize",
    });
    createAutomationDraftMock.mockResolvedValue({
      automation: {
        id: "automation-trusted",
        projectId: "project-1",
        name: "ship the trusted pipeline",
        status: "draft",
        pausedReasonCode: null,
        pausedReasonDetail: null,
        goalPrompt: "",
        setupConversationId: "automation-trusted-conversation",
        specArtifactId: null,
        authoringMode: "trusted_auto_finalize",
        decompositionVerificationStatus: "unverified",
        decompositionVerificationVerdictJson: null,
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
        planApprovalMode: "manual",
        prMergeMode: "manual",
        planDeepVerification: false,
        maxRuns: 25,
        maxConsecutiveFailures: 3,
        firstRunPrompt: null,
        setupAnalysisSummary: null,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      },
      setupConversationId: "automation-trusted-conversation",
    });
    startAgentConversationMock.mockResolvedValue({
      conversation: conversation({
        id: "automation-trusted-conversation",
        contextId: "project-1",
        agentMode: "automation",
        automationId: "automation-trusted",
      }),
      workspace: null,
      sendResult: {
        conversationId: "automation-trusted-conversation",
        agentRunId: "run-automation-trusted",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });

    renderAgentsView();
    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "ship the trusted pipeline" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(createAutomationDraftMock).toHaveBeenCalledWith({
        projectId: "project-1",
        name: "ship the trusted pipeline",
        authoringMode: "trusted_auto_finalize",
        base: {
          kind: "project_default",
          branchMode: "isolated",
          ref: "main",
          displayName: "Project default (main)",
        },
      }),
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
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument(),
    );
    fireEvent.change(screen.getByLabelText("Message input"), {
      target: { value: "continue this run" },
    });

    expect(screen.getByTestId("agents-conversation-submit")).toBeDisabled();
    expect(
      screen.getByTestId("agents-conversation-provider-status"),
    ).toHaveTextContent("Codex CLI not found");
    fireEvent.click(
      screen.getByTestId("agents-conversation-provider-status-settings"),
    );

    expect(useUiStore.getState().activeModal).toBe("settings");
    expect(useUiStore.getState().modalContext).toEqual({
      section: "providers",
    });
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
      expect(screen.getByTestId("integrated-chat-panel")).toBeInTheDocument(),
    );

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(await screen.findByText("Archive session"));
    await user.click(screen.getByRole("button", { name: "Archive session" }));

    await waitFor(() =>
      expect(archiveConversationMock).toHaveBeenCalledWith("conversation-1", {
        closePullRequest: false,
      }),
    );
    await waitFor(() =>
      expect(screen.getByTestId("agents-start-composer")).toBeInTheDocument(),
    );
    expect(
      screen.queryByTestId("integrated-chat-panel"),
    ).not.toBeInTheDocument();
    expect(invalidateSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        queryKey: [
          "agents",
          "project-conversations",
          "project-1",
          "archived-count",
        ],
        refetchType: "active",
      }),
    );

    invalidateSpy.mockRestore();
  });

  it("uploads starter attachments against a seeded conversation before sending the first message", async () => {
    mockAgentViewData();
    createConversationMock.mockResolvedValue(
      conversation({ id: "conversation-seeded", contextId: "project-1" }),
    );
    startAgentConversationMock.mockResolvedValue({
      conversation: conversation({
        id: "conversation-seeded",
        contextId: "project-1",
      }),
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
        expect(createConversationMock).toHaveBeenCalledWith(
          "project",
          "project-1",
        ),
      );
      await waitFor(() =>
        expect(invoke).toHaveBeenCalledWith("upload_chat_attachment", {
          input: expect.objectContaining({
            conversationId: "conversation-seeded",
            fileName: "screenshot.png",
            mimeType: "image/png",
          }),
        }),
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
          }),
        ),
      );
      await waitFor(() =>
        expect(
          queryClient.getQueryData([
            "chat",
            "conversations",
            "conversation-seeded",
          ]),
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
        }),
      );
      expect(createObjectURL).toHaveBeenCalledWith(file);
    } finally {
      Object.defineProperty(URL, "createObjectURL", {
        value: originalCreateObjectURL,
        configurable: true,
      });
    }
  });

  it("restores unsent starter composer folders from the draft store", async () => {
    mockAgentViewData();
    useChatStore
      .getState()
      .setComposerDraftFolders("agents:start", [
        {
          id: "draft-folder-1",
          folderPath: "/work/design-notes",
          displayName: "design-notes",
        },
      ]);

    const { queryClient } = renderAgentsView();
    queryClient.setQueryData(
      FEATURE_FLAGS_QUERY_KEY,
      enabledFeatureFlags({
        standaloneConversations: true,
      }),
    );

    expect(
      await screen.findByTestId("draft-folder-reference-chips"),
    ).toHaveTextContent("design-notes");

    await userEvent.click(screen.getByTestId("agents-start-project"));
    await userEvent.click(
      screen.getByTestId("agents-start-project-standalone"),
    );
    expect(
      screen.getByTestId("draft-folder-reference-chips"),
    ).toHaveTextContent("design-notes");
  });

  it("registers a pre-send picked folder against the seeded conversation before sending the first message", async () => {
    mockAgentViewData();
    createConversationMock.mockResolvedValue(
      conversation({
        id: "conversation-folder-seeded",
        contextId: "project-1",
      }),
    );
    startAgentConversationMock.mockResolvedValue({
      conversation: conversation({
        id: "conversation-folder-seeded",
        contextId: "project-1",
      }),
      workspace: conversationWorkspace({
        conversationId: "conversation-folder-seeded",
      }),
      sendResult: {
        conversationId: "conversation-folder-seeded",
        agentRunId: "run-folder-1",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });
    vi.mocked(openDialog).mockResolvedValue(
      "/Users/test/projects/test-project",
    );
    vi.mocked(invoke).mockImplementation((cmd) => {
      if (cmd === "add_conversation_folder_reference") {
        return Promise.resolve({
          id: "folder-ref-1",
          conversationId: "conversation-folder-seeded",
          folderPath: "/Users/test/projects/test-project",
          displayName: "test-project",
          createdAt: "2026-01-01T00:00:00Z",
        });
      }
      return Promise.resolve(undefined);
    });

    const { queryClient } = renderAgentsView();
    queryClient.setQueryData(FEATURE_FLAGS_QUERY_KEY, enabledFeatureFlags());

    fireEvent.click(await screen.findByTestId("agent-composer-actions-menu"));
    fireEvent.click(screen.getByRole("button", { name: "Add folder" }));

    expect(await screen.findByText("test-project")).toBeInTheDocument();

    fireEvent.change(screen.getByTestId("agents-start-textarea"), {
      target: { value: "review this folder" },
    });
    fireEvent.click(screen.getByTestId("agents-start-submit"));

    await waitFor(() =>
      expect(createConversationMock).toHaveBeenCalledWith(
        "project",
        "project-1",
      ),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("add_conversation_folder_reference", {
        input: {
          conversationId: "conversation-folder-seeded",
          folderPath: "/Users/test/projects/test-project",
          displayName: "test-project",
        },
      }),
    );
    await waitFor(() =>
      expect(startAgentConversationMock).toHaveBeenCalledWith(
        expect.objectContaining({
          conversationId: "conversation-folder-seeded",
          content: "review this folder",
        }),
      ),
    );
  });
});
