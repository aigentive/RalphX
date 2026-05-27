import { fireEvent, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import type { AgentProvidersSettingsResponse } from "@/api/harness-providers";
import { ideationApi } from "@/api/ideation";
import {
  DEFAULT_SIDEBAR_PUBLICATION_STATE_FILTERS,
  useAgentSessionStore,
} from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import { useAgentArtifactUiStore } from "./agentArtifactUiStore";
import { useAgentTerminalStore } from "./agentTerminalStore";
import type { AgentConversation } from "./agentConversations";
import { AgentsView } from "./AgentsView";
import {
  agentProjectFixture as project,
  agentRuntimeFixture as runtime,
  conversationFixture as conversation,
  renderWithAgentProviders as renderWithProviders,
} from "./agentsTestFixtures";


const agentsViewTestMocks = vi.hoisted(() => ({
  useProjectsMock: vi.fn(),
  useHarnessProvidersMock: vi.fn(),
  useProjectAgentConversationsMock: vi.fn(),
  useAgentSidebarProjectGroupMock: vi.fn(),
  useAgentSidebarPublicationGroupMock: vi.fn(),
  useConversationMock: vi.fn(),
  startAgentConversationMock: vi.fn(),
  getAgentConversationWorkspaceMock: vi.fn(),
  getAgentConversationWorkspaceFreshnessMock: vi.fn(),
  listAgentConversationWorkspacesByProjectMock: vi.fn(),
  listWorkspaceOpenTargetsMock: vi.fn(),
  openAgentConversationWorkspacePathMock: vi.fn(),
  listConversationsMock: vi.fn(),
  publishAgentConversationWorkspaceMock: vi.fn(),
  setAgentConversationWorkspaceAutoPublishMock: vi.fn(),
  setAgentConversationWorkspacePrSupervisionMock: vi.fn(),
  precomputePrDescriptionMock: vi.fn(),
  switchAgentConversationModeMock: vi.fn(),
  sendAgentMessageMock: vi.fn(),
  createConversationMock: vi.fn(),
  spawnConversationSessionNamerMock: vi.fn(),
  updateConversationTitleMock: vi.fn(),
  archiveConversationMock: vi.fn(),
  restoreConversationMock: vi.fn(),
  getAgentRunningStatesMock: vi.fn(),
  getPlanBranchesMock: vi.fn(),
  listIdeationSessionsMock: vi.fn(),
  getLatestChildSessionIdMock: vi.fn(),
  getWorkspaceChangesMock: vi.fn(),
  getWorkspaceChangeSummaryMock: vi.fn(),
  getWorkspaceReviewMock: vi.fn(),
  getWorkspaceDiffMock: vi.fn(),
  getWorkspaceCommitsMock: vi.fn(),
  getWorkspaceCommitChangesMock: vi.fn(),
  getWorkspaceCommitDiffMock: vi.fn(),
  getWorkspaceStagedChangesMock: vi.fn(),
  getWorkspaceUnstagedChangesMock: vi.fn(),
  getWorkspaceCumulativeChangesMock: vi.fn(),
  getWorkspaceStagedDiffMock: vi.fn(),
  getWorkspaceUnstagedDiffMock: vi.fn(),
  getWorkspaceCumulativeDiffMock: vi.fn(),
  listAgentTasksMock: vi.fn(),
  listAgentTaskListsMock: vi.fn(),
  listAgentTaskListTasksMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  integratedChatPanelRenderMock: vi.fn(),
  preloadAgentsArtifactPaneMock: vi.fn(),
  preloadAgentTerminalExperienceMock: vi.fn(),
  artifactPaneModuleLoadedMock: vi.fn(),
  terminalDrawerModuleLoadedMock: vi.fn(),
  terminalDrawerMountMock: vi.fn(),
  terminalDrawerUnmountMock: vi.fn(),
  webviewDragDropHandlers: [] as Array<(event: { payload: unknown }) => unknown>,
  webviewOnDragDropEventMock: vi.fn(),
  webviewDragDropUnlistenMock: vi.fn(),
}));

const eventSubscriptions = new Map<string, ((payload: unknown) => void)[]>();
const providerUpdatedAt = new Date().toISOString();
const defaultProviderSettings: AgentProvidersSettingsResponse = {
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
      updatedAt: providerUpdatedAt,
    },
  ],
};

const {
  useProjectsMock,
  useHarnessProvidersMock,
  useProjectAgentConversationsMock,
  useAgentSidebarProjectGroupMock,
  useAgentSidebarPublicationGroupMock,
  useConversationMock,
  startAgentConversationMock,
  getAgentConversationWorkspaceMock,
  getAgentConversationWorkspaceFreshnessMock,
  listAgentConversationWorkspacesByProjectMock,
  listWorkspaceOpenTargetsMock,
  openAgentConversationWorkspacePathMock,
  listConversationsMock,
  publishAgentConversationWorkspaceMock,
  setAgentConversationWorkspaceAutoPublishMock,
  setAgentConversationWorkspacePrSupervisionMock,
  precomputePrDescriptionMock,
  switchAgentConversationModeMock,
  sendAgentMessageMock,
  createConversationMock,
  spawnConversationSessionNamerMock,
  updateConversationTitleMock,
  archiveConversationMock,
  restoreConversationMock,
  getAgentRunningStatesMock,
  getPlanBranchesMock,
  listIdeationSessionsMock,
  getLatestChildSessionIdMock,
  getWorkspaceChangesMock,
  getWorkspaceChangeSummaryMock,
  getWorkspaceReviewMock,
  getWorkspaceDiffMock,
  getWorkspaceCommitsMock,
  getWorkspaceCommitChangesMock,
  getWorkspaceCommitDiffMock,
  getWorkspaceStagedChangesMock,
  getWorkspaceUnstagedChangesMock,
  getWorkspaceCumulativeChangesMock,
  getWorkspaceStagedDiffMock,
  getWorkspaceUnstagedDiffMock,
  getWorkspaceCumulativeDiffMock,
  listAgentTasksMock,
  listAgentTaskListsMock,
  listAgentTaskListTasksMock,
  toastErrorMock,
  toastSuccessMock,
  integratedChatPanelRenderMock,
  preloadAgentsArtifactPaneMock,
  preloadAgentTerminalExperienceMock,
  artifactPaneModuleLoadedMock,
  terminalDrawerModuleLoadedMock,
  terminalDrawerMountMock,
  terminalDrawerUnmountMock,
  webviewDragDropHandlers,
  webviewOnDragDropEventMock,
  webviewDragDropUnlistenMock,
} = agentsViewTestMocks;

