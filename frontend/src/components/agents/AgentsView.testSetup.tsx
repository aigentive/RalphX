import { fireEvent, screen, within } from "@testing-library/react";
import { useEffect, useState, type ComponentProps, type ReactNode } from "react";
import { vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import type { AgentProvidersSettingsResponse } from "@/api/harness-providers";
import type {
  AgentConversationWorkspace,
  AgentWorkspaceReviewContext,
  ComposerIntegrationReference,
  SendAgentMessageResult,
} from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { defaultIdeationSettings } from "@/types/ideation-config";
import {
  DEFAULT_SIDEBAR_PUBLICATION_STATE_FILTERS,
  useAgentSessionStore,
} from "@/stores/agentSessionStore";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import { useAgentArtifactUiStore } from "./agentArtifactUiStore";
import { resetAgentWorkspaceOperationDismissalsForTests } from "./agentWorkspaceOperationDismissals";
import { resetAgentWorkspaceOperationRegistryForTests } from "./agentWorkspaceOperationRegistry";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import type { AgentPublishSubTabRequest } from "./agentPublishSubTab";
import { useAgentTerminalStore } from "./agentTerminalStore";
import { useProjectStore } from "@/stores/projectStore";
import type { AgentConversation } from "./agentConversations";
import { AgentsView } from "./AgentsView";
import {
  agentProjectFixture as project,
  agentRuntimeFixture as runtime,
  conversationFixture as conversation,
  conversationWorkspaceFixture as conversationWorkspace,
  renderWithAgentProviders as renderWithProviders,
} from "./agentsTestFixtures";


const agentsViewTestMocks = vi.hoisted(() => ({
  useProjectsMock: vi.fn(),
  useHarnessProvidersMock: vi.fn(),
  useProjectAgentConversationsMock: vi.fn(),
  useAgentSidebarProjectGroupMock: vi.fn(),
  useAgentSidebarPublicationGroupMock: vi.fn(),
  useAgentSidebarAutomationGroupIndexMock: vi.fn(),
  useAgentSidebarAutomationGroupMock: vi.fn(),
  useConversationMock: vi.fn(),
  useConversationSummaryMock: vi.fn(),
  startAgentConversationMock: vi.fn(),
  createAutomationDraftMock: vi.fn(),
  finalizeAutomationMock: vi.fn(),
  triggerAutomationRunNowMock: vi.fn(),
  updateAutomationSetupMock: vi.fn(),
  getPullRequestDetailMock: vi.fn(),
  getAgentConversationWorkspaceMock: vi.fn(),
  getAgentConversationWorkspaceFreshnessMock: vi.fn(),
  listAgentConversationWorkspacesByProjectMock: vi.fn(),
  listWorkspaceOpenTargetsMock: vi.fn(),
  openAgentConversationWorkspacePathMock: vi.fn(),
  listConversationsMock: vi.fn(),
  listAgentConversationWorkspacePublicationEventsMock: vi.fn(),
  publishAgentConversationWorkspaceMock: vi.fn(),
  recheckAgentConversationWorkspacePrHealthMock: vi.fn(),
  retryAgentConversationWorkspacePrAutofixOverrideMock: vi.fn(),
  stopAgentConversationWorkspacePrAutofixForFailureMock: vi.fn(),
  commitAgentConversationWorkspaceLocallyMock: vi.fn(),
  updateWorkspaceFromBaseMock: vi.fn(),
  setAgentConversationWorkspaceAutoPublishMock: vi.fn(),
  setAgentConversationWorkspacePrSupervisionMock: vi.fn(),
  precomputePrDescriptionMock: vi.fn(),
  switchAgentConversationModeMock: vi.fn(),
  sendAgentMessageMock: vi.fn(),
  listAgentConversationIssuesMock: vi.fn(),
  createConversationMock: vi.fn(),
  spawnConversationSessionNamerMock: vi.fn(),
  updateConversationTitleMock: vi.fn(),
  archiveConversationMock: vi.fn(),
  restoreConversationMock: vi.fn(),
  getAgentRunningStatesMock: vi.fn(),
  getAgentConversationRuntimeIndexMock: vi.fn(),
  getAgentConversationRuntimeStatusesMock: vi.fn(),
  getPlanBranchesMock: vi.fn(),
  getTicketAssociationsMock: vi.fn(),
  startWorkFromTicketMock: vi.fn(),
  loadBranchBaseOptionsMock: vi.fn(),
  loadPullRequestBaseOptionsMock: vi.fn(),
  listIdeationSessionsMock: vi.fn(),
  getLatestChildSessionIdMock: vi.fn(),
  getWorkspaceChangesMock: vi.fn(),
  getWorkspaceChangeSummaryMock: vi.fn(),
  getWorkspaceReviewMock: vi.fn(),
  getWorkspacePrAnnotationsMock: vi.fn(),
  getWorkspaceReviewHunkAnnotationsMock: vi.fn(),
  getWorkspaceReviewContextMock: vi.fn(),
  startWorkspaceReviewMock: vi.fn(),
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
  getWorkspaceRepairSummaryMock: vi.fn(),
  getWorkspaceRepairStagedChangesMock: vi.fn(),
  getWorkspaceRepairUnstagedChangesMock: vi.fn(),
  getWorkspaceRepairConflictDiffMock: vi.fn(),
  getWorkspaceRepairStagedDiffMock: vi.fn(),
  getWorkspaceRepairUnstagedDiffMock: vi.fn(),
  listAgentTasksMock: vi.fn(),
  listAgentTaskListsMock: vi.fn(),
  listAgentTaskListTasksMock: vi.fn(),
  toastDismissMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastInfoMock: vi.fn(),
  toastLoadingMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  integratedChatPanelRenderMock: vi.fn(),
  integratedChatPanelUserMessageSentMock: vi.fn(),
  preloadAgentsArtifactPaneMock: vi.fn(),
  preloadAgentTerminalExperienceMock: vi.fn(),
  artifactPaneModuleLoadedMock: vi.fn(),
  artifactPaneRenderMock: vi.fn(),
  realPublishPanelState: {
    enabled: false,
    reviewContext: null as unknown,
  },
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
      ultraSupportedModels: ["gpt-5.6-sol", "gpt-5.6-terra"],
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
      ultraSupportedModels: [],
      supportsFastMode: false,
      fastModeSupportedModels: [],
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
  useAgentSidebarAutomationGroupIndexMock,
  useAgentSidebarAutomationGroupMock,
  useConversationMock,
  useConversationSummaryMock,
  startAgentConversationMock,
  createAutomationDraftMock,
  finalizeAutomationMock,
  triggerAutomationRunNowMock,
  updateAutomationSetupMock,
  getPullRequestDetailMock,
  getAgentConversationWorkspaceMock,
  getAgentConversationWorkspaceFreshnessMock,
  listAgentConversationWorkspacesByProjectMock,
  listWorkspaceOpenTargetsMock,
  openAgentConversationWorkspacePathMock,
  listConversationsMock,
  listAgentConversationWorkspacePublicationEventsMock,
  publishAgentConversationWorkspaceMock,
  recheckAgentConversationWorkspacePrHealthMock,
  retryAgentConversationWorkspacePrAutofixOverrideMock,
  stopAgentConversationWorkspacePrAutofixForFailureMock,
  commitAgentConversationWorkspaceLocallyMock,
  updateWorkspaceFromBaseMock,
  setAgentConversationWorkspaceAutoPublishMock,
  setAgentConversationWorkspacePrSupervisionMock,
  precomputePrDescriptionMock,
  switchAgentConversationModeMock,
  sendAgentMessageMock,
  listAgentConversationIssuesMock,
  createConversationMock,
  spawnConversationSessionNamerMock,
  updateConversationTitleMock,
  archiveConversationMock,
  restoreConversationMock,
  getAgentRunningStatesMock,
  getAgentConversationRuntimeIndexMock,
  getAgentConversationRuntimeStatusesMock,
  getPlanBranchesMock,
  getTicketAssociationsMock,
  startWorkFromTicketMock,
  loadBranchBaseOptionsMock,
  loadPullRequestBaseOptionsMock,
  listIdeationSessionsMock,
  getLatestChildSessionIdMock,
  getWorkspaceChangesMock,
  getWorkspaceChangeSummaryMock,
  getWorkspaceReviewMock,
  getWorkspacePrAnnotationsMock,
  getWorkspaceReviewHunkAnnotationsMock,
  getWorkspaceReviewContextMock,
  startWorkspaceReviewMock,
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
  getWorkspaceRepairSummaryMock,
  getWorkspaceRepairStagedChangesMock,
  getWorkspaceRepairUnstagedChangesMock,
  getWorkspaceRepairConflictDiffMock,
  getWorkspaceRepairStagedDiffMock,
  getWorkspaceRepairUnstagedDiffMock,
  listAgentTasksMock,
  listAgentTaskListsMock,
  listAgentTaskListTasksMock,
  toastDismissMock,
  toastErrorMock,
  toastInfoMock,
  toastLoadingMock,
  toastSuccessMock,
  integratedChatPanelRenderMock,
  integratedChatPanelUserMessageSentMock,
  preloadAgentsArtifactPaneMock,
  preloadAgentTerminalExperienceMock,
  artifactPaneModuleLoadedMock,
  artifactPaneRenderMock,
  realPublishPanelState,
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

vi.mock("@/api/github", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/github")>();
  return {
    ...actual,
    githubApi: {
      ...actual.githubApi,
      getPullRequestDetail: getPullRequestDetailMock,
    },
  };
});

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: (options?: unknown) => useHarnessProvidersMock(options),
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
        {
          id: "haiku",
          label: "haiku",
          menuLabel: "haiku",
          defaultEffort: "medium",
          supportedEfforts: ["low", "medium", "high"],
        },
        {
          id: "fable",
          label: "fable",
          menuLabel: "fable",
          defaultEffort: "high",
          supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
        },
      ],
      codex: [
        {
          id: "gpt-5.6-terra",
          label: "gpt-5.6-terra",
          menuLabel: "gpt-5.6-terra",
          defaultEffort: "medium",
          supportedEfforts: ["low", "medium", "high", "xhigh", "max", "ultra"],
        },
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
    automationIndex: (
      projectIds: string[],
      archivedOnly: boolean,
      search = "",
      publicationStates: string[] = [],
      pinnedConversationIds: string[] = [],
      priorityConversationIds: string[] = [],
      sort = "latest"
    ) => [
      "agents",
      "sidebar-conversations",
      "automation",
      "index",
      "projects",
      projectIds,
      "archived",
      archivedOnly,
      "search",
      search.trim().toLowerCase(),
      "states",
      publicationStates,
      "pinned",
      pinnedConversationIds,
      "priority",
      priorityConversationIds,
      "sort",
      sort,
    ],
    automationGroup: (
      groupKey: string,
      projectIds: string[],
      archivedOnly: boolean,
      search = "",
      publicationStates: string[] = [],
      pinnedConversationIds: string[] = [],
      priorityConversationIds: string[] = [],
      sort = "latest"
    ) => [
      "agents",
      "sidebar-conversations",
      "automation",
      groupKey,
      "projects",
      projectIds,
      "archived",
      archivedOnly,
      "search",
      search.trim().toLowerCase(),
      "states",
      publicationStates,
      "pinned",
      pinnedConversationIds,
      "priority",
      priorityConversationIds,
      "sort",
      sort,
    ],
  },
  useAgentSidebarProjectGroup: (args: Record<string, unknown>) =>
    useAgentSidebarProjectGroupMock(args),
  useAgentSidebarPublicationGroup: (args: Record<string, unknown>) =>
    useAgentSidebarPublicationGroupMock(args),
  useAgentSidebarAutomationGroupIndex: (args: Record<string, unknown>) =>
    useAgentSidebarAutomationGroupIndexMock(args),
  useAgentSidebarAutomationGroup: (args: Record<string, unknown>) =>
    useAgentSidebarAutomationGroupMock(args),
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
  useConversationSummary: (conversationId: string | null) =>
    useConversationSummaryMock(conversationId),
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
    getAgentWorkspaceReviewContext: (...args: unknown[]) =>
      getWorkspaceReviewContextMock(...args),
    startAgentWorkspaceReview: (...args: unknown[]) =>
      startWorkspaceReviewMock(...args),
    listAgentConversationWorkspacesByProject: (...args: unknown[]) =>
      listAgentConversationWorkspacesByProjectMock(...args),
    listConversations: (...args: unknown[]) => listConversationsMock(...args),
    listAgentConversationWorkspacePublicationEvents: (...args: unknown[]) =>
      listAgentConversationWorkspacePublicationEventsMock(...args),
    publishAgentConversationWorkspace: (...args: unknown[]) =>
      publishAgentConversationWorkspaceMock(...args),
    recheckAgentConversationWorkspacePrHealth: (...args: unknown[]) =>
      recheckAgentConversationWorkspacePrHealthMock(...args),
    retryAgentConversationWorkspacePrAutofixOverride: (...args: unknown[]) =>
      retryAgentConversationWorkspacePrAutofixOverrideMock(...args),
    stopAgentConversationWorkspacePrAutofixForFailure: (...args: unknown[]) =>
      stopAgentConversationWorkspacePrAutofixForFailureMock(...args),
    commitAgentConversationWorkspaceLocally: (...args: unknown[]) =>
      commitAgentConversationWorkspaceLocallyMock(...args),
    updateAgentConversationWorkspaceFromBase: (...args: unknown[]) =>
      updateWorkspaceFromBaseMock(...args),
    setAgentConversationWorkspaceAutoPublish: (...args: unknown[]) =>
      setAgentConversationWorkspaceAutoPublishMock(...args),
    setAgentConversationWorkspacePrSupervision: (...args: unknown[]) =>
      setAgentConversationWorkspacePrSupervisionMock(...args),
    precomputeAgentConversationWorkspacePrDescription: (...args: unknown[]) =>
      precomputePrDescriptionMock(...args),
    switchAgentConversationMode: (...args: unknown[]) =>
      switchAgentConversationModeMock(...args),
    sendAgentMessage: (...args: unknown[]) => sendAgentMessageMock(...args),
    listAgentConversationIssues: (...args: unknown[]) =>
      listAgentConversationIssuesMock(...args),
    createConversation: (...args: unknown[]) => createConversationMock(...args),
    spawnConversationSessionNamer: (...args: unknown[]) =>
      spawnConversationSessionNamerMock(...args),
    updateConversationTitle: (...args: unknown[]) => updateConversationTitleMock(...args),
    archiveConversation: (...args: unknown[]) => archiveConversationMock(...args),
    restoreConversation: (...args: unknown[]) => restoreConversationMock(...args),
    getAgentRunningStates: (...args: unknown[]) => getAgentRunningStatesMock(...args),
    getAgentConversationRuntimeIndex: (...args: unknown[]) =>
      getAgentConversationRuntimeIndexMock(...args),
    getAgentConversationRuntimeStatuses: (...args: unknown[]) =>
      getAgentConversationRuntimeStatusesMock(...args),
    listWorkspaceOpenTargets: (...args: unknown[]) =>
      listWorkspaceOpenTargetsMock(...args),
    openAgentConversationWorkspacePath: (...args: unknown[]) =>
      openAgentConversationWorkspacePathMock(...args),
    getBulkWorkspacePublicationStates: vi.fn().mockResolvedValue({}),
  },
}));

