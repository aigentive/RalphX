import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ComponentProps, ReactNode } from "react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { AgentConversationWorkspace, ForkAgentConversationResult } from "@/api/chat";

import type { AgentConversation } from "./agentConversations";
import { AgentsActiveConversationPanel } from "./AgentsActiveConversationPanel";

vi.mock("@/components/Chat/IntegratedChatPanel", () => ({
  IntegratedChatPanel: ({ renderComposer }: { renderComposer: (props: Record<string, unknown>) => ReactNode }) => (
    <div data-testid="integrated-chat-panel">
      {renderComposer({
        onSend: vi.fn(),
        onStop: vi.fn(),
        agentStatus: "idle",
        isSending: false,
        isReadOnly: false,
        autoFocus: false,
        hasQueuedMessages: false,
        onEditLastQueued: vi.fn(),
        attachments: [],
        enableAttachments: false,
        onFilesSelected: vi.fn(),
        onRemoveAttachment: vi.fn(),
        attachmentsUploading: false,
      })}
    </div>
  ),
}));

vi.mock("@/hooks/useAgentModels", () => ({
  useAgentModels: () => ({
    isReady: true,
    registry: {
      claude: [
        {
          id: "sonnet",
          label: "sonnet",
          menuLabel: "sonnet",
          defaultEffort: "medium",
          supportedEfforts: ["low", "medium", "high", "max"],
        },
        {
          id: "opus",
          label: "opus",
          menuLabel: "opus",
          defaultEffort: "xhigh",
          supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
        },
      ],
      codex: [
        {
          id: "gpt-5.5",
          label: "gpt-5.5",
          menuLabel: "gpt-5.5",
          defaultEffort: "xhigh",
          supportedEfforts: ["low", "medium", "high", "xhigh"],
        },
      ],
    },
  }),
}));

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: () => ({
    providers: [
      {
        provider: "claude",
        enabled: true,
        isDefault: true,
        model: null,
        effort: null,
        approvalPolicy: null,
        sandboxMode: null,
        claudePermissionMode: null,
        claudeDangerouslySkipPermissions: false,
        claudeAllowDangerouslySkipPermissions: false,
        available: true,
        binaryFound: true,
        binaryPath: "/tmp/claude",
        status: "ready",
        error: null,
        missingCoreExecFeatures: [],
        supportedEfforts: ["low", "medium", "high", "max"],
        updatedAt: "2026-05-16T00:00:00.000Z",
      },
    ],
    isLoading: false,
    isPlaceholderData: false,
  }),
}));

vi.mock("@/stores/chatStore", () => ({
  selectQueuedMessages: () => () => [],
  useChatStore: () => [],
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: (selector: (state: { openModal: () => void; executionStatus: { isPaused: boolean } }) => unknown) =>
    selector({ openModal: vi.fn(), executionStatus: { isPaused: false } }),
}));

vi.mock("./AgentComposerSurface", () => ({
  AgentComposerSurface: ({
    model,
    effort,
    showHelperText,
    onSend,
    onForkSession,
  }: {
    model: { value: string; onValueChange: (value: string) => void };
    effort: { value: string; onValueChange: (value: string) => void };
    showHelperText?: boolean;
    onSend: (message: string) => Promise<void> | void;
    onForkSession?: () => Promise<unknown> | void;
  }) => (
    <div>
      <div data-testid="workspace-model-value">{model.value}</div>
      <div data-testid="workspace-effort-value">{effort.value}</div>
      <div data-testid="workspace-helper-enabled">{String(showHelperText !== false)}</div>
      <button
        type="button"
        data-testid="change-workspace-model"
        onClick={() => model.onValueChange("sonnet")}
      />
      <button
        type="button"
        data-testid="change-workspace-effort"
        onClick={() => effort.onValueChange("max")}
      />
      <button
        type="button"
        data-testid="send-fork-command"
        onClick={() => void onSend("/fork")}
      />
      <button
        type="button"
        data-testid="composer-fork-action"
        onClick={() => void onForkSession?.()}
      />
    </div>
  ),
  AgentComposerProjectLine: () => null,
}));

vi.mock("./AgentConversationBaseLine", () => ({
  AgentConversationBaseLine: () => null,
}));

vi.mock("./AgentsChatHeaderController", () => ({
  AgentsChatHeaderController: () => null,
}));

vi.mock("./AgentProviderSettingsButton", () => ({
  AgentProviderSettingsButton: () => null,
}));

vi.mock("./AgentsTerminalRegion", () => ({
  AgentsTerminalDockHost: () => null,
}));

function projectConversation(): AgentConversation {
  return {
    id: "conversation-1",
    contextType: "project",
    contextId: "project-1",
    projectId: "project-1",
    ideationSessionId: null,
    claudeSessionId: null,
    providerSessionId: null,
    providerHarness: null,
    agentMode: "ideation",
    title: "Conversation",
    messageCount: 0,
    lastMessageAt: null,
    createdAt: "2026-05-16T00:00:00.000Z",
    updatedAt: "2026-05-16T00:00:00.000Z",
    archivedAt: null,
  };
}