export function getAgentsViewTestMocks() {
  return agentsViewTestMocks;
}

vi.mock("react-virtuoso", async () => {
  const React = await vi.importActual<typeof import("react")>("react");

  type VirtuosoMockRange = {
    startIndex: number;
    endIndex: number;
  };
  type VirtuosoMockProps = {
    className?: string;
    computeItemKey?: (index: number, item: unknown) => React.Key;
    data?: unknown[];
    "data-testid"?: string;
    endReached?: (index: number) => void;
    itemContent?: (index: number, item: unknown) => React.ReactNode;
    rangeChanged?: (range: VirtuosoMockRange) => void;
    scrollerRef?: (node: HTMLElement | Window | null) => void;
    style?: React.CSSProperties;
    totalCount?: number;
  };

  const Virtuoso = React.forwardRef<unknown, VirtuosoMockProps>(function MockVirtuoso(
    props,
    ref
  ) {
    const {
      className,
      computeItemKey,
      data: dataProp,
      itemContent,
      rangeChanged,
      scrollerRef,
      style,
      totalCount,
    } = props;
    const data =
      dataProp ??
      Array.from({ length: totalCount ?? 0 }, () => undefined);
    const endIndex = data.length - 1;

    React.useImperativeHandle(
      ref,
      () => ({
        getState: (stateCb: (state: unknown) => void) => {
          stateCb({
            ranges: [],
            scrollTop: 0,
          });
        },
        scrollToIndex: vi.fn(),
      }),
      []
    );

    React.useEffect(() => {
      if (endIndex < 0) {
        return;
      }
      rangeChanged?.({ startIndex: 0, endIndex });
    }, [endIndex, rangeChanged]);

    const setScrollerRef = React.useCallback((node: HTMLDivElement | null) => {
      scrollerRef?.(node);
    }, [scrollerRef]);

    return (
      <div
        ref={setScrollerRef}
        data-testid={props["data-testid"] ?? "mock-virtuoso"}
        className={className}
        style={style}
      >
        {data.map((item, index) => (
          <div key={computeItemKey?.(index, item) ?? index}>
            {itemContent?.(index, item)}
          </div>
        ))}
      </div>
    );
  });

  return { Virtuoso };
});

export function mockHarnessProviders(
  settings: AgentProvidersSettingsResponse = defaultProviderSettings,
  overrides: Record<string, unknown> = {},
) {
  useHarnessProvidersMock.mockReturnValue({
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
    ...overrides,
  });
}

export function fireAgentViewEvent<T>(event: string, payload: T) {
  const handlers = eventSubscriptions.get(event);
  if (!handlers) {
    return;
  }
  for (const handler of handlers) {
    handler(payload);
  }
}

export async function fireAgentViewNativeDragDropEvent(payload: unknown) {
  for (const handler of [...webviewDragDropHandlers]) {
    await handler({ payload });
  }
}

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (handler: (event: { payload: unknown }) => unknown) => {
      webviewOnDragDropEventMock(handler);
      webviewDragDropHandlers.push(handler);
      return Promise.resolve(() => {
        webviewDragDropUnlistenMock(handler);
        const index = webviewDragDropHandlers.indexOf(handler);
        if (index >= 0) {
          webviewDragDropHandlers.splice(index, 1);
        }
      });
    },
  }),
}));

vi.mock("@/hooks/useProjects", () => ({
  useProjects: () => useProjectsMock(),
}));

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: () => useHarnessProvidersMock(),
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

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, handler: (payload: unknown) => void) => {
      const handlers = eventSubscriptions.get(event) ?? [];
      handlers.push(handler);
      eventSubscriptions.set(event, handlers);
      return () => {
        const currentHandlers = eventSubscriptions.get(event);
        if (!currentHandlers) {
          return;
        }
        const nextHandlers = currentHandlers.filter(
          (currentHandler) => currentHandler !== handler,
        );
        if (nextHandlers.length === 0) {
          eventSubscriptions.delete(event);
          return;
        }
        eventSubscriptions.set(event, nextHandlers);
      };
    },
    emit: vi.fn(),
  }),
}));

vi.mock("./useProjectAgentConversations", () => ({
  agentConversationKeys: {
    all: ["agents", "project-conversations"],
    project: (projectId: string) => ["agents", "project-conversations", projectId],
    projectList: (projectId: string, includeArchived: boolean, search = "") => [
      "agents",
      "project-conversations",
      projectId,
      "archived",
      includeArchived,
      "search",
      search.trim().toLowerCase(),
    ],
  },
  useProjectAgentConversations: (
    projectId: string | null | undefined,
    includeArchived = false,
    options?: { search?: string; enabled?: boolean }
  ) => useProjectAgentConversationsMock(projectId, includeArchived, options),
}));