vi.mock("@/api/automations", async () => {
  const actual = await vi.importActual<typeof import("@/api/automations")>(
    "@/api/automations",
  );
  return {
    ...actual,
    automationsApi: {
      ...actual.automationsApi,
      createDraft: (...args: unknown[]) => createAutomationDraftMock(...args),
      finalize: (...args: unknown[]) => finalizeAutomationMock(...args),
      triggerRunNow: (...args: unknown[]) => triggerAutomationRunNowMock(...args),
      setupAgent: {
        ...actual.automationsApi.setupAgent,
        updateAutomation: (...args: unknown[]) =>
          updateAutomationSetupMock(...args),
      },
    },
  };
});

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
    settings: {
      get: vi.fn(),
      update: vi.fn(),
      getDisableImpact: vi.fn(),
      setTasksEnabled: vi.fn(),
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
    getAgentConversationWorkspacePrAnnotations: (...args: unknown[]) =>
      getWorkspacePrAnnotationsMock(...args),
    getAgentConversationWorkspaceReviewHunkAnnotations: (...args: unknown[]) =>
      getWorkspaceReviewHunkAnnotationsMock(...args),
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
    getAgentConversationWorkspaceRepairChangeSummary: (...args: unknown[]) =>
      getWorkspaceRepairSummaryMock(...args),
    getAgentConversationWorkspaceRepairStagedFileChanges: (...args: unknown[]) =>
      getWorkspaceRepairStagedChangesMock(...args),
    getAgentConversationWorkspaceRepairUnstagedFileChanges: (...args: unknown[]) =>
      getWorkspaceRepairUnstagedChangesMock(...args),
    getAgentConversationWorkspaceRepairConflictFileDiff: (...args: unknown[]) =>
      getWorkspaceRepairConflictDiffMock(...args),
    getAgentConversationWorkspaceCumulativeFileChanges: (...args: unknown[]) =>
      getWorkspaceCumulativeChangesMock(...args),
    getAgentConversationWorkspaceStagedFileDiff: (...args: unknown[]) =>
      getWorkspaceStagedDiffMock(...args),
    getAgentConversationWorkspaceUnstagedFileDiff: (...args: unknown[]) =>
      getWorkspaceUnstagedDiffMock(...args),
    getAgentConversationWorkspaceRepairStagedFileDiff: (...args: unknown[]) =>
      getWorkspaceRepairStagedDiffMock(...args),
    getAgentConversationWorkspaceRepairUnstagedFileDiff: (...args: unknown[]) =>
      getWorkspaceRepairUnstagedDiffMock(...args),
    getAgentConversationWorkspaceCumulativeFileDiff: (...args: unknown[]) =>
      getWorkspaceCumulativeDiffMock(...args),
  },
}));