function workspace(): AgentConversationWorkspace {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    mode: "ideation",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    baseCommit: null,
    branchName: "ralphx/conversation-1",
    worktreePath: "/tmp/conversation-1",
    linkedIdeationSessionId: null,
    linkedPlanBranchId: null,
    modeSwitchLocked: false,
    modeSwitchLockReason: null,
    publicationPrNumber: null,
    publicationPrUrl: null,
    publicationPrStatus: null,
    publicationPushStatus: null,
    status: "active",
    createdAt: "2026-05-16T00:00:00.000Z",
    updatedAt: "2026-05-16T00:00:00.000Z",
  };
}

function forkResult(): ForkAgentConversationResult {
  return {
    parentConversation: projectConversation() as never,
    conversation: { ...projectConversation(), id: "conversation-fork" } as never,
    workspace: null,
    providerSessionForked: true,
    copiedMessageCount: 2,
    copiedTimelineItemCount: 0,
  };
}

function renderPanel(
  overrides: Partial<ComponentProps<typeof AgentsActiveConversationPanel>> = {},
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const props: ComponentProps<typeof AgentsActiveConversationPanel> = {
    activeConversation: projectConversation(),
    activeConversationMode: "ideation",
    activeConversationModeLocked: false,
    activeProjectId: "project-1",
    activeProjectOptions: [{ id: "project-1", label: "RalphX" }],
    activeWorkspace: workspace(),
    activeWorkspaceFreshness: undefined,
    attachedIdeationSessionId: null,
    availableArtifactTabs: [],
    chatFocus: { type: "workspace" },
    chatFocusOptions: [],
    hasAutoOpenArtifacts: false,
    normalizedActiveRuntime: {
      provider: "claude",
      modelId: "opus",
      effort: "xhigh",
    },
    onActiveConversationModeChange: vi.fn(),
    onActiveConversationModeMenuOpen: vi.fn(),
    onActiveEffortChange: vi.fn(),
    onActiveModelChange: vi.fn(),
    onAgentUserMessageSent: vi.fn(),
    onFocusIdeationSession: vi.fn(),
    onForkConversation: vi.fn().mockResolvedValue(forkResult()),
    onOpenPublishPane: vi.fn(),
    onOpenPublishFile: vi.fn(),
    onPreloadArtifacts: vi.fn(),
    onPublishWorkspace: vi.fn(),
    onRenameConversation: vi.fn(),
    onSelectArtifact: vi.fn(),
    onToggleArtifacts: vi.fn(),
    onSelectChatFocus: vi.fn(),
    publishShortcutLabel: "P",
    publishingConversationId: null,
    selectedConversationId: "conversation-1",
    setTerminalChatDockElement: vi.fn(),
    switchingConversationModeId: null,
    terminalUnavailableReason: null,
    ...overrides,
  };
  render(
    <QueryClientProvider client={queryClient}>
      <AgentsActiveConversationPanel {...props} />
    </QueryClientProvider>,
  );
  return props;
}

describe("AgentsActiveConversationPanel", () => {
  it("normalizes workspace runtime and forwards provider-supported efforts", () => {
    const onActiveModelChange = vi.fn();
    const onActiveEffortChange = vi.fn();
    renderPanel({ onActiveEffortChange, onActiveModelChange });

    expect(screen.getByTestId("workspace-effort-value").textContent).toBe("high");
    expect(screen.getByTestId("workspace-helper-enabled").textContent).toBe("true");

    fireEvent.click(screen.getByTestId("change-workspace-model"));
    fireEvent.click(screen.getByTestId("change-workspace-effort"));

    expect(onActiveModelChange).toHaveBeenCalledWith("sonnet", [
      "low",
      "medium",
      "high",
      "max",
    ]);
    expect(onActiveEffortChange).toHaveBeenCalledWith("max", [
      "low",
      "medium",
      "high",
      "max",
    ]);
  });

  it("requires confirmation before running the typed fork command", async () => {
    const user = userEvent.setup();
    const onForkConversation = vi.fn().mockResolvedValue(forkResult());
    renderPanel({ onForkConversation });

    await user.click(screen.getByTestId("send-fork-command"));

    expect(screen.getByText("Fork session?")).toBeInTheDocument();
    expect(onForkConversation).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Fork session" }));

    await waitFor(() =>
      expect(onForkConversation).toHaveBeenCalledWith("conversation-1"),
    );
  });

  it("requires confirmation before running the composer fork action", async () => {
    const user = userEvent.setup();
    const onForkConversation = vi.fn().mockResolvedValue(forkResult());
    renderPanel({ onForkConversation });

    await user.click(screen.getByTestId("composer-fork-action"));

    expect(screen.getByText("Fork session?")).toBeInTheDocument();
    expect(onForkConversation).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Fork session" }));

    await waitFor(() =>
      expect(onForkConversation).toHaveBeenCalledWith("conversation-1"),
    );
  });
});