vi.mock("./useAgentSidebarPublicationGroup", () => ({
  agentSidebarConversationKeys: {
    all: ["agents", "sidebar-conversations"],
    projectGroup: (
      projectId: string | null | undefined,
      archivedOnly: boolean,
      search = "",
      publicationStates: string[] = [],
      pinnedConversationIds: string[] = []
    ) => [
      "agents",
      "sidebar-conversations",
      "project",
      projectId ?? "",
      "archived",
      archivedOnly,
      "search",
      search.trim().toLowerCase(),
      "states",
      publicationStates,
      "pinned",
      pinnedConversationIds,
    ],
    publicationGroup: (
      projectIds: string[],
      publicationState: string,
      archivedOnly: boolean,
      search = "",
      pinnedConversationIds: string[] = []
    ) => [
      "agents",
      "sidebar-conversations",
      "publication",
      publicationState,
      "projects",
      projectIds,
      "archived",
      archivedOnly,
      "search",
      search.trim().toLowerCase(),
      "pinned",
      pinnedConversationIds,
    ],
  },
  useAgentSidebarProjectGroup: (args: Record<string, unknown>) =>
    useAgentSidebarProjectGroupMock(args),
  useAgentSidebarPublicationGroup: (args: Record<string, unknown>) =>
    useAgentSidebarPublicationGroupMock(args),
  useProjectGroupLatestOrder: () => ({ data: undefined }),
}));

vi.mock("./AgentTerminalDrawer", async () => {
  terminalDrawerModuleLoadedMock();
  const React = await vi.importActual<typeof import("react")>("react");
  const ReactDom = await vi.importActual<typeof import("react-dom")>("react-dom");

  return {
    AgentTerminalDrawer: ({
      placement,
      expanded,
      onPlacementChange,
      onPlacementDragStart,
      onPlacementDragEnd,
      onExpand,
      onCollapse,
      dockElement,
    }: {
      placement: string;
      expanded: boolean;
      onPlacementChange: (placement: "auto" | "chat" | "panel") => void;
      onPlacementDragStart: () => void;
      onPlacementDragEnd: () => void;
      onExpand: () => void;
      onCollapse: () => void;
      dockElement: HTMLElement | null;
    }) => {
      React.useEffect(() => {
        terminalDrawerMountMock();
        return () => {
          terminalDrawerUnmountMock();
        };
      }, []);

      const drawer = (
        <div
          data-testid="agent-terminal-drawer"
          data-placement={placement}
          data-expanded={String(expanded)}
        >
          <div
            data-testid="agent-terminal-drag-handle"
            draggable
            onDragStart={(event) => {
              const dataTransfer = (event as unknown as {
                dataTransfer?: DataTransfer;
              }).dataTransfer;
              dataTransfer?.setData("text/plain", "conversation-1");
              onPlacementDragStart();
            }}
            onDragEnd={onPlacementDragEnd}
          >
            drag
          </div>
          <button
            type="button"
            data-testid="agent-terminal-placement"
          >
            {placement}
          </button>
          {(["auto", "chat", "panel"] as const).map((nextPlacement) => (
            <button
              key={nextPlacement}
              type="button"
              data-testid={`agent-terminal-placement-${nextPlacement}`}
              onClick={() => onPlacementChange(nextPlacement)}
            >
              {nextPlacement}
            </button>
          ))}
          <button
            type="button"
            data-testid="agent-terminal-expand"
            onClick={onExpand}
          >
            expand
          </button>
          <button
            type="button"
            data-testid="agent-terminal-collapse"
            onClick={onCollapse}
          >
            collapse
          </button>
        </div>
      );

      return dockElement ? ReactDom.createPortal(drawer, dockElement) : drawer;
    },
  };
});

vi.mock("./agentArtifactPanePreload", () => ({
  preloadAgentsArtifactPane: () => {
    preloadAgentsArtifactPaneMock();
    return import("./AgentsArtifactPane");
  },
}));

vi.mock("./agentTerminalPreload", () => ({
  preloadAgentTerminalDrawer: () => import("./AgentTerminalDrawer"),
  preloadAgentTerminalExperience: (...args: unknown[]) =>
    preloadAgentTerminalExperienceMock(...args),
}));