vi.mock("@/api/agent-tasks", () => ({
  agentTaskApi: {
    listAgentTasks: (...args: unknown[]) => listAgentTasksMock(...args),
    listAgentTaskLists: (...args: unknown[]) =>
      listAgentTaskListsMock(...args),
    listAgentTasksForList: (...args: unknown[]) =>
      listAgentTaskListTasksMock(...args),
    listConversationTasks: (...args: unknown[]) => listAgentTasksMock(...args),
    listConversationTaskLists: (...args: unknown[]) =>
      listAgentTaskListsMock(...args),
    listConversationTaskListTasks: (...args: unknown[]) =>
      listAgentTaskListTasksMock(...args),
  },
}));

vi.mock("sonner", () => ({
  toast: {
    dismiss: (...args: unknown[]) => toastDismissMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
    loading: (...args: unknown[]) => toastLoadingMock(...args),
    success: (...args: unknown[]) => toastSuccessMock(...args),
  },
}));

vi.mock("@/components/shared/branchBaseOptions", async () => {
  const actual = await vi.importActual<
    typeof import("@/components/shared/branchBaseOptions")
  >("@/components/shared/branchBaseOptions");
  return {
    ...actual,
    loadBranchBaseOptions: (...args: unknown[]) => loadBranchBaseOptionsMock(...args),
    loadPullRequestBaseOptions: (...args: unknown[]) =>
      loadPullRequestBaseOptionsMock(...args),
  };
});