vi.mock("@/hooks/useChat", () => ({
  chatKeys: {
    conversation: (conversationId: string) => ["chat", "conversations", conversationId],
    conversationSummary: (conversationId: string) => [
      "chat",
      "conversations",
      conversationId,
      "summary",
    ],
    conversationHistory: (conversationId: string) => [
      "chat",
      "conversations",
      conversationId,
      "history",
    ],
    conversationTimeline: (conversationId: string) => [
      "chat",
      "conversations",
      conversationId,
      "timeline",
    ],
    conversationList: (contextType: string, contextId: string) => [
      "chat",
      "conversations",
      contextType,
      contextId,
    ],
  },
  createOptimisticConversationId: () =>
    `optimistic-conversation:test-${Date.now()}-${Math.random().toString(36).slice(2)}`,
  isOptimisticConversationId: (conversationId: string | null | undefined) =>
    Boolean(conversationId?.startsWith("optimistic-conversation:")),
  invalidateConversationDataQueries: vi.fn(),
  useConversation: (conversationId: string | null) => useConversationMock(conversationId),
  useConversationSummary: (conversationId: string | null) => {
    const query = useConversationMock(conversationId);
    return {
      ...query,
      data: query.data?.conversation ?? null,
    };
  },
  useConversationHistoryWindow: (conversationId: string | null) => {
    const query = useConversationMock(conversationId);
    return {
      ...query,
      loadedStartIndex: 0,
      hasOlderMessages: false,
      isFetchingOlderMessages: false,
      fetchOlderMessages: vi.fn(),
    };
  },
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    startAgentConversation: (...args: unknown[]) => startAgentConversationMock(...args),
    getAgentConversationWorkspace: (...args: unknown[]) =>
      getAgentConversationWorkspaceMock(...args),
    getAgentConversationWorkspaceFreshness: (...args: unknown[]) =>
      getAgentConversationWorkspaceFreshnessMock(...args),
    listAgentConversationWorkspacesByProject: (...args: unknown[]) =>
      listAgentConversationWorkspacesByProjectMock(...args),
    listConversations: (...args: unknown[]) => listConversationsMock(...args),
    publishAgentConversationWorkspace: (...args: unknown[]) =>
      publishAgentConversationWorkspaceMock(...args),
    setAgentConversationWorkspaceAutoPublish: (...args: unknown[]) =>
      setAgentConversationWorkspaceAutoPublishMock(...args),
    setAgentConversationWorkspacePrSupervision: (...args: unknown[]) =>
      setAgentConversationWorkspacePrSupervisionMock(...args),
    precomputeAgentConversationWorkspacePrDescription: (...args: unknown[]) =>
      precomputePrDescriptionMock(...args),
    switchAgentConversationMode: (...args: unknown[]) =>
      switchAgentConversationModeMock(...args),
    sendAgentMessage: (...args: unknown[]) => sendAgentMessageMock(...args),
    createConversation: (...args: unknown[]) => createConversationMock(...args),
    spawnConversationSessionNamer: (...args: unknown[]) =>
      spawnConversationSessionNamerMock(...args),
    updateConversationTitle: (...args: unknown[]) => updateConversationTitleMock(...args),
    archiveConversation: (...args: unknown[]) => archiveConversationMock(...args),
    restoreConversation: (...args: unknown[]) => restoreConversationMock(...args),
    getAgentRunningStates: (...args: unknown[]) => getAgentRunningStatesMock(...args),
    listWorkspaceOpenTargets: (...args: unknown[]) =>
      listWorkspaceOpenTargetsMock(...args),
    openAgentConversationWorkspacePath: (...args: unknown[]) =>
      openAgentConversationWorkspacePathMock(...args),
    getBulkWorkspacePublicationStates: vi.fn().mockResolvedValue({}),
  },
}));

vi.mock("@/api/ideation", () => ({
  ideationApi: {
    sessions: {
      get: vi.fn(),
      getWithData: vi.fn(),
      getLatestChildSessionId: (...args: unknown[]) =>
        getLatestChildSessionIdMock(...args),
      list: (...args: unknown[]) => listIdeationSessionsMock(...args),
      updateTitle: vi.fn(),
      archive: vi.fn(),
      reopen: vi.fn(),
    },
  },
}));

vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspaceFileChanges: (...args: unknown[]) =>
      getWorkspaceChangesMock(...args),
    getAgentConversationWorkspaceChangeSummary: (...args: unknown[]) =>
      getWorkspaceChangeSummaryMock(...args),
    getAgentConversationWorkspaceReview: (...args: unknown[]) =>
      getWorkspaceReviewMock(...args),
    getAgentConversationWorkspaceFileDiff: (...args: unknown[]) =>
      getWorkspaceDiffMock(...args),
    getAgentConversationWorkspaceCommits: (...args: unknown[]) =>
      getWorkspaceCommitsMock(...args),
    getAgentConversationWorkspaceCommitFileChanges: (...args: unknown[]) =>
      getWorkspaceCommitChangesMock(...args),
    getAgentConversationWorkspaceCommitFileDiff: (...args: unknown[]) =>
      getWorkspaceCommitDiffMock(...args),
    getAgentConversationWorkspaceStagedFileChanges: (...args: unknown[]) =>
      getWorkspaceStagedChangesMock(...args),
    getAgentConversationWorkspaceUnstagedFileChanges: (...args: unknown[]) =>
      getWorkspaceUnstagedChangesMock(...args),
    getAgentConversationWorkspaceCumulativeFileChanges: (...args: unknown[]) =>
      getWorkspaceCumulativeChangesMock(...args),
    getAgentConversationWorkspaceStagedFileDiff: (...args: unknown[]) =>
      getWorkspaceStagedDiffMock(...args),
    getAgentConversationWorkspaceUnstagedFileDiff: (...args: unknown[]) =>
      getWorkspaceUnstagedDiffMock(...args),
    getAgentConversationWorkspaceCumulativeFileDiff: (...args: unknown[]) =>
      getWorkspaceCumulativeDiffMock(...args),
  },
}));

vi.mock("@/api/agent-tasks", () => ({
  agentTaskApi: {
    listConversationTasks: (...args: unknown[]) => listAgentTasksMock(...args),
    listConversationTaskLists: (...args: unknown[]) =>
      listAgentTaskListsMock(...args),
    listConversationTaskListTasks: (...args: unknown[]) =>
      listAgentTaskListTasksMock(...args),
  },
}));

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

vi.mock("@/api/plan-branch", () => ({
  planBranchApi: {
    getByProject: (...args: unknown[]) => getPlanBranchesMock(...args),
  },
}));

vi.mock("@/components/Chat/IntegratedChatPanel", () => ({
  IntegratedChatPanel: ({
    headerContent,
    headerSubContent,
    contentWidthClassName,
    renderComposer,
    ideationSessionId,
    conversationIdOverride,
    storeContextKeyOverride,
    agentProcessContextIdOverride,
    sendOptions,
    onChildSessionNavigate,
    emptyState,
  }: {
    headerContent?: ReactNode;
    headerSubContent?: ReactNode;
    contentWidthClassName?: string;
    renderComposer?: (props: Record<string, unknown>) => ReactNode;
    ideationSessionId?: string;
    conversationIdOverride?: string;
    storeContextKeyOverride?: string;
    agentProcessContextIdOverride?: string;
    sendOptions?: Record<string, unknown>;
    onChildSessionNavigate?: (sessionId: string) => void | Promise<void>;
    emptyState?: ReactNode;
  }) => {
    const agentStatus = useChatStore((state) =>
      storeContextKeyOverride
        ? state.agentStatus[storeContextKeyOverride] ?? "idle"
        : "idle"
    );
    const isSending = useChatStore((state) =>
      storeContextKeyOverride
        ? state.isSending[storeContextKeyOverride] ?? false
        : false
    );
    integratedChatPanelRenderMock({
      ideationSessionId,
      conversationIdOverride,
      storeContextKeyOverride,
      agentProcessContextIdOverride,
      sendOptions,
      hasChildSessionNavigate: Boolean(onChildSessionNavigate),
    });
    return (
      <div
        data-testid="integrated-chat-panel"
        data-content-width-class={contentWidthClassName ?? ""}
        data-ideation-session-id={ideationSessionId ?? ""}
        data-conversation-id-override={conversationIdOverride ?? ""}
        data-store-context-key-override={storeContextKeyOverride ?? ""}
        data-agent-process-context-id-override={agentProcessContextIdOverride ?? ""}
        data-send-conversation-id={
          typeof sendOptions?.conversationId === "string" ? sendOptions.conversationId : ""
        }
      >
        {headerContent}
        {headerSubContent}
        {onChildSessionNavigate ? (
          <button
            type="button"
            data-testid="mock-open-child-session"
            onClick={() => void onChildSessionNavigate("session-child")}
          >
            Open child session
          </button>
        ) : null}
        {renderComposer?.({
          onSend: vi.fn(),
          onStop: vi.fn(),
          agentStatus,
          isSending,
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
        {emptyState}
      </div>
    );
  },
}));

vi.mock("./AgentsArtifactPane", () => {
  artifactPaneModuleLoadedMock();
  return {
    AgentsArtifactPane: ({
    conversation,
    activeTab,
    focusedIdeationSessionId,
    publishFocusRequest,
    onClose,
    onFocusVerificationSession,
    onPublishWorkspace,
  }: {
    conversation: AgentConversation | null;
    activeTab?: string;
    focusedIdeationSessionId?: string | null;
    publishFocusRequest?: { filePath: string; mode: string } | null;
    onClose?: () => void;
    onFocusVerificationSession?: (parentSessionId: string, childSessionId: string) => void;
    onPublishWorkspace?: (conversationId: string) => Promise<void>;
  }) => (
    <div
      data-testid="agents-artifact-pane"
      data-active-tab={activeTab ?? ""}
      data-focused-ideation-session-id={focusedIdeationSessionId ?? ""}
      data-publish-focus-path={publishFocusRequest?.filePath ?? ""}
      data-publish-focus-mode={publishFocusRequest?.mode ?? ""}
    >
      {onClose ? (
        <button type="button" data-testid="agents-artifact-pane-close" onClick={onClose}>
          Close
        </button>
      ) : null}
      {onFocusVerificationSession ? (
        <button
          type="button"
          data-testid="mock-focus-verification-session"
          onClick={() =>
            onFocusVerificationSession("session-parent", "verification-child")
          }
        >
          Focus verification
        </button>
      ) : null}
      {conversation && onPublishWorkspace ? (
        <button
          type="button"
          data-testid="agents-publish-confirm"
          onClick={() => void onPublishWorkspace(conversation.id)}
        >
          Publish
        </button>
      ) : null}
    </div>
  ),
  };
});

vi.mock("./useAgentConversationTitleEvents", () => ({
  useAgentConversationTitleEvents: () => undefined,
}));

export function mockSidebarBreakpoint({ isLarge, isMedium }: { isLarge: boolean; isMedium: boolean }) {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    configurable: true,
    value: vi.fn((query: string) => ({
      matches:
        query === "(min-width: 1440px)"
          ? isLarge
          : query === "(min-width: 1280px)"
            ? isMedium
            : false,
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
}

export function mockAgentViewData(agentConversation: AgentConversation = conversation()) {
  useProjectsMock.mockReturnValue({
    data: [project],
    isLoading: false,
  });
  useProjectAgentConversationsMock.mockReturnValue({
    data: [agentConversation],
    conversations: [agentConversation],
    isLoading: false,
    isSuccess: true,
    hasNextPage: false,
    isFetchingNextPage: false,
    fetchNextPage: vi.fn(),
  });
  mockAgentSidebarData([agentConversation]);
  useConversationMock.mockImplementation((conversationId: string | null) => ({
    data:
      conversationId === agentConversation.id
        ? {
            conversation: agentConversation,
            messages: [],
          }
        : null,
    isLoading: false,
  }));
}

export function mockAgentSidebarData(conversations: AgentConversation[]) {
  useAgentSidebarProjectGroupMock.mockImplementation(
    ({
      projectId,
      archivedOnly = false,
      search = "",
      publicationStates = DEFAULT_SIDEBAR_PUBLICATION_STATE_FILTERS,
      pinnedConversationIds = [],
    }: {
      projectId?: string | null;
      archivedOnly?: boolean;
      search?: string;
      publicationStates?: string[];
      pinnedConversationIds?: string[];
    }) =>
      buildAgentSidebarGroupResult({
        key: projectId ?? "",
        label: project.name,
        conversations: filterSidebarConversations(conversations, {
          projectIds: projectId ? [projectId] : [],
          archivedOnly,
          search,
          publicationStates,
          pinnedConversationIds,
        }),
      })
  );
  useAgentSidebarPublicationGroupMock.mockImplementation(
    ({
      projectIds = [],
      publicationState = "active",
      archivedOnly = false,
      search = "",
      pinnedConversationIds = [],
    }: {
      projectIds?: string[];
      publicationState?: string;
      archivedOnly?: boolean;
      search?: string;
      pinnedConversationIds?: string[];
    }) =>
      buildAgentSidebarGroupResult({
        key: publicationState,
        label: publicationState,
        conversations: filterSidebarConversations(conversations, {
          projectIds,
          archivedOnly,
          search,
          publicationStates: [publicationState],
          pinnedConversationIds,
        }),
      })
  );
}

function filterSidebarConversations(
  conversations: AgentConversation[],
  {
    projectIds,
    archivedOnly,
    search,
    publicationStates,
    pinnedConversationIds,
  }: {
    projectIds: string[];
    archivedOnly: boolean;
    search: string;
    publicationStates: string[];
    pinnedConversationIds: string[];
  }
) {
  const projectIdSet = new Set(projectIds);
  const pinnedIdSet = new Set(pinnedConversationIds);
  const normalizedSearch = search.trim().toLowerCase();
  return conversations
    .filter((item) => projectIdSet.has(item.projectId ?? item.contextId))
    .filter((item) => (archivedOnly ? Boolean(item.archivedAt) : !item.archivedAt))
    .filter((item) => {
      if (!normalizedSearch) {
        return true;
      }
      return (item.title ?? "Untitled agent").toLowerCase().includes(normalizedSearch);
    })
    .filter(() => publicationStates.includes("active"))
    .sort((left, right) => {
      const pinnedDelta =
        Number(pinnedIdSet.has(right.id)) - Number(pinnedIdSet.has(left.id));
      if (pinnedDelta !== 0) {
        return pinnedDelta;
      }
      return new Date(right.createdAt).getTime() - new Date(left.createdAt).getTime();
    });
}

function buildAgentSidebarGroupResult({
  key,
  label,
  conversations,
}: {
  key: string;
  label: string;
  conversations: AgentConversation[];
}) {
  const rows = conversations.map((conversation) => ({
    conversation: {
      ...conversation,
      contextType: "project" as const,
      contextId: conversation.projectId ?? conversation.contextId,
    },
    workspace: null,
    refKind: "branch" as const,
    refLabel: "master",
    publicationState: "active" as const,
    publicationLabel: null,
  }));
  const group = {
    key,
    label,
    total: rows.length,
    offset: 0,
    limit: 20,
    hasMore: false,
    rows,
  };
  return {
    data: { pages: [group], pageParams: [0] },
    group,
    isLoading: false,
    isSuccess: true,
    isFetching: false,
    isFetchingNextPage: false,
    hasNextPage: false,
    fetchNextPage: vi.fn(),
  };
}

export function mockSessionWithData(
  overrides?: Partial<NonNullable<Awaited<ReturnType<typeof ideationApi.sessions.getWithData>>>["session"]>,
  proposals: NonNullable<Awaited<ReturnType<typeof ideationApi.sessions.getWithData>>>["proposals"] = []
) {
  const session: NonNullable<
    Awaited<ReturnType<typeof ideationApi.sessions.getWithData>>
  >["session"] = {
    id: "session-1",
    projectId: "project-1",
    title: "Agent Plan",
    titleSource: "auto",
    status: "active" as const,
    planArtifactId: null,
    seedTaskId: null,
    parentSessionId: null,
    teamMode: null,
    teamConfig: null,
    createdAt: "2026-04-23T09:00:00Z",
    updatedAt: "2026-04-23T09:00:00Z",
    archivedAt: null,
    convertedAt: null,
    verificationStatus: "unverified" as const,
    verificationInProgress: false,
    gapScore: null,
    inheritedPlanArtifactId: null,
    sessionPurpose: "general" as const,
    acceptanceStatus: null,
    ...overrides,
  };
  vi.mocked(ideationApi.sessions.get).mockResolvedValue(session);
  vi.mocked(ideationApi.sessions.getWithData).mockResolvedValue({
    session,
    proposals,
    messages: [],
  });
}

export function resetAgentSessionState(
  overrides: Partial<ReturnType<typeof useAgentSessionStore.getState>> = {}
) {
  useAgentSessionStore.setState({
    focusedProjectId: "project-1",
    selectedProjectId: null,
    selectedConversationId: null,
    lastSelectedConversationByProjectId: {},
    expandedProjectIds: { "project-1": true },
    showAllProjects: true,
    projectSort: "latest",
    sidebarGroupBy: "project",
    sidebarProjectFilterIds: [],
    sidebarPublicationStateFilters: [...DEFAULT_SIDEBAR_PUBLICATION_STATE_FILTERS],
    pinnedConversationIds: {},
    artifactByConversationId: {},
    runtimeByConversationId: {},
    lastRuntimeByProjectId: {
      "project-1": runtime,
    },
    branchBaseCacheByProjectId: {},
    lastBranchBaseSelectionByProjectId: {},
    ...overrides,
  });
}

export function renderAgentsView(options: { footer?: ReactNode } = {}) {
  return renderWithProviders(
    <AgentsView
      projectId="project-1"
      onCreateProject={vi.fn()}
      {...(options.footer !== undefined ? { footer: options.footer } : {})}
    />
  );
}

export function selectSidebarConversationRow() {
  const row = screen.getByTestId("agents-session-conversation-1");
  fireEvent.click(within(row).getAllByRole("button")[0] ?? row);
  return row;
}

export function setupAgentsViewTest() {
  eventSubscriptions.clear();
  mockSidebarBreakpoint({ isLarge: true, isMedium: true });
  window.localStorage.clear();
  useProjectAgentConversationsMock.mockReset();
  useAgentSidebarProjectGroupMock.mockReset();
  useAgentSidebarPublicationGroupMock.mockReset();
  useProjectsMock.mockReset();
  useHarnessProvidersMock.mockReset();
  useConversationMock.mockReset();
  startAgentConversationMock.mockReset();
  getAgentConversationWorkspaceMock.mockReset();
  getAgentConversationWorkspaceFreshnessMock.mockReset();
  listAgentConversationWorkspacesByProjectMock.mockReset();
  listWorkspaceOpenTargetsMock.mockReset();
  openAgentConversationWorkspacePathMock.mockReset();
  listConversationsMock.mockReset();
  publishAgentConversationWorkspaceMock.mockReset();
  setAgentConversationWorkspaceAutoPublishMock.mockReset();
  setAgentConversationWorkspacePrSupervisionMock.mockReset();
  switchAgentConversationModeMock.mockReset();
  sendAgentMessageMock.mockReset();
  createConversationMock.mockReset();
  spawnConversationSessionNamerMock.mockReset();
  updateConversationTitleMock.mockReset();
  archiveConversationMock.mockReset();
  restoreConversationMock.mockReset();
  getAgentRunningStatesMock.mockReset();
  getPlanBranchesMock.mockReset();
  listIdeationSessionsMock.mockReset();
  getWorkspaceChangesMock.mockReset();
  getWorkspaceChangeSummaryMock.mockReset();
  getWorkspaceReviewMock.mockReset();
  getWorkspaceDiffMock.mockReset();
  getWorkspaceCommitsMock.mockReset();
  getWorkspaceCommitChangesMock.mockReset();
  getWorkspaceCommitDiffMock.mockReset();
  getWorkspaceStagedChangesMock.mockReset();
  getWorkspaceUnstagedChangesMock.mockReset();
  getWorkspaceCumulativeChangesMock.mockReset();
  getWorkspaceStagedDiffMock.mockReset();
  getWorkspaceUnstagedDiffMock.mockReset();
  getWorkspaceCumulativeDiffMock.mockReset();
  listAgentTasksMock.mockReset();
  listAgentTaskListsMock.mockReset();
  listAgentTaskListTasksMock.mockReset();
  precomputePrDescriptionMock.mockReset();
  toastErrorMock.mockReset();
  toastSuccessMock.mockReset();
  integratedChatPanelRenderMock.mockReset();
  preloadAgentsArtifactPaneMock.mockReset();
  artifactPaneModuleLoadedMock.mockReset();
  preloadAgentTerminalExperienceMock.mockReset();
  terminalDrawerModuleLoadedMock.mockReset();
  terminalDrawerMountMock.mockReset();
  terminalDrawerUnmountMock.mockReset();
  webviewDragDropHandlers.splice(0);
  webviewOnDragDropEventMock.mockReset();
  webviewDragDropUnlistenMock.mockReset();

  sendAgentMessageMock.mockResolvedValue({
    conversationId: "conversation-2",
    agentRunId: "run-2",
    isNewConversation: true,
    wasQueued: false,
    queuedAsPending: false,
    queuedMessageId: null,
  });
  getAgentConversationWorkspaceMock.mockResolvedValue(null);
  getAgentConversationWorkspaceFreshnessMock.mockResolvedValue({
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
  });
  listAgentConversationWorkspacesByProjectMock.mockResolvedValue([]);
  listWorkspaceOpenTargetsMock.mockResolvedValue([]);
  openAgentConversationWorkspacePathMock.mockResolvedValue(undefined);
  listConversationsMock.mockResolvedValue([]);
  getPlanBranchesMock.mockResolvedValue([]);
  listIdeationSessionsMock.mockResolvedValue([]);
  mockAgentSidebarData([]);
  getWorkspaceChangesMock.mockResolvedValue([]);
  getWorkspaceChangeSummaryMock.mockResolvedValue({
    supportsWorktreeModes: true,
    staged: { fileCount: 0, additions: 0, deletions: 0 },
    unstaged: { fileCount: 0, additions: 0, deletions: 0 },
  });
  getWorkspaceReviewMock.mockResolvedValue({
    changes: [],
    commits: [],
    baseRef: "main",
    headRef: "HEAD",
  });
  getWorkspaceDiffMock.mockResolvedValue("");
  getWorkspaceCommitsMock.mockResolvedValue([]);
  getWorkspaceCommitChangesMock.mockResolvedValue([]);
  getWorkspaceCommitDiffMock.mockResolvedValue("");
  getWorkspaceStagedChangesMock.mockResolvedValue([]);
  getWorkspaceUnstagedChangesMock.mockResolvedValue([]);
  getWorkspaceCumulativeChangesMock.mockResolvedValue([]);
  getWorkspaceStagedDiffMock.mockResolvedValue("");
  getWorkspaceUnstagedDiffMock.mockResolvedValue("");
  getWorkspaceCumulativeDiffMock.mockResolvedValue("");
  listAgentTasksMock.mockResolvedValue([]);
  listAgentTaskListsMock.mockResolvedValue([]);
  listAgentTaskListTasksMock.mockResolvedValue([]);
  precomputePrDescriptionMock.mockResolvedValue({
    conversationId: "conversation-1",
    status: "skipped",
    cacheStatus: null,
    reason: "no_reviewable_commits",
  });
  publishAgentConversationWorkspaceMock.mockResolvedValue({
    workspace: {
      conversationId: "conversation-2",
      projectId: "project-1",
      mode: "edit",
      baseRefKind: "project_default",
      baseRef: "main",
      baseDisplayName: "Project default (main)",
      baseCommit: null,
      branchName: "ralphx/demo/agent-conversation-2",
      worktreePath: "/tmp/ralphx/conversation-2",
      linkedIdeationSessionId: null,
      linkedPlanBranchId: null,
      publicationPrNumber: 42,
      publicationPrUrl: "https://github.com/mock/project/pull/42",
      publicationPrStatus: "draft",
      publicationPushStatus: "pushed",
      autoPublishEnabled: true,
      autoPublishPausedPrAutofixEnabled: null,
      autoPublishPausedPrAutoMergeDesired: null,
      status: "active",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    },
    commitSha: "mockcommit",
    pushed: true,
    createdPr: true,
    prNumber: 42,
    prUrl: "https://github.com/mock/project/pull/42",
  });
  setAgentConversationWorkspacePrSupervisionMock.mockImplementation(
    async (conversationId: string, input: { autoFixEnabled: boolean; autoMergeDesired: boolean }) => ({
      conversationId,
      projectId: "project-1",
      mode: "edit",
      baseRefKind: "project_default",
      baseRef: "main",
      baseDisplayName: "Project default (main)",
      baseCommit: null,
      branchName: `ralphx/demo/agent-${conversationId}`,
      worktreePath: `/tmp/ralphx/${conversationId}`,
      linkedIdeationSessionId: null,
      linkedPlanBranchId: null,
      publicationPrNumber: 42,
      publicationPrUrl: "https://github.com/mock/project/pull/42",
      publicationPrStatus: "open",
      publicationPushStatus: "pushed",
      autoPublishEnabled: true,
      autoPublishPausedPrAutofixEnabled: null,
      autoPublishPausedPrAutoMergeDesired: null,
      prAutofixEnabled: input.autoFixEnabled,
      prAutoMergeDesired: input.autoMergeDesired,
      prAutoMergeMethod: "squash",
      prSupervisionStatus:
        input.autoFixEnabled || input.autoMergeDesired ? "monitoring" : "disabled",
      status: "active",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }),
  );
  setAgentConversationWorkspaceAutoPublishMock.mockImplementation(
    async (conversationId: string, input: { autoPublishEnabled: boolean }) => ({
      conversationId,
      projectId: "project-1",
      mode: "edit",
      baseRefKind: "project_default",
      baseRef: "main",
      baseDisplayName: "Project default (main)",
      baseCommit: null,
      branchName: `ralphx/demo/agent-${conversationId}`,
      worktreePath: `/tmp/ralphx/${conversationId}`,
      linkedIdeationSessionId: null,
      linkedPlanBranchId: null,
      publicationPrNumber: 42,
      publicationPrUrl: "https://github.com/mock/project/pull/42",
      publicationPrStatus: "open",
      publicationPushStatus: "pushed",
      autoPublishEnabled: input.autoPublishEnabled,
      autoPublishPausedPrAutofixEnabled: input.autoPublishEnabled ? null : true,
      autoPublishPausedPrAutoMergeDesired: input.autoPublishEnabled ? null : false,
      prAutofixEnabled: input.autoPublishEnabled,
      prAutoMergeDesired: false,
      prAutoMergeMethod: "squash",
      prSupervisionStatus: input.autoPublishEnabled ? "monitoring" : "paused",
      status: "active",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    }),
  );
  switchAgentConversationModeMock.mockResolvedValue({
    conversation: conversation({
      id: "conversation-1",
      contextId: "project-1",
      agentMode: "edit",
    }),
    workspace: {
      conversationId: "conversation-1",
      projectId: "project-1",
      mode: "edit",
      baseRefKind: "project_default",
      baseRef: "main",
      baseDisplayName: "Project default (main)",
      baseCommit: null,
      branchName: "ralphx/demo/agent-conversation-1",
      worktreePath: "/tmp/ralphx/conversation-1",
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
  });
  startAgentConversationMock.mockResolvedValue({
    conversation: conversation({ id: "conversation-2", contextId: "project-1" }),
    workspace: {
      conversationId: "conversation-2",
      projectId: "project-1",
      mode: "edit",
      baseRefKind: "project_default",
      baseRef: "main",
      baseDisplayName: "Project default (main)",
      baseCommit: null,
      branchName: "ralphx/demo/agent-conversation-2",
      worktreePath: "/tmp/ralphx/conversation-2",
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
      conversationId: "conversation-2",
      agentRunId: "run-2",
      isNewConversation: true,
      wasQueued: false,
      queuedAsPending: false,
      queuedMessageId: null,
    },
  });
  createConversationMock.mockResolvedValue(
    conversation({ id: "conversation-2", contextId: "project-1" })
  );
  spawnConversationSessionNamerMock.mockResolvedValue(undefined);
  updateConversationTitleMock.mockResolvedValue({
    ...conversation(),
    id: "conversation-2",
    title: "Fix agent landing flow",
  });
  vi.mocked(ideationApi.sessions.get).mockReset();
  vi.mocked(ideationApi.sessions.getWithData).mockReset();
  mockSessionWithData();
  getLatestChildSessionIdMock.mockReset();
  getLatestChildSessionIdMock.mockResolvedValue({
    sessionId: "session-1",
    purpose: "verification",
    latestChildSessionId: null,
  });
  mockHarnessProviders();
  archiveConversationMock.mockResolvedValue(undefined);
  restoreConversationMock.mockResolvedValue(undefined);
  getAgentRunningStatesMock.mockResolvedValue({});
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue(undefined);

  useChatStore.setState({
    messages: {},
    activeConversationIds: {},
    queuedMessages: {},
    agentStatus: {},
    agentActivityLabels: {},
    isSending: {},
  });
  useUiStore.getState().closeModal();
  useUiStore.getState().setExecutionPaused(false);
  useUiStore.getState().setExecutionQueuedCount(0, 0);
  resetAgentSessionState();
  useAgentArtifactUiStore.setState({
    artifactByConversationId: {},
  });
  useAgentTerminalStore.setState({
    openByConversationId: {},
    heightByConversationId: {},
    activeTerminalByConversationId: {},
    statusByConversationId: {},
    placement: "auto",
    draggingConversationId: null,
    dragOverDock: null,
  });
}