vi.mock("@/api/plan-branch", () => ({
  planBranchApi: {
    getByProject: (...args: unknown[]) => getPlanBranchesMock(...args),
  },
}));

vi.mock("@/api/ticketing", async () => {
  const actual = await vi.importActual<typeof import("@/api/ticketing")>(
    "@/api/ticketing"
  );
  return {
    ...actual,
    ticketingApi: {
      ...actual.ticketingApi,
      getTicketAssociations: (...args: unknown[]) =>
        getTicketAssociationsMock(...args),
      startWorkFromTicket: (input: Record<string, unknown>) => {
        startWorkFromTicketMock(input);
        const { ticketRef: _ticketRef, ...startInput } = input;
        return startAgentConversationMock(startInput);
      },
    },
  };
});

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
    onUserMessageSent,
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
    onUserMessageSent?: (payload: {
      content: string;
      result: SendAgentMessageResult;
      composerIntegrationReferences?: ComposerIntegrationReference[];
    }) => void | Promise<void>;
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
    integratedChatPanelUserMessageSentMock(onUserMessageSent);
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

vi.mock("./AgentsArtifactPane", async () => {
  const { AgentPublishPanel } = await vi.importActual<
    typeof import("./AgentsPublishPanel")
  >("./AgentsPublishPanel");
  function ControlledPublishPanel(
    props: ComponentProps<typeof AgentPublishPanel>,
  ) {
    const [activeSubTab, setActiveSubTab] = useState(props.activeSubTab);
    useEffect(() => {
      setActiveSubTab(props.activeSubTab);
    }, [props.activeSubTab]);
    return (
      <AgentPublishPanel
        {...props}
        activeSubTab={activeSubTab}
        onSubTabChange={setActiveSubTab}
      />
    );
  }
  artifactPaneModuleLoadedMock();
  return {
    AgentsArtifactPane: ({
      conversation,
      activeTab,
      focusedIdeationSession,
      publishFocusRequest,
      publishSubTabRequest,
      onClose,
      onFocusIdeationSessionForConversation,
      onFocusVerificationSession,
      onFocusWorkspaceReview,
      onOpenAutomation,
      onPublishWorkspace,
      workspace,
      projectBaseBranch,
      publishAttempt,
    }: {
      conversation: AgentConversation | null;
      workspace?: AgentConversationWorkspace | null;
      activeTab?: string;
      focusedIdeationSession?: {
        conversationId: string;
        sessionId: string;
      } | null;
      publishFocusRequest?: AgentPublishFocusRequest | null;
      publishSubTabRequest?: AgentPublishSubTabRequest | null;
      projectBaseBranch?: string | null;
      isPublishingWorkspace?: boolean;
      publishAttempt?: { conversationId: string; startedAtMs: number } | null;
      onClose?: () => void;
      onFocusIdeationSessionForConversation?: (
        conversationId: string,
        sessionId: string
      ) => void;
      onFocusVerificationSession?: (
        parentSessionId: string,
        childSessionId: string
      ) => void;
      onFocusWorkspaceReview?: (
        conversationId: string,
        runtimeHint?: {
          provider: "codex";
          modelId: "gpt-5.6-terra";
          effort: "medium";
        },
      ) => void;
      onOpenAutomation?: (automationId: string) => void;
      onPublishWorkspace?: (conversationId: string) => Promise<void>;
    }) => {
      artifactPaneRenderMock({
        conversationId: conversation?.id ?? null,
        focusedIdeationSessionId: focusedIdeationSession?.sessionId ?? null,
      });
      return realPublishPanelState.enabled && activeTab === "publish" ? (
      <div
        data-testid="agents-artifact-pane"
        data-active-tab={activeTab ?? ""}
        data-focused-ideation-session-id={focusedIdeationSession?.sessionId ?? ""}
        data-publish-focus-path={publishFocusRequest?.filePath ?? ""}
        data-publish-focus-mode={publishFocusRequest?.mode ?? ""}
        data-publish-sub-tab={
          publishSubTabRequest?.conversationId === conversation?.id
            ? (publishSubTabRequest?.tab ?? "changes")
            : "changes"
        }
        data-automation-id={conversation?.automationId ?? ""}
      >
        {onClose ? (
          <button
            type="button"
            data-testid="agents-artifact-pane-close"
            onClick={onClose}
          >
            Close
          </button>
        ) : null}
        <ControlledPublishPanel
          workspace={workspace ?? null}
          conversationTitle={conversation?.title ?? null}
          projectBaseBranch={projectBaseBranch ?? null}
          onPublishWorkspace={onPublishWorkspace}
          publishAttempt={publishAttempt ?? null}
          publishFocusRequest={publishFocusRequest ?? null}
          reviewContext={
            realPublishPanelState.reviewContext as AgentWorkspaceReviewContext | null
          }
          activeSubTab={
            publishSubTabRequest?.conversationId === conversation?.id
              ? (publishSubTabRequest?.tab ?? "changes")
              : "changes"
          }
          showReviewTab
          onSubTabChange={() => {}}
          reviewContent={() => null}
        />
      </div>
    ) : (
      <div
        data-testid="agents-artifact-pane"
        data-active-tab={activeTab ?? ""}
        data-focused-ideation-session-id={focusedIdeationSession?.sessionId ?? ""}
        data-publish-focus-path={publishFocusRequest?.filePath ?? ""}
        data-publish-focus-mode={publishFocusRequest?.mode ?? ""}
        data-publish-sub-tab={
          publishSubTabRequest?.conversationId === conversation?.id
            ? (publishSubTabRequest?.tab ?? "changes")
            : "changes"
        }
        data-automation-id={conversation?.automationId ?? ""}
      >
        {onClose ? (
          <button
            type="button"
            data-testid="agents-artifact-pane-close"
            onClick={onClose}
          >
            Close
          </button>
        ) : null}
        {onFocusIdeationSessionForConversation ? (
          <button
            type="button"
            data-testid="mock-focus-stale-proposals-session"
            onClick={() =>
              onFocusIdeationSessionForConversation("conversation-1", "session-1")
            }
          >
            Focus stale proposal session
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
        {onFocusWorkspaceReview ? (
          <button
            type="button"
            data-testid="mock-focus-workspace-review-with-stale-runtime"
            onClick={() =>
              onFocusWorkspaceReview("review-conversation-1", {
                provider: "codex",
                modelId: "gpt-5.6-terra",
                effort: "medium",
              })
            }
          >
            Focus workspace review with stale runtime
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
        {conversation?.automationId && onOpenAutomation ? (
          <button
            type="button"
            data-testid="mock-open-automation"
            onClick={() => onOpenAutomation(conversation.automationId ?? "")}
          >
            Open automation
          </button>
        ) : null}
      </div>
      );
    },
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
  useProjectStore.getState().setProjects([project]);
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
      priorityConversationIds = [],
    }: {
      projectId?: string | null;
      archivedOnly?: boolean;
      search?: string;
      publicationStates?: string[];
      pinnedConversationIds?: string[];
      priorityConversationIds?: string[];
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
          priorityConversationIds,
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
      priorityConversationIds = [],
    }: {
      projectIds?: string[];
      publicationState?: string;
      archivedOnly?: boolean;
      search?: string;
      pinnedConversationIds?: string[];
      priorityConversationIds?: string[];
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
          priorityConversationIds,
        }),
      })
  );
  useAgentSidebarAutomationGroupIndexMock.mockImplementation(
    ({
      projectIds = [],
      archivedOnly = false,
      search = "",
      publicationStates = DEFAULT_SIDEBAR_PUBLICATION_STATE_FILTERS,
      pinnedConversationIds = [],
      priorityConversationIds = [],
    }: {
      projectIds?: string[];
      archivedOnly?: boolean;
      search?: string;
      publicationStates?: string[];
      pinnedConversationIds?: string[];
      priorityConversationIds?: string[];
    }) => {
      const filtered = filterSidebarConversations(conversations, {
        projectIds,
        archivedOnly,
        search,
        publicationStates,
        pinnedConversationIds,
        priorityConversationIds,
      });
      const groups = new Map<string, { key: string; label: string; total: number }>();
      for (const item of filtered) {
        const key = item.automationId ?? "__standalone__";
        const label = item.automationId ?? "Standalone";
        groups.set(key, {
          key,
          label,
          total: (groups.get(key)?.total ?? 0) + 1,
        });
      }
      return {
        data: Array.from(groups.values()).map((group) => ({
          ...group,
          offset: 0,
          limit: 1,
          hasMore: group.total > 1,
          rows: [],
        })),
        isLoading: false,
        isSuccess: true,
        isFetching: false,
        error: null,
        refetch: vi.fn(),
      };
    }
  );
  useAgentSidebarAutomationGroupMock.mockImplementation(
    ({
      groupKey = "",
      projectIds = [],
      archivedOnly = false,
      search = "",
      publicationStates = DEFAULT_SIDEBAR_PUBLICATION_STATE_FILTERS,
      pinnedConversationIds = [],
      priorityConversationIds = [],
    }: {
      groupKey?: string;
      projectIds?: string[];
      archivedOnly?: boolean;
      search?: string;
      publicationStates?: string[];
      pinnedConversationIds?: string[];
      priorityConversationIds?: string[];
    }) =>
      buildAgentSidebarGroupResult({
        key: groupKey,
        label: groupKey === "__standalone__" ? "Standalone" : groupKey,
        conversations: filterSidebarConversations(conversations, {
          projectIds,
          archivedOnly,
          search,
          publicationStates,
          pinnedConversationIds,
          priorityConversationIds,
        }).filter((item) => (item.automationId ?? "__standalone__") === groupKey),
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
    priorityConversationIds,
  }: {
    projectIds: string[];
    archivedOnly: boolean;
    search: string;
    publicationStates: string[];
    pinnedConversationIds: string[];
    priorityConversationIds: string[];
  }
) {
  const projectIdSet = new Set(projectIds);
  const pinnedIdSet = new Set(pinnedConversationIds);
  const priorityIdSet = new Set(priorityConversationIds);
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
      const priorityDelta =
        Number(priorityIdSet.has(right.id)) - Number(priorityIdSet.has(left.id));
      if (priorityDelta !== 0) {
        return priorityDelta;
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
    createdAt: "2026-04-23T09:00:00Z",
    updatedAt: "2026-04-23T09:00:00Z",
    archivedAt: null,
    convertedAt: null,
    verificationStatus: "unverified" as const,
    verificationInProgress: false,
    gapScore: null,
    inheritedPlanArtifactId: null,
    sessionPurpose: "general" as const,
    sessionFlow: "ideation" as const,
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
    startConversationFailure: null,
    defaultStartMode: "edit",
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

export function renderAgentsView(
  options: {
    footer?: ReactNode;
    onOpenAutomation?: (automationId: string) => void;
  } = {},
) {
  return renderWithProviders(
    <AgentsView
      projectId="project-1"
      onCreateProject={vi.fn()}
      {...(options.footer !== undefined ? { footer: options.footer } : {})}
      {...(options.onOpenAutomation
        ? { onOpenAutomation: options.onOpenAutomation }
        : {})}
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
  useAgentSidebarAutomationGroupIndexMock.mockReset();
  useAgentSidebarAutomationGroupMock.mockReset();
  useProjectsMock.mockReset();
  useHarnessProvidersMock.mockReset();
  useConversationMock.mockReset();
  useConversationSummaryMock.mockReset();
  startAgentConversationMock.mockReset();
  createAutomationDraftMock.mockReset();
  finalizeAutomationMock.mockReset();
  triggerAutomationRunNowMock.mockReset();
  updateAutomationSetupMock.mockReset();
  getPullRequestDetailMock.mockReset();
  getAgentConversationWorkspaceMock.mockReset();
  getAgentConversationWorkspaceFreshnessMock.mockReset();
  listAgentConversationWorkspacesByProjectMock.mockReset();
  listWorkspaceOpenTargetsMock.mockReset();
  openAgentConversationWorkspacePathMock.mockReset();
  listConversationsMock.mockReset();
  listAgentConversationWorkspacePublicationEventsMock.mockReset();
  publishAgentConversationWorkspaceMock.mockReset();
  commitAgentConversationWorkspaceLocallyMock.mockReset();
  updateWorkspaceFromBaseMock.mockReset();
  setAgentConversationWorkspaceAutoPublishMock.mockReset();
  setAgentConversationWorkspacePrSupervisionMock.mockReset();
  switchAgentConversationModeMock.mockReset();
  sendAgentMessageMock.mockReset();
  listAgentConversationIssuesMock.mockReset();
  createConversationMock.mockReset();
  spawnConversationSessionNamerMock.mockReset();
  updateConversationTitleMock.mockReset();
  archiveConversationMock.mockReset();
  restoreConversationMock.mockReset();
  getAgentRunningStatesMock.mockReset();
  getAgentConversationRuntimeIndexMock.mockReset();
  getAgentConversationRuntimeStatusesMock.mockReset();
  getPlanBranchesMock.mockReset();
  getTicketAssociationsMock.mockReset();
  startWorkFromTicketMock.mockReset();
  loadBranchBaseOptionsMock.mockReset();
  loadPullRequestBaseOptionsMock.mockReset();
  listIdeationSessionsMock.mockReset();
  getWorkspaceChangesMock.mockReset();
  getWorkspaceChangeSummaryMock.mockReset();
  getWorkspaceReviewMock.mockReset();
  getWorkspacePrAnnotationsMock.mockReset();
  getWorkspaceReviewHunkAnnotationsMock.mockReset();
  getWorkspaceReviewContextMock.mockReset();
  startWorkspaceReviewMock.mockReset();
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
  getWorkspaceRepairSummaryMock.mockReset();
  getWorkspaceRepairStagedChangesMock.mockReset();
  getWorkspaceRepairUnstagedChangesMock.mockReset();
  getWorkspaceRepairConflictDiffMock.mockReset();
  getWorkspaceRepairStagedDiffMock.mockReset();
  getWorkspaceRepairUnstagedDiffMock.mockReset();
  listAgentTasksMock.mockReset();
  listAgentTaskListsMock.mockReset();
  listAgentTaskListTasksMock.mockReset();
  precomputePrDescriptionMock.mockReset();
  toastDismissMock.mockReset();
  toastErrorMock.mockReset();
  toastInfoMock.mockReset();
  toastLoadingMock.mockReset();
  toastSuccessMock.mockReset();
  integratedChatPanelRenderMock.mockReset();
  integratedChatPanelUserMessageSentMock.mockReset();
  preloadAgentsArtifactPaneMock.mockReset();
  artifactPaneModuleLoadedMock.mockReset();
  artifactPaneRenderMock.mockReset();
  realPublishPanelState.enabled = false;
  realPublishPanelState.reviewContext = null;
  resetAgentWorkspaceOperationRegistryForTests();
  resetAgentWorkspaceOperationDismissalsForTests();
  preloadAgentTerminalExperienceMock.mockReset();
  terminalDrawerModuleLoadedMock.mockReset();
  terminalDrawerMountMock.mockReset();
  terminalDrawerUnmountMock.mockReset();
  webviewDragDropHandlers.splice(0);
  webviewOnDragDropEventMock.mockReset();
  webviewDragDropUnlistenMock.mockReset();

  vi.mocked(ideationApi.settings.get).mockResolvedValue(defaultIdeationSettings);

  sendAgentMessageMock.mockResolvedValue({
    conversationId: "conversation-2",
    agentRunId: "run-2",
    isNewConversation: true,
    wasQueued: false,
    queuedAsPending: false,
    queuedMessageId: null,
  });
  getPullRequestDetailMock.mockResolvedValue({
    state: "loaded",
    origin: "ownedOutbound",
    description: {
      number: 42,
      title: "Published pull request",
      body: null,
      author: "octocat",
      createdAt: "2026-07-23T15:00:00Z",
      url: "https://github.com/mock/project/pull/42",
      state: "open",
      isDraft: false,
      headRefName: "ralphx/ralphx/agent-abcdef12",
      baseRefName: "main",
    },
    checks: [],
    reviewSummary: null,
    issueComments: [],
    reviewThread: [],
    rxConversations: [],
    linkedTickets: [],
    sourcesUnavailable: [],
  });
  listAgentConversationIssuesMock.mockResolvedValue([]);
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
  getTicketAssociationsMock.mockResolvedValue({
    tasks: [],
    proposals: [],
    sessions: [],
    conversations: [],
    pullRequests: [],
    checks: [],
    qa: [],
    specs: [],
  });
  loadBranchBaseOptionsMock.mockResolvedValue({
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
        key: "local_branch:feature/new-base",
        label: "feature/new-base",
        detail: "Local branch",
        source: "local",
        selection: {
          kind: "local_branch",
          ref: "feature/new-base",
          displayName: "feature/new-base",
        },
      },
    ],
    selectedKey: "project_default:main",
  });
  loadPullRequestBaseOptionsMock.mockResolvedValue([
    {
      key: "pull_request:42:feature/pr-base",
      label: "#42 Add PR base",
      detail: "feature/pr-base -> main",
      source: "pull_request",
      selection: {
        kind: "local_branch",
        ref: "feature/pr-base",
        displayName: "PR #42: Add PR base",
        sourcePullRequest: {
          number: 42,
          url: "https://github.com/mock/project/pull/42",
          title: "Add PR base",
          headRefName: "feature/pr-base",
          baseRefName: "main",
          headRefOid: "pr-head-sha",
        },
      },
    },
  ]);
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
  getWorkspacePrAnnotationsMock.mockResolvedValue({
    prNumber: 0,
    headSha: null,
    annotations: [],
    sourcesUnavailable: [],
  });
  getWorkspaceReviewHunkAnnotationsMock.mockResolvedValue({
    artifactId: null,
    artifactVersion: null,
    targetScope: null,
    headSha: null,
    diffFingerprint: null,
    annotations: [],
  });
  getWorkspaceReviewContextMock.mockResolvedValue({
    success: true,
    workspace: conversationWorkspace,
    events: [],
    target: null,
    monitor: {
      status: "idle",
      reviewArtifactId: null,
      reviewArtifactVersion: null,
    },
    isCurrent: false,
    isOutdated: false,
    shouldShowTab: false,
  });
  startWorkspaceReviewMock.mockResolvedValue({
    success: true,
    target: null,
    monitor: {
      status: "idle",
      reviewArtifactId: null,
      reviewArtifactVersion: null,
    },
    isCurrent: false,
    isOutdated: false,
    shouldShowTab: false,
    started: false,
    skippedReason: "no_reviewable_changes",
    wasQueued: false,
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
  getWorkspaceRepairSummaryMock.mockResolvedValue({
    supportsWorktreeModes: true,
    staged: { fileCount: 0, additions: 0, deletions: 0 },
    unstaged: { fileCount: 0, additions: 0, deletions: 0 },
    conflicted: { fileCount: 0, files: [] },
    repairState: {
      expectedBranch: "ralphx/demo/agent-conversation-1",
      checkedOutBranch: "HEAD",
      rebaseInProgress: true,
      mergeInProgress: false,
    },
  });
  getWorkspaceRepairStagedChangesMock.mockResolvedValue([]);
  getWorkspaceRepairUnstagedChangesMock.mockResolvedValue([]);
  getWorkspaceRepairConflictDiffMock.mockResolvedValue({
    filePath: "frontend/src/App.tsx",
    baseContent: "base\n",
    oursContent: "ours\n",
    theirsContent: "theirs\n",
    mergedWithMarkers: "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n",
    language: "typescript",
  });
  getWorkspaceRepairStagedDiffMock.mockResolvedValue("");
  getWorkspaceRepairUnstagedDiffMock.mockResolvedValue("");
  listAgentTasksMock.mockResolvedValue([]);
  listAgentTaskListsMock.mockResolvedValue([]);
  listAgentTaskListTasksMock.mockResolvedValue([]);
  precomputePrDescriptionMock.mockResolvedValue({
    conversationId: "conversation-1",
    status: "skipped",
    cacheStatus: null,
    reason: "no_reviewable_commits",
  });
  listAgentConversationWorkspacePublicationEventsMock.mockResolvedValue([]);
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
      autoPublishInitialPrEnabled: false,
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
  updateWorkspaceFromBaseMock.mockImplementation(
    async (
      conversationId: string,
      base?: {
        kind: string;
        ref: string;
        displayName: string;
        sourcePullRequest?: {
          number: number;
          url?: string | null;
          title?: string | null;
          headRefName: string;
          baseRefName?: string | null;
          headRefOid?: string | null;
        } | null;
      } | null,
    ) => ({
      workspace: conversationWorkspace({
        conversationId,
        baseRefKind: base?.kind ?? "project_default",
        baseRef: base?.ref ?? "main",
        baseDisplayName: base?.displayName ?? "Project default (main)",
        baseCommit: "updated-base",
        sourcePullRequest: base?.sourcePullRequest ?? null,
      }),
      updated: true,
      targetRef: base?.ref ?? "origin/main",
      baseCommit: "updated-base",
    }),
  );
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
      autoPublishInitialPrEnabled: false,
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
      autoPublishInitialPrEnabled: input.autoPublishEnabled,
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
  createAutomationDraftMock.mockResolvedValue({
    automation: {
      id: "automation-1",
      projectId: "project-1",
      name: "Automation",
      status: "draft",
      pausedReasonCode: null,
      pausedReasonDetail: null,
      goalPrompt: "",
      setupConversationId: "automation-setup-1",
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
    setupConversationId: "automation-setup-1",
  });
  finalizeAutomationMock.mockResolvedValue({
    id: "automation-1",
    projectId: "project-1",
    name: "Automation",
    status: "active",
    pausedReasonCode: null,
    pausedReasonDetail: null,
    goalPrompt: "Keep dependencies updated",
    setupConversationId: "automation-setup-1",
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
    firstRunPrompt: "Update dependencies",
    setupAnalysisSummary: "Ready",
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
  });
  triggerAutomationRunNowMock.mockResolvedValue({ scheduled: true, reason: null });
  updateAutomationSetupMock.mockResolvedValue({
    id: "automation-1",
    projectId: "project-1",
    name: "Automation",
    status: "draft",
    pausedReasonCode: null,
    pausedReasonDetail: null,
    goalPrompt: "",
    setupConversationId: "automation-setup-1",
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
  });
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
  archiveConversationMock.mockResolvedValue({
    conversation: conversation(),
    cleanup: {
      runtimeShutdownSucceeded: true,
      cleanupClaim: "claimed",
      localCleanup: "cleaned",
      message: null,
    },
  });
  restoreConversationMock.mockResolvedValue(undefined);
  getAgentRunningStatesMock.mockResolvedValue({});
  getAgentConversationRuntimeIndexMock.mockResolvedValue({
    conversationId: "conversation-1",
    rows: [],
  });
  getAgentConversationRuntimeStatusesMock.mockResolvedValue({});
  useConversationSummaryMock.mockImplementation((conversationId: string | null) => {
    const query = useConversationMock(conversationId);
    return {
      ...query,
      data: query?.data?.conversation ?? null,
    };
  });
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue(undefined);

  useChatStore.setState({
    messages: {},
    context: null,
    isLoading: false,
    activeConversationIds: {},
    activeAgentRunIds: {},
    queuedMessages: {},
    agentStatus: {},
    agentActivityLabels: {},
    isSending: {},
    lastAgentEventTimestamp: {},
    toolCallStartTimes: {},
    lastToolCallCompletionTimestamp: {},
    toolCallCompletionTimestamps: {},
    effectiveModel: {},
    composerDraftsByKey: {},
  });
  useUiStore.getState().closeModal();
  useUiStore.getState().setExecutionPaused(false);
  useUiStore.getState().setExecutionQueuedCount(0, 0);
  resetAgentSessionState();
  useAgentArtifactUiStore.setState({
    artifactByConversationId: {},
    publishSubTabRequest: null,
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
