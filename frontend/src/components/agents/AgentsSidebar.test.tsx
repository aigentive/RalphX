import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useState, type ComponentProps } from "react";
import userEvent from "@testing-library/user-event";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useChatStore } from "@/stores/chatStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import type {
  AgentConversationWorkspace,
  AgentSidebarAttentionLane,
} from "@/api/chat";
import type { Project } from "@/types/project";
import type { AgentConversation } from "./agentConversations";
import {
  formatAgentConversationCreatedAt,
  getAgentConversationStoreKey,
} from "./agentConversations";
import { AgentsSidebar } from "./AgentsSidebar";

type ConversationsResult = {
  data: AgentConversation[];
  isLoading: boolean;
  total?: number;
  hasNextPage?: boolean;
  isFetchingNextPage?: boolean;
  fetchNextPage?: () => Promise<unknown>;
};

type VirtuosoMockRange = {
  startIndex: number;
  endIndex: number;
};

type VirtuosoMockDimensions = {
  clientHeight: number;
  scrollHeight: number;
};

type VirtuosoMockStateSnapshot = {
  ranges: Array<{
    startIndex: number;
    endIndex: number;
    size: number;
  }>;
  scrollTop: number;
};

const { virtuosoMockState } = vi.hoisted(() => ({
  virtuosoMockState: {
    dimensionsByTestId: new Map<string, VirtuosoMockDimensions>(),
    endReachedByTestId: new Map<string, () => void>(),
    rangeByTestId: new Map<string, VirtuosoMockRange>(),
    restoreStateByTestIdAndCount: new Map<string, VirtuosoMockStateSnapshot>(),
    scrollToCallsByTestId: new Map<string, number>(),
    resetScrollAfterMountWithoutStateByTestId: new Set<string>(),
    asyncGetStateByTestId: new Set<string>(),
  },
}));
const { conversationsByProject } = vi.hoisted(() => ({
  conversationsByProject: new Map<string, ConversationsResult>(),
}));
const { projectConversationCalls } = vi.hoisted(() => ({
  projectConversationCalls: [] as Array<{
    projectId: string | null;
    includeArchived: boolean;
    options?: { search?: string; enabled?: boolean };
    pinnedConversationIds?: string[];
    priorityConversationIds?: string[];
    minimumRowCount?: number;
    pageSize?: number;
  }>,
}));
const { archivedConversationCounts, archivedCountCalls } = vi.hoisted(() => ({
  archivedConversationCounts: new Map<string, number>(),
  archivedCountCalls: [] as string[][],
}));
const { workspacesByProject, workspaceCalls } = vi.hoisted(() => ({
  workspacesByProject: new Map<string, AgentConversationWorkspace[]>(),
  workspaceCalls: [] as Array<{
    projectId: string | null;
    enabled?: boolean;
  }>,
}));
const { publicationGroupCalls } = vi.hoisted(() => ({
  publicationGroupCalls: [] as Array<{
    projectIds: string[];
    publicationState: string;
    archivedOnly: boolean;
    search: string;
    pinnedConversationIds: string[];
    priorityConversationIds?: string[];
    sort: string;
    minimumRowCount?: number;
  }>,
}));
const { inboxLaneByConversationId } = vi.hoisted(() => ({
  inboxLaneByConversationId: new Map<
    string,
    {
      lane: AgentSidebarAttentionLane;
      actionVerb: string;
      parkedDelegateCount?: number;
      reviewState?: string | null;
    }
  >(),
}));
const { inboxGroupCalls } = vi.hoisted(() => ({
  inboxGroupCalls: [] as Array<{
    lane: AgentSidebarAttentionLane;
    priorityConversationIds?: string[];
  }>,
}));
const { inboxGroupTotalsByLane } = vi.hoisted(() => ({
  inboxGroupTotalsByLane: new Map<string, number>(),
}));
const { mutedConversationIds } = vi.hoisted(() => ({
  mutedConversationIds: new Set<string>(),
}));
const { automationGroupIndexCalls, automationGroupCalls, automationLabels } = vi.hoisted(() => ({
  automationGroupIndexCalls: [] as Array<{
    projectIds: string[];
    archivedOnly: boolean;
    search: string;
    publicationStates: string[];
    pinnedConversationIds: string[];
    priorityConversationIds?: string[];
    sort: string;
  }>,
  automationGroupCalls: [] as Array<{
    groupKey: string;
    projectIds: string[];
    archivedOnly: boolean;
    search: string;
    publicationStates: string[];
    pinnedConversationIds: string[];
    priorityConversationIds?: string[];
    sort: string;
    minimumRowCount?: number;
    enabled?: boolean;
  }>,
  automationLabels: new Map<string, string>(),
}));
const { latestProjectOrderData } = vi.hoisted(() => ({
  latestProjectOrderData: { current: null as string[] | null },
}));
const { runningStatesHook, publicationPollingHook } = vi.hoisted(() => ({
  runningStatesHook: vi.fn(),
  publicationPollingHook: vi.fn(),
}));
const { prTemplateDialogCalls } = vi.hoisted(() => ({
  prTemplateDialogCalls: [] as Array<{
    open: boolean;
    projectId: string | null;
  }>,
}));

vi.mock("react-virtuoso", async () => {
  const React = await vi.importActual<typeof import("react")>("react");

  type VirtuosoMockProps = {
    data?: unknown[];
    itemContent?: (index: number, item: unknown) => React.ReactNode;
    computeItemKey?: (index: number, item: unknown) => React.Key;
    endReached?: (index: number) => void;
    initialScrollTop?: number;
    restoreStateFrom?: VirtuosoMockStateSnapshot;
    rangeChanged?: (range: VirtuosoMockRange) => void;
    scrollerRef?: (node: HTMLElement | Window | null) => void;
    className?: string;
    style?: React.CSSProperties;
    "data-testid"?: string;
  };

  const Virtuoso = React.forwardRef<unknown, VirtuosoMockProps>(
    function MockVirtuoso(props, ref) {
      const {
        className,
        computeItemKey,
        data = [],
        endReached,
        initialScrollTop,
        itemContent,
        rangeChanged,
        restoreStateFrom,
        scrollerRef,
        style,
        "data-testid": dataTestId,
      } = props;
      const scrollerNodeRef = React.useRef<HTMLDivElement | null>(null);
      const testId = dataTestId ?? "mock-virtuoso";
      if (restoreStateFrom) {
        virtuosoMockState.restoreStateByTestIdAndCount.set(
          `${testId}:${data.length}`,
          restoreStateFrom
        );
      }
      const range = virtuosoMockState.rangeByTestId.get(testId);
      const startIndex = range?.startIndex ?? 0;
      const endIndex = range?.endIndex ?? data.length - 1;
      const visibleItems = data
        .map((item, index) => ({ item, index }))
        .slice(startIndex, endIndex + 1);

      React.useImperativeHandle(
        ref,
        () => ({
          getState: (stateCb: (state: VirtuosoMockStateSnapshot) => void) => {
            const snapshot = {
              ranges: [
                {
                  startIndex: 0,
                  endIndex: Math.max(0, data.length - 1),
                  size: 46,
                },
              ],
              scrollTop: scrollerNodeRef.current?.scrollTop ?? 0,
            };
            if (virtuosoMockState.asyncGetStateByTestId.has(testId)) {
              window.requestAnimationFrame(() => stateCb(snapshot));
              return;
            }
            stateCb(snapshot);
          },
          scrollTo: (location: ScrollToOptions) => {
            if (typeof location.top === "number" && scrollerNodeRef.current) {
              virtuosoMockState.scrollToCallsByTestId.set(
                testId,
                (virtuosoMockState.scrollToCallsByTestId.get(testId) ?? 0) + 1
              );
              scrollerNodeRef.current.scrollTop = location.top;
            }
          },
        }),
        [data.length, testId]
      );

      React.useEffect(() => {
        if (data.length === 0) {
          return;
        }
        rangeChanged?.({
          startIndex,
          endIndex: Math.min(endIndex, data.length - 1),
        });
      }, [data.length, endIndex, rangeChanged, startIndex]);

      React.useEffect(() => {
        virtuosoMockState.endReachedByTestId.set(testId, () => {
          endReached?.(data.length - 1);
        });
        return () => {
          virtuosoMockState.endReachedByTestId.delete(testId);
        };
      }, [data.length, endReached, testId]);

      React.useEffect(() => {
        const node = scrollerNodeRef.current;
        if (
          !node ||
          restoreStateFrom ||
          !virtuosoMockState.resetScrollAfterMountWithoutStateByTestId.has(testId)
        ) {
          return;
        }

        const frameId = window.requestAnimationFrame(() => {
          node.scrollTop = 0;
        });
        return () => window.cancelAnimationFrame(frameId);
      }, [restoreStateFrom, testId]);

      const setScrollerRef = React.useCallback(
        (node: HTMLDivElement | null) => {
          scrollerNodeRef.current = node;
          if (node) {
            const dimensions = virtuosoMockState.dimensionsByTestId.get(testId);
            if (dimensions) {
              Object.defineProperty(node, "clientHeight", {
                configurable: true,
                value: dimensions.clientHeight,
              });
              Object.defineProperty(node, "scrollHeight", {
                configurable: true,
                value: dimensions.scrollHeight,
              });
            }
            if (restoreStateFrom) {
              node.scrollTop = restoreStateFrom.scrollTop;
            } else if (typeof initialScrollTop === "number") {
              node.scrollTop = initialScrollTop;
            }
          }
          scrollerRef?.(node);
        },
        [initialScrollTop, restoreStateFrom, scrollerRef, testId]
      );

      return (
        <div
          ref={setScrollerRef}
          data-testid={testId}
          data-count={data.length}
          data-visible-end={endIndex}
          data-visible-start={startIndex}
          className={className}
          style={style}
        >
          {visibleItems.map(({ item, index }) => (
            <div key={computeItemKey?.(index, item) ?? index}>
              {itemContent?.(index, item)}
            </div>
          ))}
        </div>
      );
    }
  );

  return { Virtuoso };
});

vi.mock("./useProjectAgentConversations", () => ({
  useProjectAgentConversations: (
    projectId: string | null | undefined,
    includeArchived = false,
    options?: { search?: string; enabled?: boolean }
  ) =>
    (() => {
      projectConversationCalls.push({
        projectId: projectId ?? null,
        includeArchived,
        options,
      });
      const result = conversationsByProject.get(projectId ?? "");
      if (result) {
        return {
          ...result,
          total: result.total ?? result.data.length,
        };
      }
      return {
        data: [],
        isLoading: false,
        total: 0,
        hasNextPage: false,
        isFetchingNextPage: false,
        fetchNextPage: vi.fn(),
      };
    })(),
}));

vi.mock("./useAgentSidebarRunningStates", () => ({
  useAgentSidebarRunningStates: runningStatesHook,
}));

vi.mock("./useAgentSidebarPublicationPolling", () => ({
  useAgentSidebarPublicationPolling: publicationPollingHook,
  workspacePublicationFingerprint: (state: string, label: string | null | undefined) =>
    `${state}\u0000${label?.trim().toLowerCase() ?? ""}`,
}));

vi.mock("./PrTemplateEditorDialog", () => {
  return {
    PrTemplateEditorDialog: ({
      open,
      project,
    }: {
      open: boolean;
      project: Project | null;
    }) => {
      prTemplateDialogCalls.push({
        open,
        projectId: project?.id ?? null,
      });
      return open ? (
        <div data-testid="pr-template-editor-dialog">
          Edit PR Template for {project?.name}
        </div>
      ) : null;
    },
  };
});

vi.mock("./useArchivedConversationCounts", () => ({
  useArchivedConversationCounts: (projectIds: string[]) => {
    archivedCountCalls.push(projectIds);
    const byProjectId = Object.fromEntries(
      projectIds.map((projectId) => [projectId, archivedConversationCounts.get(projectId) ?? 0])
    );
    const totalArchivedCount = Object.values(byProjectId).reduce(
      (sum, count) => sum + count,
      0
    );

    return {
      byProjectId,
      totalArchivedCount,
      isLoading: false,
    };
  },
}));

vi.mock("./useProjectAgentConversationWorkspaces", () => ({
  useProjectAgentConversationWorkspaces: (
    projectId: string | null | undefined,
    options?: { enabled?: boolean }
  ) => {
    workspaceCalls.push({
      projectId: projectId ?? null,
      enabled: options?.enabled,
    });
    return {
      data: workspacesByProject.get(projectId ?? "") ?? [],
      isLoading: false,
    };
  },
}));

vi.mock("./useAgentSidebarPublicationGroup", () => {
  const getPublicationLabel = (state: string): string =>
    state
      .split("_")
      .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
      .join(" ");

  const buildGroupResult = ({
    projectIds,
    groupKey,
    groupLabel,
    archivedOnly,
    search,
    publicationStates,
    pinnedConversationIds,
    priorityConversationIds = [],
    sort = "latest",
    total,
    hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
    isLoading,
  }: {
    projectIds: string[];
    groupKey: string;
    groupLabel: string;
    archivedOnly: boolean;
    search: string;
    publicationStates: string[];
    pinnedConversationIds: string[];
    priorityConversationIds?: string[];
    sort?: string;
    total?: number;
    hasNextPage?: boolean;
    isFetchingNextPage?: boolean;
    fetchNextPage?: () => Promise<unknown>;
    isLoading?: boolean;
  }) => {
    const getPublicationState = (
      workspace: AgentConversationWorkspace | null
    ): string => {
      const prStatus = workspace?.publicationPrStatus?.trim().toLowerCase();
      const pushStatus = workspace?.publicationPushStatus?.trim().toLowerCase();
      if (prStatus === "merged") return "merged";
      if (prStatus === "closed") return "closed";
      if (pushStatus === "needs_agent") return "uncommitted";
      if (
        pushStatus === "pending" ||
        pushStatus === "failed" ||
        pushStatus === "description_failed"
      ) {
        return "unpushed";
      }
      if (prStatus === "draft") return "draft";
      return "active";
    };
    const getPublicationLabel = (
      workspace: AgentConversationWorkspace | null,
      state: string
    ): string | null => {
      const supervisionStatus = workspace?.prSupervisionStatus?.trim().toLowerCase();
      if (
        state === "active" ||
        state === "uncommitted" ||
        state === "unpushed"
      ) {
        if (supervisionStatus === "fixing" || supervisionStatus === "publishing") {
          return "fixing";
        }
        if (supervisionStatus === "blocked") {
          return "blocked";
        }
        if (
          supervisionStatus === "waiting" ||
          supervisionStatus === "waiting_for_checks"
        ) {
          return "waiting";
        }
        if (
          supervisionStatus === "monitoring" &&
          workspace?.prAutoMergeCurrent === true
        ) {
          return "auto-merge";
        }
      }
      return state === "active" ? null : state;
    };
    const normalizedSearch = search.trim().toLowerCase();
    const pinnedIds = new Set(pinnedConversationIds);
    const priorityIds = new Set(priorityConversationIds);
    const workspaceByConversationId = new Map(
      projectIds.flatMap((projectId) =>
        (workspacesByProject.get(projectId) ?? []).map((workspace) => [
          workspace.conversationId,
          workspace,
        ] as const)
      )
    );
    const rows = projectIds
      .flatMap((projectId) => conversationsByProject.get(projectId)?.data ?? [])
      .filter((conversation) =>
        archivedOnly ? Boolean(conversation.archivedAt) : !conversation.archivedAt
      )
      .filter((conversation) => {
        if (!normalizedSearch) return true;
        return (conversation.title ?? "Untitled agent")
          .toLowerCase()
          .includes(normalizedSearch);
      })
      .map((conversation) => {
        const workspace = workspaceByConversationId.get(conversation.id) ?? null;
        const state = getPublicationState(workspace);
        return {
          conversation,
          workspace,
          refKind: workspace?.publicationPrNumber != null ? "pull-request" : "branch",
          refLabel:
            workspace?.publicationPrNumber != null
              ? `PR #${workspace.publicationPrNumber}`
              : workspace?.baseRef ?? "master",
          publicationState: state,
          publicationLabel: getPublicationLabel(workspace, state),
          isMuted: mutedConversationIds.has(conversation.id),
        };
      })
      .filter((row) => publicationStates.includes(row.publicationState))
      .sort((left, right) => {
        const pinnedDelta =
          Number(pinnedIds.has(right.conversation.id)) -
          Number(pinnedIds.has(left.conversation.id));
        if (pinnedDelta !== 0) return pinnedDelta;
        const priorityDelta =
          Number(priorityIds.has(right.conversation.id)) -
          Number(priorityIds.has(left.conversation.id));
        if (priorityDelta !== 0) return priorityDelta;
        if (sort === "az" || sort === "za") {
          const leftTitle = (left.conversation.title ?? "Untitled agent").toLowerCase();
          const rightTitle = (right.conversation.title ?? "Untitled agent").toLowerCase();
          const titleDelta = leftTitle.localeCompare(rightTitle);
          if (titleDelta !== 0) return sort === "az" ? titleDelta : -titleDelta;
        }
        return (
          new Date(right.conversation.createdAt).getTime() -
          new Date(left.conversation.createdAt).getTime()
        );
      });

    return {
      group: {
        key: groupKey,
        label: groupLabel,
        total: total ?? rows.length,
        offset: 0,
        limit: 8,
        hasMore: hasNextPage ?? false,
        rows,
      },
      isLoading: isLoading ?? false,
      hasNextPage: hasNextPage ?? false,
      isFetchingNextPage: isFetchingNextPage ?? false,
      fetchNextPage: fetchNextPage ?? vi.fn(),
    };
  };

  return {
    useAgentSidebarPublicationGroup: ({
      projectIds,
      publicationState,
      archivedOnly,
      search,
      pinnedConversationIds,
      priorityConversationIds,
      sort,
      minimumRowCount,
    }: {
      projectIds: string[];
      publicationState: string;
      archivedOnly: boolean;
      search: string;
      pinnedConversationIds: string[];
      priorityConversationIds?: string[];
      sort: string;
      minimumRowCount?: number;
    }) => {
      const firstProjectResult = projectIds
        .map((projectId) => conversationsByProject.get(projectId))
        .find(Boolean);
      publicationGroupCalls.push({
        projectIds,
        publicationState,
        archivedOnly,
        search,
        pinnedConversationIds,
        priorityConversationIds,
        sort,
        minimumRowCount,
      });
      return buildGroupResult({
        projectIds,
        groupKey: publicationState,
        groupLabel: getPublicationLabel(publicationState),
        archivedOnly,
        search,
        publicationStates: [publicationState],
        pinnedConversationIds,
        priorityConversationIds,
        sort,
        hasNextPage: firstProjectResult?.hasNextPage,
        isFetchingNextPage: firstProjectResult?.isFetchingNextPage,
        fetchNextPage: firstProjectResult?.fetchNextPage,
        isLoading: firstProjectResult?.isLoading,
      });
    },
    useAgentSidebarInboxGroup: ({
      lane,
      projectIds,
      archivedOnly,
      search,
      publicationStates,
      pinnedConversationIds,
      priorityConversationIds,
      sort,
    }: {
      lane: "needs" | "working" | "stale" | "done";
      projectIds: string[];
      archivedOnly: boolean;
      search: string;
      publicationStates: string[];
      pinnedConversationIds: string[];
      priorityConversationIds?: string[];
      sort: string;
    }) => {
      const firstProjectResult = projectIds
        .map((projectId) => conversationsByProject.get(projectId))
        .find(Boolean);
      inboxGroupCalls.push({ lane, priorityConversationIds });
      const result = buildGroupResult({
        projectIds,
        groupKey: lane,
        groupLabel: lane,
        archivedOnly,
        search,
        publicationStates,
        pinnedConversationIds,
        priorityConversationIds,
        sort,
        isLoading: firstProjectResult?.isLoading,
      });
      const rows = result.group.rows
        .filter((row) => inboxLaneByConversationId.get(row.conversation.id)?.lane === lane)
        .map((row) => ({
          ...row,
          attentionLane: lane,
          parkedDelegateCount:
            inboxLaneByConversationId.get(row.conversation.id)?.parkedDelegateCount ?? 0,
          actionVerb: inboxLaneByConversationId.get(row.conversation.id)?.actionVerb ?? "",
          reviewState:
            inboxLaneByConversationId.get(row.conversation.id)?.reviewState ?? null,
        }));
      return {
        ...result,
        group: {
          ...result.group,
          rows,
          total: inboxGroupTotalsByLane.get(lane) ?? rows.length,
        },
        hasNextPage: firstProjectResult?.hasNextPage ?? false,
        isFetchingNextPage: firstProjectResult?.isFetchingNextPage ?? false,
        fetchNextPage: firstProjectResult?.fetchNextPage ?? vi.fn(),
      };
    },
    useAgentSidebarProjectGroup: ({
      projectId,
      archivedOnly,
      search,
      publicationStates,
      pinnedConversationIds,
      priorityConversationIds,
      minimumRowCount,
      pageSize,
    }: {
      projectId: string | null | undefined;
      archivedOnly: boolean;
      search: string;
      publicationStates: string[];
      pinnedConversationIds: string[];
      priorityConversationIds?: string[];
      minimumRowCount?: number;
      pageSize?: number;
    }) => {
      const projectResult = conversationsByProject.get(projectId ?? "");
      projectConversationCalls.push({
        projectId: projectId ?? null,
        includeArchived: archivedOnly,
        options: { search, enabled: true },
        pinnedConversationIds,
        priorityConversationIds,
        minimumRowCount,
        pageSize,
      });
      return buildGroupResult({
        projectIds: projectId ? [projectId] : [],
        groupKey: projectId ?? "",
        groupLabel: projectId ?? "",
        archivedOnly,
        search,
        publicationStates,
        pinnedConversationIds,
        priorityConversationIds,
        total: projectResult?.total,
        hasNextPage: projectResult?.hasNextPage,
        isFetchingNextPage: projectResult?.isFetchingNextPage,
        fetchNextPage: projectResult?.fetchNextPage,
        isLoading: projectResult?.isLoading,
      });
    },
    useAgentSidebarAutomationGroupIndex: ({
      projectIds,
      archivedOnly,
      search,
      publicationStates,
      pinnedConversationIds,
      priorityConversationIds,
      sort,
    }: {
      projectIds: string[];
      archivedOnly: boolean;
      search: string;
      publicationStates: string[];
      pinnedConversationIds: string[];
      priorityConversationIds?: string[];
      sort: string;
    }) => {
      automationGroupIndexCalls.push({
        projectIds,
        archivedOnly,
        search,
        publicationStates,
        pinnedConversationIds,
        priorityConversationIds,
        sort,
      });
      const rows = buildGroupResult({
        projectIds,
        groupKey: "automation-index",
        groupLabel: "Automations",
        archivedOnly,
        search,
        publicationStates,
        pinnedConversationIds,
        priorityConversationIds,
        sort,
      }).group.rows;
      const groups = new Map<string, { key: string; label: string; total: number }>();
      for (const row of rows) {
        const key = row.conversation.automationId ?? "__standalone__";
        const label =
          key === "__standalone__"
            ? "Standalone"
            : automationLabels.get(key) ?? key;
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
    },
    useAgentSidebarAutomationGroup: ({
      groupKey,
      projectIds,
      archivedOnly,
      search,
      publicationStates,
      pinnedConversationIds,
      priorityConversationIds,
      sort,
      minimumRowCount,
      enabled = true,
    }: {
      groupKey: string;
      projectIds: string[];
      archivedOnly: boolean;
      search: string;
      publicationStates: string[];
      pinnedConversationIds: string[];
      priorityConversationIds?: string[];
      sort: string;
      minimumRowCount?: number;
      enabled?: boolean;
    }) => {
      automationGroupCalls.push({
        groupKey,
        projectIds,
        archivedOnly,
        search,
        publicationStates,
        pinnedConversationIds,
        priorityConversationIds,
        sort,
        minimumRowCount,
        enabled,
      });
      const label =
        groupKey === "__standalone__"
          ? "Standalone"
          : automationLabels.get(groupKey) ?? groupKey;
      const result = buildGroupResult({
        projectIds,
        groupKey,
        groupLabel: label,
        archivedOnly,
        search,
        publicationStates,
        pinnedConversationIds,
        priorityConversationIds,
        sort,
      });
      return {
        ...result,
        group: {
          ...result.group,
          rows: result.group.rows.filter(
            (row) => (row.conversation.automationId ?? "__standalone__") === groupKey
          ),
          total: result.group.rows.filter(
            (row) => (row.conversation.automationId ?? "__standalone__") === groupKey
          ).length,
        },
      };
    },
    useProjectGroupLatestOrder: () => ({
      data: latestProjectOrderData.current,
      isLoading: false,
    }),
  };
});

const project = (overrides: Partial<Project> = {}): Project => ({
  id: "project-1",
  name: "ralphx",
  workingDirectory: "/tmp/ralphx",
  gitMode: "worktree",
  baseBranch: null,
  worktreeParentDirectory: null,
  useFeatureBranches: true,
  mergeValidationMode: "block",
  detectedAnalysis: null,
  customAnalysis: null,
  analyzedAt: null,
  githubPrEnabled: false,
  createdAt: "2026-04-22T09:00:00Z",
  updatedAt: "2026-04-22T09:00:00Z",
  ...overrides,
});

const conversation = (
  overrides: Partial<AgentConversation> = {}
): AgentConversation => ({
  id: "conversation-1",
  contextType: "project",
  contextId: "project-1",
  claudeSessionId: null,
  providerSessionId: "thread-1",
  providerHarness: "codex",
  upstreamProvider: null,
  providerProfile: null,
  title: "Fix font scaling",
  messageCount: 1,
  lastMessageAt: "2026-04-22T12:00:00Z",
  createdAt: "2026-04-22T10:00:00Z",
  updatedAt: "2026-04-22T12:00:00Z",
  archivedAt: null,
  projectId: "project-1",
  ideationSessionId: null,
  ...overrides,
});

const workspace = (
  overrides: Partial<AgentConversationWorkspace> = {}
): AgentConversationWorkspace => ({
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
  createdAt: "2026-04-22T10:00:00Z",
  updatedAt: "2026-04-22T12:00:00Z",
  ...overrides,
});

function buildSidebarProps(
  projects: Project[] = [project()],
  props?: Partial<ComponentProps<typeof AgentsSidebar>>
): ComponentProps<typeof AgentsSidebar> {
  return {
    projects,
    focusedProjectId: "project-1",
    selectedConversationId: null,
    onFocusProject: vi.fn(),
    onSelectConversation: vi.fn(),
    onCreateAgent: vi.fn(),
    onCreateProject: vi.fn(),
    onArchiveProject: vi.fn(),
    onAutoRenameConversation: vi.fn(),
    onRenameConversation: vi.fn(),
    onArchiveConversation: vi.fn(),
    onBulkArchiveConversations: vi.fn(),
    onBulkMuteConversations: vi.fn(),
    onSetConversationMuted: vi.fn(),
    onRestoreConversation: vi.fn(),
    onForkConversation: vi.fn(),
    showArchived: false,
    onShowArchivedChange: vi.fn(),
    ...props,
  };
}

function renderSidebar(
  projects: Project[] = [project()],
  props?: Partial<ComponentProps<typeof AgentsSidebar>>
) {
  return render(
    <TooltipProvider delayDuration={0}>
      <AgentsSidebar {...buildSidebarProps(projects, props)} />
    </TooltipProvider>
  );
}

function getProjectRowOrder() {
  return screen
    .getAllByTestId((testId) => testId.startsWith("agents-project-project-"))
    .map((row) => row.getAttribute("data-testid"));
}

function getSessionRowOrder() {
  return screen
    .getAllByTestId((testId) => testId.startsWith("agents-session-conversation-"))
    .map((row) => row.getAttribute("data-testid"));
}

function rectWithHeight(height: number): DOMRect {
  return {
    bottom: height,
    height,
    left: 0,
    right: 0,
    top: 0,
    width: 0,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  } as DOMRect;
}

function mockMeasuredSidebarRowHeight(height: number) {
  return vi
    .spyOn(HTMLElement.prototype, "getBoundingClientRect")
    .mockImplementation(function getBoundingClientRect() {
      if ((this as HTMLElement).dataset.agentSidebarRowSlot === "true") {
        return rectWithHeight(height);
      }
      return rectWithHeight(0);
    });
}

function waitForAnimationFrame() {
  return new Promise<void>((resolve) => {
    window.requestAnimationFrame(() => resolve());
  });
}

describe("AgentsSidebar", () => {
  beforeEach(() => {
    conversationsByProject.clear();
    projectConversationCalls.length = 0;
    archivedConversationCounts.clear();
    archivedCountCalls.length = 0;
    workspacesByProject.clear();
    workspaceCalls.length = 0;
    publicationGroupCalls.length = 0;
    inboxLaneByConversationId.clear();
    inboxGroupCalls.length = 0;
    inboxGroupTotalsByLane.clear();
    mutedConversationIds.clear();
    automationGroupIndexCalls.length = 0;
    automationGroupCalls.length = 0;
    automationLabels.clear();
    prTemplateDialogCalls.length = 0;
    latestProjectOrderData.current = null;
    virtuosoMockState.dimensionsByTestId.clear();
    virtuosoMockState.endReachedByTestId.clear();
    virtuosoMockState.rangeByTestId.clear();
    virtuosoMockState.restoreStateByTestIdAndCount.clear();
    virtuosoMockState.scrollToCallsByTestId.clear();
    virtuosoMockState.resetScrollAfterMountWithoutStateByTestId.clear();
    virtuosoMockState.asyncGetStateByTestId.clear();
    runningStatesHook.mockClear();
    publicationPollingHook.mockClear();
    useChatStore.setState({ activeConversationIds: {}, agentStatus: {} });
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true, "project-2": false },
      showAllProjects: true,
      showEmptyProjectGroups: true,
      projectSort: "latest",
      sidebarGroupBy: "project",
      sidebarInboxActiveLane: "recent",
      sidebarProjectFilterIds: [],
      sidebarPublicationStateFilters: [
        "active",
        "draft",
        "merged",
        "closed",
        "uncommitted",
        "unpushed",
      ],
      pinnedConversationIds: {},
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("uses flat v27 panel chrome without light-theme blur or glow", () => {
    renderSidebar();

    const sidebar = screen.getByTestId("agents-sidebar");
    const inlineStyle = sidebar.getAttribute("style") ?? "";
    expect(inlineStyle).toContain("background-color: var(--app-sidebar-bg)");
    expect(inlineStyle).toContain("border-right-color: var(--app-sidebar-border)");
    expect(inlineStyle).toContain("box-shadow: none");
    expect(inlineStyle).not.toContain("backdrop");

    expect(screen.getByTestId("agents-new-agent")).toHaveTextContent("New");
    expect(screen.getByTestId("agents-new-agent").className).toContain("h-7");
    expect(screen.getByTestId("agents-add-project").className).toContain("rounded-[6px]");
  });

  it("keeps a lightweight list frame but does not mount Virtuoso while hidden", () => {
    conversationsByProject.set("project-1", {
      data: [conversation()],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    const hiddenSidebar = renderSidebar([project()], { isVisible: false });

    expect(
      screen.getByTestId("agents-sidebar-session-list-project-1-frame")
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-sidebar-session-list-project-1")
    ).not.toBeInTheDocument();

    hiddenSidebar.rerender(
      <TooltipProvider delayDuration={0}>
        <AgentsSidebar {...buildSidebarProps([project()], { isVisible: true })} />
      </TooltipProvider>
    );

    expect(screen.getByTestId("agents-sidebar-session-list-project-1")).toBeInTheDocument();
  });

  it("renders the data-driven No project group and selects its standalone row without a project id", async () => {
    const standalone = conversation({
      id: "standalone-1",
      contextType: "standalone",
      contextId: "standalone-1",
      projectId: null,
      title: "Private exploration",
    });
    conversationsByProject.set("__no_project__", {
      data: [standalone],
      isLoading: false,
      total: 1,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    const onSelectConversation = vi.fn();

    renderSidebar([], { onSelectConversation });

    expect(screen.getByTestId("agents-project-__no_project__")).toHaveTextContent(
      "No project",
    );
    expect(screen.getByText("Private exploration")).toBeInTheDocument();
    await userEvent.click(screen.getByText("Private exploration"));
    expect(onSelectConversation).toHaveBeenCalledWith(null, standalone);
    expect(runningStatesHook).toHaveBeenCalledWith(
      [expect.objectContaining({ id: "standalone-1", contextType: "standalone" })],
      true,
    );
  });

  it("omits the No project group when its backend group has no rows", () => {
    renderSidebar([project()]);

    expect(screen.queryByTestId("agents-project-__no_project__")).not.toBeInTheDocument();
  });

  it("orders sessions by created time and shows created time instead of provider", () => {
    const older = conversation({
      id: "older",
      title: "Older agent",
      createdAt: "2026-04-22T10:00:00Z",
      lastMessageAt: "2026-04-22T12:00:00Z",
    });
    const newer = conversation({
      id: "newer",
      title: "Newer agent",
      createdAt: "2026-04-22T11:00:00Z",
      lastMessageAt: "2026-04-22T11:01:00Z",
    });
    conversationsByProject.set("project-1", {
      data: [newer, older],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    const rows = screen.getAllByTestId(/agents-session-/);
    expect(rows.map((row) => row.getAttribute("data-testid"))).toEqual([
      "agents-session-newer",
      "agents-session-older",
    ]);

    const firstRow = within(rows[0]);
    expect(firstRow.getByText("Newer agent")).toBeInTheDocument();
    expect(
      firstRow.getByText(formatAgentConversationCreatedAt(newer.createdAt))
    ).toBeInTheDocument();
    expect(firstRow.queryByText("codex")).not.toBeInTheDocument();
  });

  it("builder_conversation_renders_visible_in_sidebar_with_distinct_label_and_icon", () => {
    conversationsByProject.set("project-1", {
      data: [conversation({ agentMode: "persona_builder", title: "Persona builder" })],
      isLoading: false,
    });

    renderSidebar();

    const row = within(screen.getByTestId("agents-session-conversation-1"));
    expect(row.getByText("Persona Builder")).toBeInTheDocument();
    expect(row.getByTestId("agents-mode-icon-persona_builder")).toBeInTheDocument();
  });

  it("shows compact conversation time with a full timestamp title", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 3, 25, 16, 33, 0));
    const activeConversation = conversation({
      createdAt: new Date(2026, 3, 25, 14, 33, 0).toISOString(),
    });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    const row = within(screen.getByTestId("agents-session-conversation-1"));
    expect(row.getByText("2h")).toHaveAttribute(
      "title",
      "Apr 25, 2026, 2:33 PM",
    );
  });

  it("shows compact day labels before switching to a date-only label", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 3, 25, 16, 33, 0));
    const activeConversation = conversation({
      createdAt: new Date(2026, 3, 23, 12, 6, 0).toISOString(),
    });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    const row = within(screen.getByTestId("agents-session-conversation-1"));
    expect(row.getByText("2d")).toHaveAttribute(
      "title",
      "Apr 23, 2026, 12:06 PM",
    );
    expect(row.queryByText(/12:06/)).not.toBeInTheDocument();
    expect(row.queryByText(/days ago/)).not.toBeInTheDocument();
  });

  it("uses PR metadata instead of the base branch and omits implied open state", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 3, 25, 19, 0, 0));
    const activeConversation = conversation({
      createdAt: new Date(2026, 3, 25, 10, 0, 0).toISOString(),
    });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: activeConversation.id,
        publicationPrNumber: 123,
        publicationPrStatus: "open",
        publicationPushStatus: "pushed",
      }),
    ]);

    renderSidebar([project({ baseBranch: "develop" })]);

    const row = within(screen.getByTestId("agents-session-conversation-1"));
    expect(row.getByText("PR #123")).toBeInTheDocument();
    expect(row.getByText("9h")).toBeInTheDocument();
    expect(screen.getByTestId("agents-ref-icon-conversation-1")).toHaveAttribute(
      "data-ref-kind",
      "pull-request",
    );
    expect(row.queryByText("develop")).not.toBeInTheDocument();
    expect(row.queryByText("open")).not.toBeInTheDocument();
  });

  it("shows branch metadata and meaningful publication state badges", () => {
    const activeConversation = conversation({ id: "conversation-merged" });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: activeConversation.id,
        baseRef: "feature/base",
        baseDisplayName: "feature/base",
        publicationPrNumber: 77,
        publicationPrStatus: "merged",
        publicationPushStatus: "pushed",
      }),
    ]);

    renderSidebar([project({ baseBranch: "main" })]);

    const row = within(screen.getByTestId("agents-session-conversation-merged"));
    expect(row.getByText("PR #77")).toBeInTheDocument();
    expect(row.getByText("merged")).toBeInTheDocument();
    expect(screen.getByTestId("agents-ref-icon-conversation-merged")).toHaveAttribute(
      "data-ref-kind",
      "pull-request",
    );
  });

  it("shows a base-ahead indicator only when staleBaseDetectedAt is persisted", () => {
    const staleConversation = conversation({ id: "conversation-stale-base" });
    const freshConversation = conversation({ id: "conversation-fresh-base" });
    conversationsByProject.set("project-1", {
      data: [staleConversation, freshConversation],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: staleConversation.id,
        staleBaseDetectedAt: "2026-08-06T15:00:00Z",
      }),
      workspace({ conversationId: freshConversation.id }),
    ]);

    renderSidebar([project({ baseBranch: "main" })]);

    expect(
      within(screen.getByTestId("agents-session-conversation-stale-base")).getByTestId(
        "agents-session-base-ahead-conversation-stale-base",
      ),
    ).toHaveAttribute(
      "aria-label",
      "Base branch has moved ahead — this workspace needs an update",
    );
    expect(
      within(
        screen.getByTestId("agents-session-conversation-fresh-base"),
      ).queryByTestId("agents-session-base-ahead-conversation-fresh-base"),
    ).not.toBeInTheDocument();
  });

  it("only shows a runtime label for running conversations", () => {
    const idleConversation = conversation({ id: "conversation-idle" });
    const runningConversation = conversation({
      id: "conversation-running",
      title: "Running agent",
    });
    const runningStoreKey = getAgentConversationStoreKey(runningConversation);
    conversationsByProject.set("project-1", {
      data: [runningConversation, idleConversation],
      total: 2,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useChatStore.setState({
      activeConversationIds: { [runningStoreKey]: runningConversation.id },
      agentStatus: { [runningStoreKey]: "running" },
    });

    renderSidebar();

    expect(screen.getByTestId("agents-session-conversation-running")).toHaveTextContent(
      "running"
    );
    expect(screen.queryByText("queued")).not.toBeInTheDocument();
    expect(screen.queryByText("done")).not.toBeInTheDocument();
    expect(screen.queryByText("blocked")).not.toBeInTheDocument();
  });

  it("uses the Review activity label for a running workspace Review", () => {
    const reviewingConversation = conversation({
      id: "conversation-reviewing",
      title: "Reviewing workspace",
    });
    const reviewingStoreKey = getAgentConversationStoreKey(reviewingConversation);
    conversationsByProject.set("project-1", {
      data: [reviewingConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useChatStore.setState({
      activeConversationIds: { [reviewingStoreKey]: reviewingConversation.id },
      agentStatus: { [reviewingStoreKey]: "generating" },
      agentActivityLabels: { [reviewingStoreKey]: "reviewing" },
    });

    renderSidebar();

    const row = screen.getByTestId("agents-session-conversation-reviewing");
    expect(row).toHaveTextContent("reviewing");
    expect(within(row).queryByText("running")).not.toBeInTheDocument();
  });

  it("shows reviewing ahead of blocked PR supervision for a running workspace Review", () => {
    const reviewingConversation = conversation({
      id: "conversation-reviewing-blocked",
      title: "Reviewing blocked workspace",
    });
    const reviewingStoreKey = getAgentConversationStoreKey(reviewingConversation);
    conversationsByProject.set("project-1", {
      data: [reviewingConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: reviewingConversation.id,
        publicationPrNumber: 556,
        publicationPushStatus: "pushed",
        prSupervisionStatus: "blocked",
      }),
    ]);
    useChatStore.setState({
      activeConversationIds: {
        [reviewingStoreKey]: reviewingConversation.id,
      },
      agentStatus: { [reviewingStoreKey]: "generating" },
      agentActivityLabels: { [reviewingStoreKey]: "reviewing" },
    });

    renderSidebar();

    const row = within(screen.getByTestId("agents-session-conversation-reviewing-blocked"));
    expect(row.getByText("PR #556")).toBeInTheDocument();
    expect(row.getByText("reviewing")).toBeInTheDocument();
    expect(row.queryByText("blocked")).not.toBeInTheDocument();
    expect(row.queryByText("running")).not.toBeInTheDocument();
  });

  it("uses fixing publication label instead of generic running text", () => {
    const fixingConversation = conversation({
      id: "conversation-fixing",
      title: "Repair workspace",
    });
    const fixingStoreKey = getAgentConversationStoreKey(fixingConversation);
    conversationsByProject.set("project-1", {
      data: [fixingConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: fixingConversation.id,
        publicationPushStatus: "needs_agent",
        prSupervisionStatus: "fixing",
      }),
    ]);
    useChatStore.setState({
      activeConversationIds: { [fixingStoreKey]: fixingConversation.id },
      agentStatus: { [fixingStoreKey]: "running" },
    });

    renderSidebar();

    const row = screen.getByTestId("agents-session-conversation-fixing");
    expect(row).toHaveTextContent("fixing");
    expect(within(row).queryByText("running")).not.toBeInTheDocument();
  });

  it("bounds project session lists to eight visible rows and virtualizes overflow", () => {
    virtuosoMockState.rangeByTestId.set("agents-sidebar-session-list-project-1", {
      startIndex: 0,
      endIndex: 7,
    });
    conversationsByProject.set("project-1", {
      data: Array.from({ length: 12 }, (_, index) =>
        conversation({
          id: `conversation-${index + 1}`,
          title: `Agent ${index + 1}`,
        })
      ),
      total: 12,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    const list = screen.getByTestId("agents-sidebar-session-list-project-1");
    expect(list).toHaveAttribute("data-count", "12");
    expect(list).toHaveAttribute("data-visible-end", "7");
    expect(list).toHaveStyle({ height: "368px", maxHeight: "368px" });
    expect(screen.getByTestId("agents-session-conversation-1")).toBeInTheDocument();
    expect(screen.getByTestId("agents-session-conversation-8")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-session-conversation-9")).not.toBeInTheDocument();
  });

  it("runs project sidebar polling only for the virtual visible rows", async () => {
    virtuosoMockState.rangeByTestId.set("agents-sidebar-session-list-project-1", {
      startIndex: 2,
      endIndex: 4,
    });
    conversationsByProject.set("project-1", {
      data: Array.from({ length: 12 }, (_, index) =>
        conversation({
          id: `conversation-visible-${index + 1}`,
          title: `Agent ${index + 1}`,
        })
      ),
      total: 12,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    await waitFor(() => {
      const runningCall = [...runningStatesHook.mock.calls]
        .reverse()
        .find(([, isVisible]) => isVisible);
      expect(
        runningCall?.[0].map((conversationArg: AgentConversation) => conversationArg.id)
      ).toEqual([
        "conversation-visible-3",
        "conversation-visible-4",
        "conversation-visible-5",
      ]);
    });

    const pollingCall = [...publicationPollingHook.mock.calls]
      .reverse()
      .find(([, isVisible]) => isVisible);
    expect(
      pollingCall?.[0].map((conversationArg: AgentConversation) => conversationArg.id)
    ).toEqual([
      "conversation-visible-3",
      "conversation-visible-4",
      "conversation-visible-5",
    ]);
    expect(Array.from((pollingCall?.[2] as Map<string, string>).keys())).toEqual([
      "conversation-visible-3",
      "conversation-visible-4",
      "conversation-visible-5",
    ]);
  });

  it("runs publication sidebar polling only for the virtual visible rows", async () => {
    useAgentSessionStore.setState({
      sidebarGroupBy: "publication",
    });
    virtuosoMockState.rangeByTestId.set(
      "agents-sidebar-session-list-publication-active",
      {
        startIndex: 1,
        endIndex: 3,
      }
    );
    conversationsByProject.set("project-1", {
      data: Array.from({ length: 10 }, (_, index) =>
        conversation({
          id: `conversation-publication-visible-${index + 1}`,
          title: `Agent ${index + 1}`,
        })
      ),
      total: 10,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    await waitFor(() => {
      const pollingCall = [...publicationPollingHook.mock.calls]
        .reverse()
        .find(([, isVisible]) => isVisible);
      expect(
        pollingCall?.[0].map((conversationArg: AgentConversation) => conversationArg.id)
      ).toEqual([
        "conversation-publication-visible-2",
        "conversation-publication-visible-3",
        "conversation-publication-visible-4",
      ]);
    });

    const runningCall = [...runningStatesHook.mock.calls]
      .reverse()
      .find(([, isVisible]) => isVisible);
    expect(
      runningCall?.[0].map((conversationArg: AgentConversation) => conversationArg.id)
    ).toEqual([
      "conversation-publication-visible-2",
      "conversation-publication-visible-3",
      "conversation-publication-visible-4",
    ]);
  });

  it("measures the rendered session row height for list viewport sizing", async () => {
    const rectSpy = mockMeasuredSidebarRowHeight(44);
    conversationsByProject.set("project-1", {
      data: Array.from({ length: 12 }, (_, index) =>
        conversation({
          id: `conversation-${index + 1}`,
          title: `Agent ${index + 1}`,
        })
      ),
      total: 12,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    try {
      renderSidebar();

      await waitFor(() =>
        expect(screen.getByTestId("agents-sidebar-session-list-project-1")).toHaveStyle({
          height: "352px",
          maxHeight: "352px",
        })
      );
    } finally {
      rectSpy.mockRestore();
    }
  });

  it("fills available height and adapts page size for a single selected project", async () => {
    const rectSpy = mockMeasuredSidebarRowHeight(46);
    const focused = project({ id: "project-1", name: "alpha" });
    const idle = project({ id: "project-2", name: "beta" });
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true, "project-2": false },
      showAllProjects: false,
      sidebarProjectFilterIds: ["project-1"],
      projectSort: "latest",
    });
    virtuosoMockState.dimensionsByTestId.set("agents-sidebar-session-list-project-1", {
      clientHeight: 736,
      scrollHeight: 368,
    });
    conversationsByProject.set("project-1", {
      data: Array.from({ length: 8 }, (_, index) =>
        conversation({
          id: `conversation-${index + 1}`,
          title: `Agent ${index + 1}`,
        })
      ),
      total: 32,
      isLoading: false,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    try {
      renderSidebar([focused, idle]);

      await waitFor(() =>
        expect(screen.getByTestId("agents-sidebar-session-list-project-1")).toHaveStyle({
          height: "100%",
        })
      );
      expect(screen.getByTestId("agents-sidebar-session-list-project-1")).not.toHaveStyle({
        maxHeight: "368px",
      });
      await waitFor(() =>
        expect(projectConversationCalls).toContainEqual(
          expect.objectContaining({
            projectId: "project-1",
            pageSize: 18,
          })
        )
      );
    } finally {
      rectSpy.mockRestore();
    }
  });

  it("auto-fetches the next project page when the virtual list reaches the end", () => {
    const fetchNextPage = vi.fn().mockResolvedValue(undefined);
    conversationsByProject.set("project-1", {
      data: [conversation()],
      isLoading: false,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage,
    });

    renderSidebar();

    expect(screen.queryByTestId("agents-load-more-project-1")).not.toBeInTheDocument();

    virtuosoMockState.endReachedByTestId.get("agents-sidebar-session-list-project-1")?.();
    expect(fetchNextPage).toHaveBeenCalledTimes(1);
  });

  it("keeps project pagination available after a same-size group refresh", async () => {
    const user = userEvent.setup();
    const rows = Array.from({ length: 8 }, (_, index) =>
      conversation({
        id: `conversation-refresh-${index + 1}`,
        title: `Refresh row ${index + 1}`,
      })
    );
    const firstFetchNextPage = vi.fn().mockResolvedValue(undefined);
    const refreshedFetchNextPage = vi.fn().mockResolvedValue(undefined);

    function RefreshingSidebar() {
      const [refreshVersion, setRefreshVersion] = useState(0);
      conversationsByProject.set("project-1", {
        data: rows,
        total: 24,
        isLoading: false,
        hasNextPage: true,
        isFetchingNextPage: false,
        fetchNextPage:
          refreshVersion === 0 ? firstFetchNextPage : refreshedFetchNextPage,
      });

      return (
        <TooltipProvider delayDuration={0}>
          <button
            type="button"
            onClick={() => setRefreshVersion((version) => version + 1)}
          >
            Refresh group
          </button>
          <AgentsSidebar
            projects={[project()]}
            focusedProjectId="project-1"
            selectedConversationId={null}
            onFocusProject={vi.fn()}
            onSelectConversation={vi.fn()}
            onCreateAgent={vi.fn()}
            onCreateProject={vi.fn()}
            onArchiveProject={vi.fn()}
            onAutoRenameConversation={vi.fn()}
            onRenameConversation={vi.fn()}
            onArchiveConversation={vi.fn()}
            onRestoreConversation={vi.fn()}
            onForkConversation={vi.fn()}
            showArchived={false}
            onShowArchivedChange={vi.fn()}
          />
        </TooltipProvider>
      );
    }

    render(<RefreshingSidebar />);

    virtuosoMockState.endReachedByTestId.get("agents-sidebar-session-list-project-1")?.();
    expect(firstFetchNextPage).toHaveBeenCalledTimes(1);
    await waitForAnimationFrame();
    await new Promise<void>((resolve) => window.setTimeout(resolve, 0));

    await user.click(screen.getByRole("button", { name: "Refresh group" }));
    virtuosoMockState.endReachedByTestId.get("agents-sidebar-session-list-project-1")?.();

    expect(refreshedFetchNextPage).toHaveBeenCalledTimes(1);
  });

  it("auto-fetches the next project page when the user scrolls the virtual list to the bottom", async () => {
    const fetchNextPage = vi.fn().mockResolvedValue(undefined);
    const paginationProject = project({
      id: "project-pagination",
      name: "pagination",
    });
    virtuosoMockState.dimensionsByTestId.set(
      "agents-sidebar-session-list-project-pagination",
      {
        clientHeight: 368,
        scrollHeight: 736,
      }
    );
    conversationsByProject.set("project-pagination", {
      data: Array.from({ length: 8 }, (_, index) =>
        conversation({
          id: `conversation-${index + 1}`,
          title: `Agent ${index + 1}`,
          projectId: "project-pagination",
          contextId: "project-pagination",
        })
      ),
      total: 212,
      isLoading: false,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage,
    });

    renderSidebar([paginationProject], { focusedProjectId: "project-pagination" });

    const list = screen.getByTestId(
      "agents-sidebar-session-list-project-pagination"
    );
    list.scrollTop = 368;
    fireEvent.scroll(list);

    await waitFor(() => expect(fetchNextPage).toHaveBeenCalledTimes(1));
  });

  it("auto-fills an exact eight-row project page when more pages exist but no scrollbar is present", async () => {
    const fetchNextPage = vi.fn().mockResolvedValue(undefined);
    virtuosoMockState.dimensionsByTestId.set("agents-sidebar-session-list-project-1", {
      clientHeight: 368,
      scrollHeight: 368,
    });
    conversationsByProject.set("project-1", {
      data: Array.from({ length: 8 }, (_, index) =>
        conversation({
          id: `conversation-${index + 1}`,
          title: `Agent ${index + 1}`,
        })
      ),
      total: 12,
      isLoading: false,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage,
    });

    renderSidebar();

    await waitFor(() => expect(fetchNextPage).toHaveBeenCalledTimes(1));
  });

  it("remembers project scroll when switching open project groups", async () => {
    const user = userEvent.setup();
    const first = project({ id: "project-1", name: "alpha" });
    const second = project({ id: "project-2", name: "beta" });
    virtuosoMockState.resetScrollAfterMountWithoutStateByTestId.add(
      "agents-sidebar-session-list-project-1"
    );
    conversationsByProject.set("project-1", {
      data: Array.from({ length: 12 }, (_, index) =>
        conversation({
          id: `conversation-project-a-${index + 1}`,
          title: `Project A row ${index + 1}`,
        })
      ),
      total: 12,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [
        conversation({
          id: "conversation-project-b",
          title: "Project B row",
          projectId: "project-2",
          contextId: "project-2",
        }),
      ],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([first, second]);

    const projectAList = screen.getByTestId("agents-sidebar-session-list-project-1");
    projectAList.scrollTop = 184;
    fireEvent.scroll(projectAList);

    await user.click(screen.getByTestId("agents-project-row-project-2"));
    expect(
      screen.queryByTestId("agents-sidebar-session-list-project-1")
    ).not.toBeInTheDocument();

    await user.click(screen.getByTestId("agents-project-row-project-1"));
    await waitForAnimationFrame();
    await waitForAnimationFrame();

    expect(screen.getByTestId("agents-sidebar-session-list-project-1").scrollTop).toBe(
      184
    );
  });

  it("restores a saved scroll state only once for a scroll key across data refreshes", () => {
    const rows = Array.from({ length: 12 }, (_, index) =>
      conversation({
        id: `conversation-restore-${index + 1}`,
        title: `Restore row ${index + 1}`,
      })
    );
    conversationsByProject.set("project-1", {
      data: rows,
      total: rows.length,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    const firstRender = renderSidebar();
    const firstList = screen.getByTestId("agents-sidebar-session-list-project-1");
    firstList.scrollTop = 184;
    fireEvent.scroll(firstList);
    firstRender.unmount();
    virtuosoMockState.dimensionsByTestId.set("agents-sidebar-session-list-project-1", {
      clientHeight: 368,
      scrollHeight: 736,
    });

    const refreshedRows = [
      ...rows,
      conversation({ id: "conversation-restore-13", title: "Restore row 13" }),
    ];
    const restoredRender = renderSidebar();
    conversationsByProject.set("project-1", {
      data: refreshedRows,
      total: refreshedRows.length,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    restoredRender.rerender(
      <TooltipProvider delayDuration={0}>
        <AgentsSidebar {...buildSidebarProps()} />
      </TooltipProvider>
    );

    const testId = "agents-sidebar-session-list-project-1";
    expect(virtuosoMockState.restoreStateByTestIdAndCount.get(`${testId}:12`)).toBe(
      virtuosoMockState.restoreStateByTestIdAndCount.get(`${testId}:13`)
    );
    expect(virtuosoMockState.scrollToCallsByTestId.get(testId)).toBe(1);
  });

  it("restores the latest saved scroll position after a hide and show cycle", async () => {
    const hideProject = project({ id: "project-hide", name: "hide-show" });
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-hide": true },
    });
    const rows = Array.from({ length: 12 }, (_, index) =>
      conversation({
        id: `conversation-hide-${index + 1}`,
        title: `Hide row ${index + 1}`,
        projectId: "project-hide",
        contextId: "project-hide",
      })
    );
    conversationsByProject.set("project-hide", {
      data: rows,
      total: rows.length,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    virtuosoMockState.dimensionsByTestId.set(
      "agents-sidebar-session-list-project-hide",
      { clientHeight: 368, scrollHeight: 736 }
    );

    const view = renderSidebar([hideProject], { isVisible: true });
    const list = screen.getByTestId("agents-sidebar-session-list-project-hide");
    list.scrollTop = 184;
    fireEvent.scroll(list);
    await waitForAnimationFrame();

    view.rerender(
      <TooltipProvider delayDuration={0}>
        <AgentsSidebar {...buildSidebarProps([hideProject], { isVisible: false })} />
      </TooltipProvider>
    );
    expect(
      screen.queryByTestId("agents-sidebar-session-list-project-hide")
    ).not.toBeInTheDocument();

    view.rerender(
      <TooltipProvider delayDuration={0}>
        <AgentsSidebar {...buildSidebarProps([hideProject], { isVisible: true })} />
      </TooltipProvider>
    );
    await waitForAnimationFrame();

    expect(
      screen.getByTestId("agents-sidebar-session-list-project-hide").scrollTop
    ).toBe(184);
  });

  it("refetches the previously loaded project page depth when returning to a group", async () => {
    const user = userEvent.setup();
    const first = project({ id: "project-depth-1", name: "alpha" });
    const second = project({ id: "project-depth-2", name: "beta" });
    virtuosoMockState.asyncGetStateByTestId.add(
      "agents-sidebar-session-list-project-depth-1"
    );
    const projectARows = Array.from({ length: 16 }, (_, index) =>
      conversation({
        id: `conversation-project-a-page-${index + 1}`,
        title: `Project A row ${index + 1}`,
        projectId: "project-depth-1",
        contextId: "project-depth-1",
      })
    );
    const fetchProjectANextPage = vi.fn();

    function StatefulSidebar() {
      const [projectAPageCount, setProjectAPageCount] = useState(2);
      const [focusedProjectId, setFocusedProjectId] = useState("project-depth-1");
      fetchProjectANextPage.mockImplementation(async () => {
        setProjectAPageCount(2);
      });
      conversationsByProject.set("project-depth-1", {
        data: projectARows.slice(0, projectAPageCount * 8),
        total: projectARows.length,
        isLoading: false,
        hasNextPage: projectAPageCount < 2,
        isFetchingNextPage: false,
        fetchNextPage: fetchProjectANextPage,
      });
      conversationsByProject.set("project-depth-2", {
        data: [
          conversation({
            id: "conversation-project-b-page-depth",
            title: "Project B row",
            projectId: "project-depth-2",
            contextId: "project-depth-2",
          }),
        ],
        total: 1,
        isLoading: false,
        hasNextPage: false,
        isFetchingNextPage: false,
        fetchNextPage: vi.fn(),
      });

      return (
        <TooltipProvider delayDuration={0}>
          <AgentsSidebar
            projects={[first, second]}
            focusedProjectId={focusedProjectId}
            selectedConversationId={null}
            onFocusProject={(projectId) => {
              setFocusedProjectId(projectId);
              if (projectId === "project-depth-2") {
                setProjectAPageCount(1);
              }
            }}
            onSelectConversation={vi.fn()}
            onCreateAgent={vi.fn()}
            onCreateProject={vi.fn()}
            onArchiveProject={vi.fn()}
            onAutoRenameConversation={vi.fn()}
            onRenameConversation={vi.fn()}
            onArchiveConversation={vi.fn()}
            onRestoreConversation={vi.fn()}
            onForkConversation={vi.fn()}
            showArchived={false}
            onShowArchivedChange={vi.fn()}
          />
        </TooltipProvider>
      );
    }

    render(<StatefulSidebar />);

    expect(
      screen.getByTestId("agents-session-conversation-project-a-page-16")
    ).toBeInTheDocument();
    const projectAList = screen.getByTestId(
      "agents-sidebar-session-list-project-depth-1"
    );
    projectAList.scrollTop = 322;
    fireEvent.scroll(projectAList);

    await user.click(screen.getByTestId("agents-project-row-project-depth-2"));
    expect(
      screen.queryByTestId("agents-sidebar-session-list-project-depth-1")
    ).not.toBeInTheDocument();
    await waitFor(() =>
      expect(projectConversationCalls).toContainEqual(
        expect.objectContaining({
          projectId: "project-depth-1",
          minimumRowCount: 16,
        })
      )
    );

    await user.click(screen.getByTestId("agents-project-row-project-depth-1"));

    await waitFor(() => expect(fetchProjectANextPage).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(
        screen.getByTestId("agents-session-conversation-project-a-page-16")
      ).toBeInTheDocument()
    );
    expect(
      screen.getByTestId("agents-sidebar-session-list-project-depth-1").scrollTop
    ).toBe(322);
  });

  it("keeps remembered project page depth when selected fallback priority changes", async () => {
    const user = userEvent.setup();
    const depthProject = project({ id: "project-priority-depth", name: "depth" });
    const projectRows = Array.from({ length: 16 }, (_, index) =>
      conversation({
        id: `conversation-priority-page-${index + 1}`,
        title: `Priority page row ${index + 1}`,
        projectId: depthProject.id,
        contextId: depthProject.id,
      })
    );
    const selectedFallback = conversation({
      id: "conversation-priority-selected",
      title: "Selected fallback",
      projectId: depthProject.id,
      contextId: depthProject.id,
      createdAt: "2026-04-22T13:00:00Z",
    });
    conversationsByProject.set(depthProject.id, {
      data: projectRows,
      total: 24,
      isLoading: false,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    function StatefulSidebar() {
      const [selectedConversation, setSelectedConversation] =
        useState<AgentConversation | null>(null);

      return (
        <TooltipProvider delayDuration={0}>
          <button
            type="button"
            onClick={() => setSelectedConversation(selectedFallback)}
          >
            Select fallback
          </button>
          <AgentsSidebar
            projects={[depthProject]}
            focusedProjectId={depthProject.id}
            selectedConversationId={selectedConversation?.id ?? null}
            pinnedConversation={selectedConversation}
            onFocusProject={vi.fn()}
            onSelectConversation={vi.fn()}
            onCreateAgent={vi.fn()}
            onCreateProject={vi.fn()}
            onArchiveProject={vi.fn()}
            onAutoRenameConversation={vi.fn()}
            onRenameConversation={vi.fn()}
            onArchiveConversation={vi.fn()}
            onRestoreConversation={vi.fn()}
            onForkConversation={vi.fn()}
            showArchived={false}
            onShowArchivedChange={vi.fn()}
          />
        </TooltipProvider>
      );
    }

    render(<StatefulSidebar />);

    const projectList = screen.getByTestId(
      "agents-sidebar-session-list-project-priority-depth"
    );
    projectList.scrollTop = 322;
    fireEvent.scroll(projectList);
    await waitFor(() =>
      expect(projectConversationCalls).toContainEqual(
        expect.objectContaining({
          projectId: depthProject.id,
          minimumRowCount: 16,
        })
      )
    );

    projectConversationCalls.length = 0;
    await user.click(screen.getByRole("button", { name: "Select fallback" }));

    await waitFor(() =>
      expect(
        projectConversationCalls
          .filter((call) => call.projectId === depthProject.id)
          .at(-1)
      ).toEqual(
        expect.objectContaining({
          projectId: depthProject.id,
          minimumRowCount: 16,
          pinnedConversationIds: [],
          priorityConversationIds: [selectedFallback.id],
        })
      )
    );
  });

  it("skips the No project pseudo-group when selected fallback priority changes", async () => {
    const user = userEvent.setup();
    const realProject = project({ id: "project-fallback-real", name: "Real project" });
    const selectedFallback = conversation({
      id: "conversation-fallback-real",
      title: "Real project fallback",
      projectId: realProject.id,
      contextId: realProject.id,
    });
    conversationsByProject.set(realProject.id, {
      data: [selectedFallback],
      isLoading: false,
      total: 1,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("__no_project__", {
      data: [conversation({ id: "standalone-fallback-decoy", projectId: null })],
      isLoading: false,
      total: 1,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    function StatefulSidebar() {
      const [selectedConversation, setSelectedConversation] =
        useState<AgentConversation | null>(null);

      return (
        <TooltipProvider delayDuration={0}>
          <button
            type="button"
            onClick={() => setSelectedConversation(selectedFallback)}
          >
            Change fallback priority
          </button>
          <AgentsSidebar
            projects={[realProject]}
            focusedProjectId={realProject.id}
            selectedConversationId={selectedConversation?.id ?? null}
            pinnedConversation={selectedConversation}
            onFocusProject={vi.fn()}
            onSelectConversation={vi.fn()}
            onCreateAgent={vi.fn()}
            onCreateProject={vi.fn()}
            onArchiveProject={vi.fn()}
            onAutoRenameConversation={vi.fn()}
            onRenameConversation={vi.fn()}
            onArchiveConversation={vi.fn()}
            onRestoreConversation={vi.fn()}
            onForkConversation={vi.fn()}
            showArchived={false}
            onShowArchivedChange={vi.fn()}
          />
        </TooltipProvider>
      );
    }

    render(<StatefulSidebar />);
    projectConversationCalls.length = 0;
    await user.click(
      screen.getByRole("button", { name: "Change fallback priority" })
    );

    await waitFor(() =>
      expect(projectConversationCalls).toContainEqual(
        expect.objectContaining({
          projectId: realProject.id,
          priorityConversationIds: [selectedFallback.id],
        })
      )
    );
    expect(projectConversationCalls).not.toContainEqual(
      expect.objectContaining({
        projectId: "__no_project__",
        priorityConversationIds: [selectedFallback.id],
      })
    );
  });

  it("keeps a loaded paginated row in place after selecting it", async () => {
    const user = userEvent.setup();
    const newer = conversation({
      id: "conversation-newer",
      title: "Newer loaded agent",
      createdAt: "2026-04-22T12:00:00Z",
    });
    const older = conversation({
      id: "conversation-older",
      title: "Older loaded agent",
      createdAt: "2026-04-22T10:00:00Z",
    });
    conversationsByProject.set("project-1", {
      data: [newer, older],
      total: 8,
      isLoading: false,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    function StatefulSidebar() {
      const [selectedConversationId, setSelectedConversationId] = useState<string | null>(
        null
      );
      const [pinnedConversation, setPinnedConversation] =
        useState<AgentConversation | null>(null);

      return (
        <TooltipProvider delayDuration={0}>
          <AgentsSidebar
            projects={[project()]}
            focusedProjectId="project-1"
            selectedConversationId={selectedConversationId}
            pinnedConversation={pinnedConversation}
            onFocusProject={vi.fn()}
            onSelectConversation={(_projectId, selectedConversation) => {
              setSelectedConversationId(selectedConversation.id);
              setPinnedConversation(selectedConversation);
            }}
            onCreateAgent={vi.fn()}
            onCreateProject={vi.fn()}
            onArchiveProject={vi.fn()}
            onAutoRenameConversation={vi.fn()}
            onRenameConversation={vi.fn()}
            onArchiveConversation={vi.fn()}
            onRestoreConversation={vi.fn()}
            onForkConversation={vi.fn()}
            showArchived={false}
            onShowArchivedChange={vi.fn()}
          />
        </TooltipProvider>
      );
    }

    render(<StatefulSidebar />);

    expect(getSessionRowOrder()).toEqual([
      "agents-session-conversation-newer",
      "agents-session-conversation-older",
    ]);

    await user.click(
      within(screen.getByTestId("agents-session-conversation-older")).getByRole(
        "button",
        { name: /Older loaded agent/ }
      )
    );

    expect(
      within(screen.getByTestId("agents-session-conversation-older")).getByRole(
        "button",
        { name: /Older loaded agent/ }
      )
    ).toHaveAttribute("aria-current", "true");
    expect(getSessionRowOrder()).toEqual([
      "agents-session-conversation-newer",
      "agents-session-conversation-older",
    ]);
    expect(projectConversationCalls.at(-1)?.pinnedConversationIds).toEqual([]);
  });

  it("shows the backend total session count rather than the loaded page size", () => {
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1" }), conversation({ id: "conversation-2" })],
      total: 11,
      isLoading: false,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    expect(screen.getByText("11")).toBeInTheDocument();
  });

  it("uses design-system-owned active project and session highlight state", () => {
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-active", title: "Selected run" })],
      total: 4,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      focusedProjectId: "project-1",
      selectedConversationId: "conversation-active",
    });

    const projectRow = screen.getByTestId("agents-project-row-project-1");
    expect(projectRow).toHaveClass("agents-project-row");
    expect(projectRow).toHaveAttribute("aria-current", "true");
    expect(projectRow.getAttribute("style") ?? "").not.toContain("rgba(255");
    expect(within(projectRow).getByText("4")).toHaveClass("agents-project-count");

    const sessionRow = within(screen.getByTestId("agents-session-conversation-active"))
      .getByRole("button", { name: /Selected run/ });
    expect(sessionRow).toHaveClass("agents-session-row");
    expect(sessionRow).toHaveAttribute("aria-current", "true");
    expect(sessionRow.getAttribute("style") ?? "").not.toContain("rgba(255");
    expect(within(sessionRow).getByText("master").closest(".agents-session-meta")).toBeTruthy();
  });

  it("renders archived visibility inside Filters and toggles archived sessions", async () => {
    const user = userEvent.setup();
    const onShowArchivedChange = vi.fn();
    archivedConversationCounts.set("project-1", 4);
    conversationsByProject.set("project-1", {
      data: [conversation()],
      total: 6,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onShowArchivedChange });

    expect(screen.queryByTestId("agents-show-archived-pill")).not.toBeInTheDocument();
    await user.click(screen.getByTestId("agents-filters-trigger"));

    const archivedFilter = screen.getByTestId("agents-filter-archived");
    expect(archivedFilter).toHaveTextContent("Archived");
    expect(archivedFilter).toHaveTextContent("4");
    expect(archivedCountCalls.at(-1)).toEqual(["project-1"]);

    expect(archivedFilter).toHaveAttribute("role", "checkbox");
    await user.click(archivedFilter);
    expect(onShowArchivedChange).toHaveBeenCalledWith(true);
  });

  it("keeps selected archived filter styling neutral inside the filters popover", async () => {
    const user = userEvent.setup();
    archivedConversationCounts.set("project-1", 4);

    renderSidebar([project()], { showArchived: true });

    await user.click(screen.getByTestId("agents-filters-trigger"));
    expect(
      screen.getByTestId("agents-filter-popover").getAttribute("style")
    ).toContain("background-color: var(--bg-elevated)");
  });

  it("suppresses empty project headers in archived-only results", () => {
    const alpha = project({ id: "project-1", name: "alpha" });
    const beta = project({ id: "project-2", name: "beta" });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-active" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [],
      total: 0,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([alpha, beta], { showArchived: true });

    expect(screen.queryByTestId("agents-project-project-1")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-project-project-2")).not.toBeInTheDocument();
    expect(useAgentSessionStore.getState().showEmptyProjectGroups).toBe(true);
  });

  it("renders the static v27 Recent block above the add-project action", () => {
    conversationsByProject.set("project-1", {
      data: [conversation()],
      total: 6,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    const recent = screen.getByTestId("agents-static-recent");
    // Static recent block is rendered but hidden ("Coming soon") via aria-hidden + display:none
    expect(recent).toHaveAttribute("aria-hidden", "true");
    expect(within(recent).getByText("Recent", { selector: "span" })).toBeInTheDocument();
    expect(
      within(recent).getByRole("button", { name: "View all", hidden: true }),
    ).toBeInTheDocument();
    expect(within(recent).getByText("Add ranking to reefbot homepage")).toBeInTheDocument();
    expect(within(recent).getByText("Tighten kanban drag handles")).toBeInTheDocument();
    expect(screen.getByTestId("agents-add-project")).toBeInTheDocument();
  });

  it("shows empty projects by default in the v27 tree", () => {
    conversationsByProject.set("project-1", {
      data: [],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    expect(screen.getByTestId("agents-project-project-1")).toBeInTheDocument();
    expect(screen.queryByText("No chats yet.")).not.toBeInTheDocument();
    expect(screen.queryByText("Start")).not.toBeInTheDocument();
  });

  it("keeps a newly selected empty project visible and ordered after active projects", async () => {
    const user = userEvent.setup();
    const alpha = project({ id: "project-1", name: "alpha" });
    const beta = project({ id: "project-2", name: "beta" });
    const gamma = project({ id: "project-3", name: "gamma" });
    useAgentSessionStore.setState({
      showAllProjects: false,
      sidebarProjectFilterIds: ["project-1"],
    });
    latestProjectOrderData.current = ["project-1"];
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-active", title: "Active work" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [],
      total: 0,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([alpha, beta, gamma]);

    await user.click(screen.getByTestId("agents-filters-trigger"));
    await user.click(screen.getByTestId("agents-filter-projects-section-trigger"));
    await user.click(screen.getByTestId("agents-filter-project-project-2"));

    expect(screen.getByTestId("agents-filter-projects-section-trigger")).toHaveTextContent(
      "2/3",
    );
    expect(useAgentSessionStore.getState().sidebarProjectFilterIds).toEqual([
      "project-1",
      "project-2",
    ]);
    expect(getProjectRowOrder()).toEqual([
      "agents-project-project-1",
      "agents-project-project-2",
    ]);
    expect(screen.queryByTestId("agents-project-project-3")).not.toBeInTheDocument();
    expect(screen.queryByText("No chats yet.")).not.toBeInTheDocument();
  });

  it("toggles empty project headers without changing project selection", async () => {
    const user = userEvent.setup();
    const alpha = project({ id: "project-1", name: "alpha" });
    const beta = project({ id: "project-2", name: "beta" });
    useAgentSessionStore.setState({
      showAllProjects: false,
      sidebarProjectFilterIds: ["project-1", "project-2"],
    });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-active", title: "Active work" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [],
      total: 0,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([alpha, beta]);

    await user.click(screen.getByTestId("agents-filters-trigger"));
    const emptyGroupsFilter = screen.getByTestId("agents-filter-empty-project-groups");
    expect(emptyGroupsFilter).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("agents-project-project-2")).toBeInTheDocument();

    await user.click(emptyGroupsFilter);
    expect(useAgentSessionStore.getState()).toMatchObject({
      showEmptyProjectGroups: false,
      showAllProjects: false,
      sidebarProjectFilterIds: ["project-1", "project-2"],
    });
    expect(screen.queryByTestId("agents-project-project-2")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-project-project-1")).toBeInTheDocument();

    await user.click(emptyGroupsFilter);
    expect(screen.getByTestId("agents-project-project-2")).toBeInTheDocument();
  });

  it("keeps a loading project header until its empty result settles", async () => {
    const user = userEvent.setup();
    const loadingProject = project({ id: "project-1", name: "alpha" });
    conversationsByProject.set("project-1", {
      data: [],
      total: 0,
      isLoading: true,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([loadingProject]);

    await user.click(screen.getByTestId("agents-filters-trigger"));
    await user.click(screen.getByTestId("agents-filter-empty-project-groups"));
    expect(screen.getByTestId("agents-project-project-1")).toBeInTheDocument();

    conversationsByProject.set("project-1", {
      data: [],
      total: 0,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useAgentSessionStore.getState().setProjectSort("az");

    await waitFor(() =>
      expect(screen.queryByTestId("agents-project-project-1")).not.toBeInTheDocument(),
    );
  });

  it("hydrates every project row when the show-all-projects filter is enabled", () => {
    const focused = project({ id: "project-1", name: "alpha" });
    const idle = project({ id: "project-2", name: "beta" });
    const anotherIdle = project({ id: "project-3", name: "gamma" });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [conversation({ id: "conversation-2", projectId: "project-2", contextId: "project-2" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([focused, idle, anotherIdle]);

    expect(projectConversationCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          projectId: "project-1",
          options: expect.objectContaining({ enabled: true }),
        }),
        expect.objectContaining({
          projectId: "project-2",
          options: expect.objectContaining({ enabled: true }),
        }),
        expect.objectContaining({
          projectId: "project-3",
          options: expect.objectContaining({ enabled: true }),
        }),
      ])
    );
    expect(archivedCountCalls.at(-1)).toEqual(["project-1", "project-2", "project-3"]);
  });

  it("collapses the previously expanded project when another project opens", () => {
    const first = project({ id: "project-1", name: "alpha" });
    const second = project({ id: "project-2", name: "beta" });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1", title: "First run" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [
        conversation({
          id: "conversation-2",
          title: "Second run",
          projectId: "project-2",
          contextId: "project-2",
        }),
      ],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([first, second]);

    expect(screen.getByTestId("agents-session-conversation-1")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-session-conversation-2")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-project-row-project-2"));

    expect(screen.queryByTestId("agents-session-conversation-1")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-session-conversation-2")).toBeInTheDocument();
    expect(useAgentSessionStore.getState().expandedProjectIds).toMatchObject({
      "project-1": false,
      "project-2": true,
    });
  });

  it("keeps the expanded project capped while all projects are visible", () => {
    const first = project({ id: "project-1", name: "alpha" });
    const second = project({ id: "project-2", name: "beta" });
    const third = project({ id: "project-3", name: "gamma" });
    useAgentSessionStore.setState({
      expandedProjectIds: {
        "project-1": true,
        "project-2": false,
        "project-3": false,
      },
      showAllProjects: true,
      projectSort: "latest",
    });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1", title: "First run" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [
        conversation({
          id: "conversation-2",
          title: "Second run",
          projectId: "project-2",
          contextId: "project-2",
        }),
      ],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-3", {
      data: [
        conversation({
          id: "conversation-3",
          title: "Third run",
          projectId: "project-3",
          contextId: "project-3",
        }),
      ],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([first, second, third]);

    const expandedProject = screen.getByTestId("agents-project-project-1");
    const collapsedProject = screen.getByTestId("agents-project-project-2");
    const projectList = expandedProject.parentElement;

    expect(projectList).toHaveClass("flex-1", "overflow-y-auto");
    expect(projectList).not.toHaveClass("overflow-hidden");
    expect(expandedProject).not.toHaveClass("flex-1", "min-h-0");
    expect(collapsedProject).not.toHaveClass("flex-1");
    expect(screen.getByTestId("agents-sidebar-session-list-project-1")).toHaveStyle({
      height: "46px",
      maxHeight: "368px",
    });
    expect(screen.queryByTestId("agents-sidebar-session-list-project-2")).not.toBeInTheDocument();
  });

  it("lets the expanded project use remaining sidebar height when multiple projects are filtered", () => {
    const first = project({ id: "project-1", name: "alpha" });
    const second = project({ id: "project-2", name: "beta" });
    const third = project({ id: "project-3", name: "gamma" });
    useAgentSessionStore.setState({
      expandedProjectIds: {
        "project-1": true,
        "project-2": false,
        "project-3": false,
      },
      showAllProjects: false,
      sidebarProjectFilterIds: ["project-1", "project-2"],
      projectSort: "latest",
    });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1", title: "First run" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [
        conversation({
          id: "conversation-2",
          title: "Second run",
          projectId: "project-2",
          contextId: "project-2",
        }),
      ],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-3", {
      data: [
        conversation({
          id: "conversation-3",
          title: "Third run",
          projectId: "project-3",
          contextId: "project-3",
        }),
      ],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([first, second, third]);

    const expandedProject = screen.getByTestId("agents-project-project-1");
    const collapsedProject = screen.getByTestId("agents-project-project-2");
    const projectList = expandedProject.parentElement;

    expect(projectList).toHaveClass("flex", "min-h-0", "flex-1", "overflow-hidden");
    expect(expandedProject).toHaveClass("flex-1", "min-h-0");
    expect(collapsedProject).not.toHaveClass("flex-1");
    expect(screen.getByTestId("agents-sidebar-session-list-project-1")).toHaveStyle({
      height: "100%",
    });
    expect(screen.queryByTestId("agents-sidebar-session-list-project-2")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-project-project-3")).not.toBeInTheDocument();
  });

  it("fills a single visible project when expansion state is unset and focus is elsewhere", () => {
    const visibleProject = project({ id: "project-1", name: "alpha" });
    const focusedElsewhere = project({ id: "project-2", name: "beta" });
    useAgentSessionStore.setState({
      expandedProjectIds: {},
      showAllProjects: false,
      sidebarProjectFilterIds: ["project-1"],
      projectSort: "latest",
    });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1", title: "First run" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [
        conversation({
          id: "conversation-2",
          title: "Second run",
          projectId: "project-2",
          contextId: "project-2",
        }),
      ],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([visibleProject, focusedElsewhere], {
      focusedProjectId: "project-2",
    });

    const visibleProjectGroup = screen.getByTestId("agents-project-project-1");
    const projectList = visibleProjectGroup.parentElement;

    expect(projectList).toHaveClass("flex", "min-h-0", "flex-1", "overflow-hidden");
    expect(visibleProjectGroup).toHaveClass("flex-1", "min-h-0");
    expect(screen.queryByTestId("agents-project-project-2")).not.toBeInTheDocument();
  });

  it("searches conversations on the backend across projects without matching project names", async () => {
    const focused = project({ id: "project-1", name: "alpha" });
    const idle = project({ id: "project-2", name: "beta" });
    conversationsByProject.set("project-1", {
      data: [],
      total: 0,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [
        conversation({
          id: "conversation-search",
          title: "Fix sidebar search",
          projectId: "project-2",
          contextId: "project-2",
        }),
      ],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([focused, idle]);

    fireEvent.click(screen.getByTestId("agents-search-toggle"));
    fireEvent.change(screen.getByTestId("agents-search-input"), {
      target: { value: "sidebar" },
    });

    await waitFor(() =>
      expect(projectConversationCalls).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            projectId: "project-2",
            options: expect.objectContaining({
              enabled: true,
              search: "sidebar",
            }),
          }),
        ])
      )
    );
    expect(screen.getByTestId("agents-session-conversation-search")).toHaveTextContent(
      "Fix sidebar search"
    );
    expect(screen.queryByTestId("agents-project-project-1")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-project-project-2")).toBeInTheDocument();
  });

  it("renders Group, Filters, Sort toolbar controls with grouping outside Filters", async () => {
    const user = userEvent.setup();
    conversationsByProject.set("project-1", {
      data: [],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    expect(screen.getByTestId("agents-filter-toolbar")).toHaveClass(
      "flex",
      "mb-2",
      "px-3",
    );
    expect(screen.getByTestId("agents-filter-toolbar").getAttribute("style")).toContain(
      "background-color: var(--bg-surface)",
    );
    expect(screen.getByTestId("agents-group-trigger")).toHaveTextContent("Project");
    expect(screen.getByTestId("agents-group-trigger")).toHaveAttribute(
      "aria-label",
      "Group conversations: Project"
    );
    expect(screen.getByTestId("agents-filters-trigger")).toHaveTextContent("Filters");
    expect(screen.getByTestId("agents-sort-trigger")).toHaveTextContent("Sort");
    expect(screen.getByTestId("agents-bulk-archive-trigger")).toHaveAccessibleName(
      "Bulk archive sessions"
    );
    expect(screen.getByTestId("agents-sort-trigger")).toHaveAttribute(
      "aria-label",
      "Sort projects: Latest"
    );
    expect(
      screen.getByTestId("agents-group-trigger").compareDocumentPosition(
        screen.getByTestId("agents-filters-trigger")
      )
    ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(
      screen.getByTestId("agents-filters-trigger").compareDocumentPosition(
        screen.getByTestId("agents-sort-trigger")
      )
    ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(
      screen.getByTestId("agents-sort-trigger").compareDocumentPosition(
        screen.getByTestId("agents-bulk-archive-trigger")
      )
    ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(screen.getByTestId("agents-bulk-archive-trigger")).toHaveClass("ml-auto");
    await user.hover(screen.getByTestId("agents-bulk-archive-trigger"));
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Bulk archive sessions");
    expect(screen.queryByTestId("agents-show-archived-pill")).not.toBeInTheDocument();

    await user.click(screen.getByTestId("agents-filters-trigger"));
    expect(screen.queryByTestId("agents-filter-group-by")).not.toBeInTheDocument();
    expect(screen.queryByRole("radio", { name: "Project" })).not.toBeInTheDocument();
    await user.keyboard("{Escape}");

    await user.click(screen.getByTestId("agents-group-trigger"));
    expect(screen.getByTestId("agents-group-popover")).toHaveTextContent("Project");
    expect(screen.getByTestId("agents-group-popover")).toHaveTextContent(
      "Publication state"
    );
    expect(screen.getByTestId("agents-group-popover")).toHaveTextContent("Automations");
    await user.click(screen.getByRole("radio", { name: "Publication state" }));
    await waitFor(() =>
      expect(screen.getByTestId("agents-sort-trigger")).toHaveAttribute(
        "aria-label",
        "Sort conversations: Latest"
      )
    );
    expect(screen.getByTestId("agents-group-trigger")).toHaveTextContent(
      "Publication state"
    );
    await user.click(screen.getByRole("radio", { name: "Automations" }));
    await waitFor(() =>
      expect(screen.getByTestId("agents-sort-trigger")).toHaveAttribute(
        "aria-label",
        "Sort automations: Latest"
      )
    );
    expect(screen.getByTestId("agents-group-trigger")).toHaveTextContent(
      "Automations"
    );
    expect(screen.getByTestId("agents-group-trigger")).toHaveAttribute(
      "aria-label",
      "Group conversations: Automations"
    );
    await user.click(screen.getByRole("radio", { name: "Inbox" }));
    await waitFor(() =>
      expect(screen.getByTestId("agents-group-trigger")).toHaveTextContent("Inbox")
    );

    await user.click(screen.getByTestId("agents-filters-trigger"));
    await user.click(
      screen.getByTestId("agents-filter-projects-section-trigger")
    );
    expect(screen.getByTestId("agents-filter-all-projects")).toHaveTextContent(
      "All projects"
    );

    await user.click(
      screen.getByTestId("agents-filter-publication-section-trigger")
    );
    expect(screen.getByTestId("agents-filter-publication-state-active")).toHaveTextContent(
      "Active"
    );
  });

  it("bulk archives open pull-request rows while leaving remote pull requests open", async () => {
    const user = userEvent.setup();
    const eligibleConversation = conversation({
      id: "conversation-bulk-eligible",
      title: "Eligible session",
    });
    const blockedConversation = conversation({
      id: "conversation-bulk-blocked",
      title: "Open PR session",
    });
    workspacesByProject.set("project-1", [
      workspace({ conversationId: eligibleConversation.id }),
      workspace({
        conversationId: blockedConversation.id,
        publicationPrNumber: 91,
        publicationPrStatus: "open",
      }),
    ]);
    conversationsByProject.set("project-1", {
      data: [eligibleConversation, blockedConversation],
      total: 2,
      isLoading: false,
    });
    let settleArchive: ((result: {
      archivedConversationIds: string[];
      failedConversationIds: string[];
      cleanupPendingConversationIds: string[];
      cleanupUnsafeConversationIds: string[];
    }) => void) | undefined;
    const onBulkArchiveConversations = vi.fn(
      () =>
        new Promise<{
          archivedConversationIds: string[];
          failedConversationIds: string[];
          cleanupPendingConversationIds: string[];
          cleanupUnsafeConversationIds: string[];
        }>((resolve) => {
          settleArchive = resolve;
        })
    );

    renderSidebar([project()], { onBulkArchiveConversations });

    await user.click(screen.getByTestId("agents-bulk-archive-trigger"));
    const eligibleCheckbox = screen.getByRole("checkbox", {
      name: "Select Eligible session for bulk archive",
    });
    const blockedCheckbox = screen.getByRole("checkbox", {
      name: "Select Open PR session for bulk archive",
    });
    expect(eligibleCheckbox).not.toBeChecked();
    expect(blockedCheckbox).not.toBeDisabled();
    expect(screen.getByRole("button", { name: "Archive selected" })).toBeDisabled();

    await user.click(eligibleCheckbox);
    await user.click(blockedCheckbox);
    expect(screen.getByText("2 selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Archive selected" })).toBeEnabled();

    const blockedRow = screen.getByTestId(`agents-session-${blockedConversation.id}`);
    await user.click(within(blockedRow).getByRole("button", { name: "Session actions" }));
    expect(screen.getByText("Archive session")).toBeInTheDocument();
    await user.keyboard("{Escape}");

    await user.click(screen.getByRole("button", { name: "Archive selected" }));
    expect(screen.getByRole("heading", { name: "Archive selected sessions?" })).toBeVisible();
    expect(screen.getByText(/Remote pull requests remain open/)).toBeVisible();
    expect(screen.getByText(/including uncommitted changes/)).toBeVisible();
    expect(onBulkArchiveConversations).not.toHaveBeenCalled();
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Archive selected",
      })
    );

    expect(onBulkArchiveConversations).toHaveBeenCalledWith([
      {
        conversation: eligibleConversation,
        workspace: expect.objectContaining({ conversationId: eligibleConversation.id }),
      },
      {
        conversation: blockedConversation,
        workspace: expect.objectContaining({ conversationId: blockedConversation.id }),
      },
    ]);
    expect(screen.getByRole("button", { name: "Archiving selected..." })).toBeDisabled();

    settleArchive?.({
      archivedConversationIds: [eligibleConversation.id, blockedConversation.id],
      failedConversationIds: [],
      cleanupPendingConversationIds: [],
      cleanupUnsafeConversationIds: [],
    });
    await waitFor(() =>
      expect(screen.queryByText("Archive selected sessions?")).not.toBeInTheDocument()
    );
    expect(
      screen.queryByRole("checkbox", {
        name: "Select Eligible session for bulk archive",
      })
    ).not.toBeInTheDocument();
  });

  it("retains only failed rows after a partial bulk archive result", async () => {
    const user = userEvent.setup();
    const archivedConversation = conversation({
      id: "conversation-bulk-success",
      title: "Archived session",
    });
    const failedConversation = conversation({
      id: "conversation-bulk-failure",
      title: "Retry session",
    });
    conversationsByProject.set("project-1", {
      data: [archivedConversation, failedConversation],
      total: 2,
      isLoading: false,
    });
    const onBulkArchiveConversations = vi.fn().mockResolvedValue({
      archivedConversationIds: [archivedConversation.id],
      failedConversationIds: [failedConversation.id],
      cleanupPendingConversationIds: [],
      cleanupUnsafeConversationIds: [],
    });

    renderSidebar([project()], { onBulkArchiveConversations });
    await user.click(screen.getByTestId("agents-bulk-archive-trigger"));
    await user.click(
      screen.getByRole("checkbox", {
        name: "Select Archived session for bulk archive",
        hidden: true,
      })
    );
    await user.click(
      screen.getByRole("checkbox", {
        name: "Select Retry session for bulk archive",
        hidden: true,
      })
    );
    await user.click(screen.getByRole("button", { name: "Archive selected" }));
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", {
        name: "Archive selected",
      })
    );

    await waitFor(() => expect(screen.getByText("1 selected")).toBeInTheDocument());
    expect(
      screen.getByRole("checkbox", {
        name: "Select Archived session for bulk archive",
        hidden: true,
      })
    ).not.toBeChecked();
    expect(
      screen.getByRole("checkbox", {
        name: "Select Retry session for bulk archive",
        hidden: true,
      })
    ).toBeChecked();
    expect(screen.getByRole("heading", { name: "Archive selected sessions?" })).toBeVisible();
  });

  it("mutes every selected session without opening archive confirmation", async () => {
    const user = userEvent.setup();
    const first = conversation({ id: "conversation-bulk-mute-first", title: "First mute" });
    const second = conversation({ id: "conversation-bulk-mute-second", title: "Second mute" });
    conversationsByProject.set("project-1", {
      data: [first, second], total: 2, isLoading: false, hasNextPage: false, isFetchingNextPage: false,
    });
    const onBulkMuteConversations = vi.fn();
    renderSidebar([project()], { onBulkMuteConversations });

    await user.click(screen.getByTestId("agents-bulk-archive-trigger"));
    await user.click(screen.getByRole("checkbox", { name: "Select First mute for bulk archive" }));
    await user.click(screen.getByRole("checkbox", { name: "Select Second mute for bulk archive" }));
    await user.click(screen.getByRole("button", { name: "Mute" }));

    expect(onBulkMuteConversations).toHaveBeenCalledWith([first.id, second.id]);
    expect(screen.queryByRole("heading", { name: "Archive selected sessions?" })).not.toBeInTheDocument();
  });

  it("prunes a selected row when it leaves the loaded filter result", async () => {
    const user = userEvent.setup();
    const selectedConversation = conversation({
      id: "conversation-bulk-stale",
      title: "Stale selection",
    });
    const remainingConversation = conversation({
      id: "conversation-bulk-current",
      title: "Current selection",
    });
    conversationsByProject.set("project-1", {
      data: [selectedConversation, remainingConversation],
      total: 2,
      isLoading: false,
    });

    renderSidebar();
    await user.click(screen.getByTestId("agents-bulk-archive-trigger"));
    await user.click(
      screen.getByRole("checkbox", {
        name: "Select Stale selection for bulk archive",
      })
    );
    expect(screen.getByText("1 selected")).toBeInTheDocument();

    conversationsByProject.set("project-1", {
      data: [remainingConversation],
      total: 1,
      isLoading: false,
    });
    useAgentSessionStore.getState().setProjectSort("az");

    await waitFor(() => expect(screen.getByText("0 selected")).toBeInTheDocument());
    expect(
      screen.queryByRole("checkbox", {
        name: "Select Stale selection for bulk archive",
      })
    ).not.toBeInTheDocument();
  });

  it("keeps a selected row eligible when refreshed workspace data reports an open PR", async () => {
    const user = userEvent.setup();
    const selectedConversation = conversation({
      id: "conversation-bulk-open-pr-refresh",
      title: "PR changed session",
    });
    conversationsByProject.set("project-1", {
      data: [selectedConversation],
      total: 1,
      isLoading: false,
    });
    workspacesByProject.set("project-1", [
      workspace({ conversationId: selectedConversation.id }),
    ]);

    renderSidebar();
    await user.click(screen.getByTestId("agents-bulk-archive-trigger"));
    await user.click(
      screen.getByRole("checkbox", {
        name: "Select PR changed session for bulk archive",
      })
    );
    expect(screen.getByText("1 selected")).toBeInTheDocument();

    workspacesByProject.set("project-1", [
      workspace({
        conversationId: selectedConversation.id,
        publicationPrNumber: 92,
        publicationPrStatus: "open",
      }),
    ]);
    useAgentSessionStore.getState().setProjectSort("az");

    await waitFor(() => expect(screen.getByText("1 selected")).toBeInTheDocument());
    expect(
      screen.getByRole("checkbox", {
        name: "Select PR changed session for bulk archive",
      })
    ).not.toBeDisabled();
  });

  it("uses a soft wrapper border for sidebar search focus", async () => {
    const user = userEvent.setup();
    renderSidebar();

    await user.click(screen.getByTestId("agents-search-toggle"));
    const input = screen.getByTestId("agents-search-input");
    const searchFrame = input.parentElement as HTMLElement;

    fireEvent.focus(input);

    await waitFor(() =>
      expect(searchFrame.getAttribute("style")).toContain(
        "border-color: var(--accent-border)"
      )
    );

    fireEvent.blur(input);

    await waitFor(() =>
      expect(searchFrame.getAttribute("style")).toContain(
        "border-color: var(--overlay-weak)"
      )
    );
  });

  it("scopes project hydration until the show-all-projects filter is enabled", async () => {
    const user = userEvent.setup();
    const focused = project({ id: "project-1", name: "alpha" });
    const idle = project({ id: "project-2", name: "beta" });
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true, "project-2": false },
      showAllProjects: false,
      projectSort: "latest",
    });

    renderSidebar([focused, idle]);

    expect(
      projectConversationCalls.some((call) => call.projectId === "project-2")
    ).toBe(false);

    await user.click(screen.getByTestId("agents-filters-trigger"));
    await user.click(
      screen.getByTestId("agents-filter-projects-section-trigger")
    );
    await user.click(screen.getByTestId("agents-filter-all-projects"));

    await waitFor(() =>
      expect(useAgentSessionStore.getState().showAllProjects).toBe(true),
    );
    expect(
      projectConversationCalls.filter((call) => call.projectId === "project-2").at(-1)
        ?.options?.enabled,
    ).toBe(true);
  });

  it("switches from all projects to individual project filters inside the filter popover", async () => {
    const user = userEvent.setup();
    const alpha = project({ id: "project-1", name: "alpha" });
    const beta = project({ id: "project-2", name: "beta" });
    const gamma = project({ id: "project-3", name: "gamma" });

    renderSidebar([alpha, beta, gamma]);

    await user.click(screen.getByTestId("agents-filters-trigger"));
    await user.click(screen.getByTestId("agents-filter-projects-section-trigger"));
    await user.click(screen.getByTestId("agents-filter-project-project-2"));

    expect(useAgentSessionStore.getState()).toMatchObject({
      showAllProjects: false,
      sidebarProjectFilterIds: ["project-1", "project-3"],
    });

    await user.click(screen.getByTestId("agents-filter-project-project-3"));
    expect(useAgentSessionStore.getState().sidebarProjectFilterIds).toEqual([
      "project-1",
    ]);
  });

  it("hydrates individually selected project filters while all projects is off", () => {
    const focused = project({ id: "project-1", name: "alpha" });
    const selected = project({ id: "project-2", name: "beta" });
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true, "project-2": true },
      showAllProjects: false,
      sidebarProjectFilterIds: ["project-2"],
    });
    conversationsByProject.set("project-2", {
      data: [
        conversation({
          id: "conversation-filtered-project",
          title: "Filtered project",
          projectId: "project-2",
          contextId: "project-2",
        }),
      ],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([focused, selected]);

    expect(screen.queryByTestId("agents-project-project-1")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-project-project-2")).toBeInTheDocument();
    expect(screen.getByTestId("agents-session-conversation-filtered-project"))
      .toHaveTextContent("Filtered project");
    expect(
      projectConversationCalls.filter((call) => call.projectId === "project-2").at(-1)
        ?.options?.enabled,
    ).toBe(true);
  });

  it("keeps the add project footer action alongside the restored controls", () => {
    const onCreateProject = vi.fn();
    conversationsByProject.set("project-1", {
      data: [conversation()],
      total: 6,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onCreateProject });

    fireEvent.click(screen.getByTestId("agents-add-project"));

    expect(onCreateProject).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("agents-filters-trigger")).toBeInTheDocument();
  });

  it("opens the sort dropdown synchronously with a selected conversation", () => {
    const conv = conversation({ id: "conversation-selected", title: "Selected run" });
    conversationsByProject.set("project-1", {
      data: [conv],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { selectedConversationId: conv.id });

    fireEvent.pointerDown(screen.getByTestId("agents-sort-trigger"), {
      button: 0,
      ctrlKey: false,
    });

    expect(screen.getByRole("menuitemradio", { name: "Latest" })).toBeInTheDocument();
  });

  it("sorts project groups by most recent conversation when sort is latest", () => {
    const alpha = project({ id: "project-1", name: "alpha" });
    const beta = project({ id: "project-2", name: "beta" });
    const gamma = project({ id: "project-3", name: "gamma" });

    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1", projectId: "project-1", contextId: "project-1" })],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [conversation({ id: "conversation-2", projectId: "project-2", contextId: "project-2" })],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-3", {
      data: [conversation({ id: "conversation-3", projectId: "project-3", contextId: "project-3" })],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    useAgentSessionStore.setState({
      expandedProjectIds: {
        "project-1": true,
        "project-2": true,
        "project-3": true,
      },
    });

    latestProjectOrderData.current = ["project-3", "project-1", "project-2"];

    renderSidebar([alpha, beta, gamma]);

    expect(getProjectRowOrder()).toEqual([
      "agents-project-project-3",
      "agents-project-project-1",
      "agents-project-project-2",
    ]);
  });

  it("can sort projects alphabetically after latest sort", async () => {
    const user = userEvent.setup();
    const alpha = project({ id: "project-1", name: "alpha" });
    const beta = project({ id: "project-2", name: "beta" });

    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-1", projectId: "project-1", contextId: "project-1" })],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    conversationsByProject.set("project-2", {
      data: [conversation({ id: "conversation-2", projectId: "project-2", contextId: "project-2" })],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    latestProjectOrderData.current = ["project-2", "project-1"];

    renderSidebar([alpha, beta]);

    expect(getProjectRowOrder()).toEqual([
      "agents-project-project-2",
      "agents-project-project-1",
    ]);

    await user.click(screen.getByTestId("agents-sort-trigger"));
    await user.click(screen.getByRole("menuitemradio", { name: "A-Z" }));

    await waitFor(() =>
      expect(useAgentSessionStore.getState().projectSort).toBe("az")
    );
    await waitFor(() =>
      expect(getProjectRowOrder()).toEqual([
        "agents-project-project-1",
        "agents-project-project-2",
      ])
    );
  });

  it("uses sort for conversations inside publication-state groups", async () => {
    const user = userEvent.setup();
    const zulu = conversation({
      id: "conversation-zulu",
      title: "Zulu task",
      createdAt: "2026-05-02T10:00:00Z",
    });
    const alpha = conversation({
      id: "conversation-alpha",
      title: "Alpha task",
      createdAt: "2026-05-01T10:00:00Z",
    });
    conversationsByProject.set("project-1", {
      data: [alpha, zulu],
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()]);

    await user.click(screen.getByTestId("agents-group-trigger"));
    await user.click(screen.getByRole("radio", { name: "Publication state" }));

    expect(getSessionRowOrder()).toEqual([
      "agents-session-conversation-zulu",
      "agents-session-conversation-alpha",
    ]);

    await user.click(screen.getByTestId("agents-sort-trigger"));
    await user.click(screen.getByRole("menuitemradio", { name: "A-Z" }));

    await waitFor(() =>
      expect(getSessionRowOrder()).toEqual([
        "agents-session-conversation-alpha",
        "agents-session-conversation-zulu",
      ])
    );
    expect(publicationGroupCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          publicationState: "active",
          sort: "az",
        }),
      ])
    );
  });

  it("keeps project actions visible while open and confirms before archiving", () => {
    const onArchiveProject = vi.fn();
    conversationsByProject.set("project-1", {
      data: [conversation()],
      total: 6,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onArchiveProject });

    const actions = screen.getByTestId("agents-project-actions-project-1");
    const trigger = within(actions).getByRole("button", { name: "Project actions" });
    const count = within(screen.getByTestId("agents-project-row-project-1")).getByText("6");

    expect(count.className).toContain("group-hover/project-row:opacity-0");
    expect(actions.className).toContain("group-hover/project-row:opacity-100");
    expect(actions.className).not.toContain("group-hover/session:opacity-100");
    expect(trigger.className).toContain("hover:bg-transparent");
    expect(trigger.className).toContain("data-[state=open]:bg-transparent");

    fireEvent.pointerDown(trigger);

    expect(actions.className).toContain("opacity-100");
    expect(count.className).toContain("opacity-0");

    fireEvent.click(screen.getByText("Archive project"));

    expect(screen.getByText("Archive project?")).toBeInTheDocument();
    expect(onArchiveProject).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Archive project" }));

    expect(onArchiveProject).toHaveBeenCalledWith("project-1");
  });

  it("opens the PR template editor from project actions before archive", () => {
    conversationsByProject.set("project-1", {
      data: [conversation()],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()]);

    const actions = screen.getByTestId("agents-project-actions-project-1");
    const trigger = within(actions).getByRole("button", { name: "Project actions" });
    fireEvent.pointerDown(trigger);

    const editItem = screen.getByText("Edit PR Template");
    const archiveItem = screen.getByText("Archive project");
    expect(
      editItem.compareDocumentPosition(archiveItem) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();

    fireEvent.click(editItem);

    expect(screen.getByTestId("pr-template-editor-dialog")).toHaveTextContent(
      "Edit PR Template for ralphx"
    );
    expect(prTemplateDialogCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ open: true, projectId: "project-1" }),
      ])
    );
  });

  it("does not show a tooltip for project actions", async () => {
    const user = userEvent.setup();
    conversationsByProject.set("project-1", {
      data: [conversation()],
      total: 6,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()]);

    const actions = screen.getByTestId("agents-project-actions-project-1");
    const trigger = within(actions).getByRole("button", { name: "Project actions" });

    await user.hover(trigger);
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("opens a rename dialog from session actions and saves the new title", async () => {
    const user = userEvent.setup();
    const onRenameConversation = vi.fn().mockResolvedValue(undefined);
    const activeConversation = conversation({ id: "conversation-rename", title: "Untitled agent" });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onRenameConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));

    const input = screen.getByLabelText("Session title");
    await user.clear(input);
    await user.type(input, "Review follow-up");
    await user.click(screen.getByRole("button", { name: "Rename session" }));

    await waitFor(() =>
      expect(onRenameConversation).toHaveBeenCalledWith("conversation-rename", "Review follow-up")
    );
    expect(screen.queryByText("Rename session")).not.toBeInTheDocument();
  });

  it("starts auto rename from the project session rename dialog", async () => {
    const user = userEvent.setup();
    const onAutoRenameConversation = vi.fn().mockResolvedValue(undefined);
    const onRenameConversation = vi.fn().mockResolvedValue(undefined);
    const activeConversation = conversation({
      id: "conversation-auto-rename",
      title: "Discuss stale fallback",
    });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      onAutoRenameConversation,
      onRenameConversation,
    });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));
    await user.click(screen.getByRole("button", { name: "Auto rename" }));

    await waitFor(() =>
      expect(onAutoRenameConversation).toHaveBeenCalledWith(activeConversation)
    );
    expect(onRenameConversation).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByLabelText("Session title")).toBeNull());
  });

  it("forks a session from row actions", async () => {
    const user = userEvent.setup();
    const onForkConversation = vi.fn().mockResolvedValue(undefined);
    const activeConversation = conversation({ id: "conversation-fork", title: "Forkable run" });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onForkConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Fork session"));

    expect(screen.getByText("Fork session?")).toBeInTheDocument();
    expect(onForkConversation).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Fork session" }));

    await waitFor(() => expect(onForkConversation).toHaveBeenCalledWith(activeConversation));
  });

  it("cancels a session fork from row actions", async () => {
    const user = userEvent.setup();
    const onForkConversation = vi.fn().mockResolvedValue(undefined);
    const activeConversation = conversation({ id: "conversation-fork-cancel", title: "Forkable run" });
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onForkConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Fork session"));
    await user.click(screen.getByRole("button", { name: "Cancel" }));

    expect(onForkConversation).not.toHaveBeenCalled();
  });

  it("hides the session status dot when the row action menu is visible", async () => {
    const user = userEvent.setup();
    const conv = conversation({ id: "conversation-menu-overlap", title: "Menu overlap" });
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()]);

    const row = screen.getByTestId("agents-session-conversation-menu-overlap");
    const statusSlot = row.querySelector(".agents-session-status-slot");
    const dot = row.querySelector('span[aria-hidden="true"].rounded-full');
    expect(statusSlot?.className).toContain("h-4");
    expect(statusSlot?.className).toContain("w-4");
    expect(statusSlot?.className).toContain("group-hover/session:opacity-0");
    expect(dot?.className).toContain("block");
    expect(dot?.className).toContain("h-[7px]");
    expect(dot?.className).toContain("w-[7px]");

    const trigger = within(row).getByRole("button", { name: "Session actions" });
    await user.click(trigger);

    expect(trigger.className).toContain("hover:bg-transparent");
    expect(trigger.className).toContain("data-[state=open]:bg-transparent");
    expect(statusSlot?.className).toContain("opacity-0");
  });

  it("confirms before archiving a session", async () => {
    const user = userEvent.setup();
    const onArchiveConversation = vi.fn();
    const activeConversation = conversation({ id: "conversation-archive", title: "Untitled agent" });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: activeConversation.id,
        linkedPlanBranchId: "plan-branch-archive",
      }),
    ]);
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onArchiveConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Archive session"));

    expect(screen.getByText("Archive session?")).toBeInTheDocument();
    expect(onArchiveConversation).not.toHaveBeenCalled();
    expect(
      screen.getByText("Archiving leaves this pull request open unless you choose to close it.")
    ).toBeInTheDocument();
    expect(
      screen.getByText(/including uncommitted changes and ignored build or test artifacts/)
    ).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Close pull request" })).not.toBeChecked();

    await user.click(screen.getByRole("button", { name: "Archive session" }));

    expect(onArchiveConversation).toHaveBeenCalledWith(activeConversation, {
      closePullRequest: false,
    });
  });

  it("does not offer pull-request closure when the session has no pull request", async () => {
    const user = userEvent.setup();
    const activeConversation = conversation({ id: "conversation-archive-without-pr" });
    workspacesByProject.set("project-1", [
      workspace({ conversationId: activeConversation.id }),
    ]);
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()]);

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Archive session"));

    expect(screen.queryByRole("checkbox", { name: "Close pull request" })).not.toBeInTheDocument();
  });

  it("sends archive PR closure only after explicit selection and resets it when reopened", async () => {
    const user = userEvent.setup();
    const onArchiveConversation = vi.fn();
    const activeConversation = conversation({ id: "conversation-archive-opt-in" });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: activeConversation.id,
        publicationPrNumber: 43,
        publicationPrStatus: "open",
      }),
    ]);
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onArchiveConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Archive session"));
    const closePullRequest = screen.getByRole("checkbox", { name: "Close pull request" });
    await user.click(closePullRequest);
    expect(closePullRequest).toBeChecked();

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Archive session"));
    expect(screen.getByRole("checkbox", { name: "Close pull request" })).not.toBeChecked();

    await user.click(screen.getByRole("checkbox", { name: "Close pull request" }));
    await user.click(screen.getByRole("button", { name: "Archive session" }));

    expect(onArchiveConversation).toHaveBeenCalledWith(activeConversation, {
      closePullRequest: true,
    });
  });

  it("keeps Review PR archive separate from pull-request closure", async () => {
    const user = userEvent.setup();
    const onArchiveConversation = vi.fn();
    const activeConversation = conversation({ id: "conversation-archive-review-pr" });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: activeConversation.id,
        mode: "review_pr",
        publicationPrNumber: 44,
        publicationPrStatus: "open",
      }),
    ]);
    conversationsByProject.set("project-1", {
      data: [activeConversation],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onArchiveConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Archive session"));

    expect(screen.queryByRole("checkbox", { name: "Close pull request" })).not.toBeInTheDocument();
    expect(screen.getByText("The reviewed pull request will remain open.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Archive session" }));
    expect(onArchiveConversation).toHaveBeenCalledWith(activeConversation, {
      closePullRequest: false,
    });
  });

  it("toggles the sidebar search input and clears the query via the X button", async () => {
    const user = userEvent.setup();
    renderSidebar();

    expect(screen.queryByTestId("agents-search-input")).toBeNull();
    await user.click(screen.getByTestId("agents-search-toggle"));
    const input = screen.getByTestId("agents-search-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "alpha" } });
    expect(input.value).toBe("alpha");

    await user.click(screen.getByLabelText("Clear search"));
    expect(input.value).toBe("");

    // Toggling search closed clears the query and removes the input row.
    fireEvent.change(input, { target: { value: "beta" } });
    await user.click(screen.getByTestId("agents-search-toggle"));
    expect(screen.queryByTestId("agents-search-input")).toBeNull();
  });

  it("toggles the whole project row via the agentSessionStore", async () => {
    const user = userEvent.setup();
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true },
      showAllProjects: true,
      projectSort: "latest",
    });
    conversationsByProject.set("project-1", {
      data: [],
      total: 0,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar();

    const projectRow = screen.getByTestId("agents-project-row-project-1");
    expect(projectRow).toHaveAttribute("aria-expanded", "true");

    await user.click(projectRow);
    expect(useAgentSessionStore.getState().expandedProjectIds["project-1"]).toBe(false);
    expect(projectRow).toHaveAttribute("aria-expanded", "false");

    await user.click(projectRow);
    expect(useAgentSessionStore.getState().expandedProjectIds["project-1"]).toBe(true);
    expect(projectRow).toHaveAttribute("aria-expanded", "true");
  });

  it("focuses and expands a project row without selecting a conversation", async () => {
    const user = userEvent.setup();
    const onFocusProject = vi.fn();
    const onSelectConversation = vi.fn();
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": false },
      showAllProjects: true,
      projectSort: "latest",
    });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-not-selected", title: "Do not select me" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    renderSidebar([project()], { onFocusProject, onSelectConversation });

    await user.click(screen.getByTestId("agents-project-row-project-1"));

    expect(onFocusProject).toHaveBeenCalledWith("project-1");
    expect(onSelectConversation).not.toHaveBeenCalled();
    expect(useAgentSessionStore.getState().expandedProjectIds["project-1"]).toBe(true);
  });

  it("renders a collapsed project neutrally even when it contains the selected conversation", async () => {
    const user = userEvent.setup();
    const selected = conversation({ id: "conversation-selected", title: "Selected run" });
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true },
      showAllProjects: true,
      projectSort: "latest",
    });
    conversationsByProject.set("project-1", {
      data: [selected],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      focusedProjectId: "project-1",
      selectedConversationId: selected.id,
    });

    const projectRow = screen.getByTestId("agents-project-row-project-1");
    expect(projectRow).toHaveAttribute("aria-current", "true");

    await user.click(projectRow);

    expect(projectRow).not.toHaveAttribute("aria-current");
    expect(screen.queryByTestId("agents-session-conversation-selected")).not.toBeInTheDocument();
  });

  it("clears focused project active styling when the project is collapsed", async () => {
    const user = userEvent.setup();
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": true },
      showAllProjects: true,
      projectSort: "latest",
    });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-counted", title: "Counted run" })],
      total: 46,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      focusedProjectId: "project-1",
      selectedConversationId: null,
    });

    const projectRow = screen.getByTestId("agents-project-row-project-1");
    expect(projectRow).toHaveAttribute("aria-current", "true");
    expect(projectRow).toHaveAttribute("aria-expanded", "true");

    await user.click(projectRow);

    expect(projectRow).toHaveAttribute("aria-expanded", "false");
    expect(projectRow).not.toHaveAttribute("aria-current");
  });

  it("renders the active runtime badge when a project is collapsed but has a running agent", () => {
    useAgentSessionStore.setState({
      expandedProjectIds: { "project-1": false },
      showAllProjects: true,
      projectSort: "latest",
    });
    const conv = conversation({ id: "conversation-running", title: "Running" });
    const storeKey = getAgentConversationStoreKey(conv);
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 0,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useChatStore.setState({
      activeConversationIds: { [storeKey]: conv.id },
      agentStatus: { [storeKey]: "generating" },
    });

    renderSidebar([project()], { focusedProjectId: null });

    const projectRow = screen.getByTestId("agents-project-row-project-1");
    expect(within(projectRow).getByText("1")).toBeInTheDocument();
  });

  it("selects a conversation row when clicked", async () => {
    const user = userEvent.setup();
    const onSelectConversation = vi.fn();
    const conv = conversation({ id: "conversation-pick", title: "Pick me" });
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onSelectConversation });

    await user.click(screen.getByText("Pick me"));
    expect(onSelectConversation).toHaveBeenCalledWith("project-1", conv);
  });

  it("submits a rename via the Enter key inside the dialog", async () => {
    const user = userEvent.setup();
    const onRenameConversation = vi.fn().mockResolvedValue(undefined);
    const conv = conversation({ id: "conversation-rename-2", title: "old" });
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onRenameConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));
    const titleInput = screen.getByLabelText("Session title");
    await user.clear(titleInput);
    await user.type(titleInput, "renamed-via-enter{Enter}");
    await waitFor(() =>
      expect(onRenameConversation).toHaveBeenCalledWith(
        "conversation-rename-2",
        "renamed-via-enter",
      ),
    );
  });

  it("cancel button in rename dialog closes without invoking onRenameConversation", async () => {
    const user = userEvent.setup();
    const onRenameConversation = vi.fn();
    const conv = conversation({ id: "conversation-cancel-rename", title: "old" });
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onRenameConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));
    expect(screen.getByLabelText("Session title")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByLabelText("Session title")).toBeNull());
    expect(onRenameConversation).not.toHaveBeenCalled();
  });

  it("restores an archived conversation via the row dropdown", async () => {
    const user = userEvent.setup();
    const onRestoreConversation = vi.fn();
    const archived = conversation({
      id: "conversation-archived",
      title: "Old run",
      archivedAt: "2026-04-22T13:00:00Z",
    });
    conversationsByProject.set("project-1", {
      data: [archived],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      showArchived: true,
      onRestoreConversation,
    });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Restore session"));
    expect(onRestoreConversation).toHaveBeenCalledWith(archived);
  });

  it("renders the running runtime label and accent status dot for a generating conversation", () => {
    const conv = conversation({ id: "conversation-run", title: "Live run" });
    const storeKey = getAgentConversationStoreKey(conv);
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useChatStore.setState({
      activeConversationIds: { [storeKey]: conv.id },
      agentStatus: { [storeKey]: "generating" },
    });

    renderSidebar();

    expect(screen.getByText("running")).toBeInTheDocument();
  });

  it("renders an awaiting input runtime label for a retained idle conversation", () => {
    const conv = conversation({ id: "conversation-waiting", title: "Waiting run" });
    const storeKey = getAgentConversationStoreKey(conv);
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useChatStore.setState({
      activeConversationIds: { [storeKey]: conv.id },
      agentStatus: { [storeKey]: "waiting_for_input" },
    });

    renderSidebar();

    expect(screen.getByText("awaiting input")).toBeInTheDocument();
    expect(screen.queryByText("running")).not.toBeInTheDocument();
  });

  it("orders backend-returned pinned conversations before unpinned rows", () => {
    const loaded = conversation({ id: "conversation-loaded", title: "Loaded" });
    const pinned = conversation({ id: "conversation-pinned", title: "Pinned run" });
    conversationsByProject.set("project-1", {
      data: [loaded, pinned],
      total: 2,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useAgentSessionStore.setState({
      pinnedConversationIds: { [pinned.id]: true },
    });

    renderSidebar([project()], {
      selectedConversationId: pinned.id,
    });

    const rows = screen.getAllByTestId(/agents-session-/);
    expect(rows.map((row) => row.getAttribute("data-testid"))).toEqual([
      "agents-session-conversation-pinned",
      "agents-session-conversation-loaded",
    ]);
  });

  it("keeps real pinned conversations ahead of selected fallback priority rows", () => {
    const selected = conversation({
      id: "conversation-selected-new",
      title: "Selected new run",
      createdAt: "2026-04-22T13:00:00Z",
    });
    const pinned = conversation({
      id: "conversation-pinned-old",
      title: "Pinned old run",
      createdAt: "2026-04-22T10:00:00Z",
    });
    const unpinned = conversation({
      id: "conversation-unpinned-middle",
      title: "Unpinned middle run",
      createdAt: "2026-04-22T12:00:00Z",
    });
    conversationsByProject.set("project-1", {
      data: [selected, unpinned, pinned],
      total: 3,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useAgentSessionStore.setState({
      pinnedConversationIds: { [pinned.id]: true },
    });

    renderSidebar([project()], {
      selectedConversationId: selected.id,
      pinnedConversation: selected,
    });

    expect(getSessionRowOrder()).toEqual([
      "agents-session-conversation-pinned-old",
      "agents-session-conversation-selected-new",
      "agents-session-conversation-unpinned-middle",
    ]);
    expect(projectConversationCalls.at(-1)).toEqual(
      expect.objectContaining({
        pinnedConversationIds: [pinned.id],
        priorityConversationIds: [selected.id],
      })
    );
  });

  it("does not duplicate a pinnedConversation already present in the loaded list", () => {
    const shared = conversation({ id: "conversation-shared", title: "Shared run" });
    conversationsByProject.set("project-1", {
      data: [shared],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      pinnedConversation: shared,
      selectedConversationId: shared.id,
    });

    const rows = screen.getAllByTestId(/agents-session-/);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveAttribute("data-testid", "agents-session-conversation-shared");
    expect(projectConversationCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          projectId: "project-1",
          pinnedConversationIds: [],
          priorityConversationIds: [shared.id],
        }),
      ])
    );
  });

  it("pins and unpins a session from the row action menu", async () => {
    const user = userEvent.setup();
    const older = conversation({
      id: "conversation-older",
      title: "Older",
      createdAt: "2026-04-22T10:00:00Z",
    });
    const newer = conversation({
      id: "conversation-newer",
      title: "Newer",
      createdAt: "2026-04-22T12:00:00Z",
    });
    conversationsByProject.set("project-1", {
      data: [newer, older],
      total: 2,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()]);

    await user.click(
      within(screen.getByTestId("agents-session-conversation-older")).getByRole(
        "button",
        { name: "Session actions" }
      )
    );
    await user.click(screen.getByText("Pin session"));

    expect(
      useAgentSessionStore.getState().pinnedConversationIds["conversation-older"]
    ).toBe(true);
    expect(
      screen.getAllByTestId(/agents-session-/).map((row) => row.getAttribute("data-testid"))
    ).toEqual([
      "agents-session-conversation-older",
      "agents-session-conversation-newer",
    ]);
    expect(screen.getByTestId("agents-pin-icon-conversation-older")).toBeInTheDocument();

    await user.click(
      within(screen.getByTestId("agents-session-conversation-older")).getByRole(
        "button",
        { name: "Session actions" }
      )
    );
    await user.click(screen.getByText("Unpin session"));
    expect(
      useAgentSessionStore.getState().pinnedConversationIds["conversation-older"]
    ).toBeUndefined();
  });

  it("uses the pinned icon as the colored live status slot for pinned running sessions", () => {
    const conv = conversation({ id: "conversation-pinned-running", title: "Pinned live" });
    const storeKey = getAgentConversationStoreKey(conv);
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useAgentSessionStore.setState({
      pinnedConversationIds: { [conv.id]: true },
    });
    useChatStore.setState({
      activeConversationIds: { [storeKey]: conv.id },
      agentStatus: { [storeKey]: "running" },
    });

    renderSidebar();

    expect(
      screen
        .getByTestId("agents-pin-icon-conversation-pinned-running")
        .getAttribute("style")
    ).toContain("color: var(--accent-primary)");
  });

  it("shows mute before archive and keeps a pinned muted session pinned", async () => {
    const user = userEvent.setup();
    const conv = conversation({ id: "conversation-muted", title: "Muted session" });
    conversationsByProject.set("project-1", {
      data: [conv], total: 1, isLoading: false, hasNextPage: false, isFetchingNextPage: false,
    });
    mutedConversationIds.add(conv.id);
    useAgentSessionStore.setState({ pinnedConversationIds: { [conv.id]: true } });
    const onSetConversationMuted = vi.fn();
    renderSidebar([project()], { onSetConversationMuted });

    expect(screen.getByTestId(`agents-pin-icon-${conv.id}`)).toBeInTheDocument();
    expect(screen.queryByTestId(`agents-mute-icon-${conv.id}`)).not.toBeInTheDocument();
    await user.click(within(screen.getByTestId(`agents-session-${conv.id}`)).getByRole("button", { name: "Session actions" }));
    expect(screen.getByText("Unmute session")).toBeInTheDocument();
    expect(screen.getByText("Returns it to its normal lane.")).toBeInTheDocument();
    expect(screen.getByText("Deletes the local workspace and branch.")).toBeInTheDocument();
    const menuItemTexts = screen.getAllByRole("menuitem").map((item) => item.textContent);
    expect(menuItemTexts.indexOf("Unmute sessionReturns it to its normal lane.")).toBeLessThan(
      menuItemTexts.indexOf("Archive sessionDeletes the local workspace and branch.")
    );
    await user.click(screen.getByText("Unmute session"));
    expect(onSetConversationMuted).toHaveBeenCalledWith(conv, false);
  });

  it("renders the muted marker when the session is not pinned", () => {
    const conv = conversation({ id: "conversation-muted-marker" });
    conversationsByProject.set("project-1", {
      data: [conv], total: 1, isLoading: false, hasNextPage: false, isFetchingNextPage: false,
    });
    mutedConversationIds.add(conv.id);

    renderSidebar([project()]);

    expect(screen.getByTestId(`agents-mute-icon-${conv.id}`)).toHaveAccessibleName(
      "Muted until it changes"
    );
  });

  it("offers Mute until it changes for an unmuted session", async () => {
    const user = userEvent.setup();
    const conv = conversation({ id: "conversation-unmuted-menu" });
    conversationsByProject.set("project-1", {
      data: [conv], total: 1, isLoading: false, hasNextPage: false, isFetchingNextPage: false,
    });
    renderSidebar([project()]);

    await user.click(within(screen.getByTestId(`agents-session-${conv.id}`)).getByRole("button", { name: "Session actions" }));
    expect(screen.getByText("Mute until it changes")).toBeInTheDocument();
  });

  it("groups conversations by publication state when selected in Group", async () => {
    const user = userEvent.setup();
    const merged = conversation({ id: "conversation-merged", title: "Merged run" });
    const closed = conversation({ id: "conversation-closed", title: "Closed run" });
    conversationsByProject.set("project-1", {
      data: [merged, closed],
      total: 2,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: merged.id,
        publicationPrNumber: 11,
        publicationPrStatus: "merged",
      }),
      workspace({
        conversationId: closed.id,
        publicationPrNumber: 12,
        publicationPrStatus: "closed",
      }),
    ]);

    renderSidebar([project()]);

    await user.click(screen.getByTestId("agents-group-trigger"));
    await user.click(screen.getByRole("radio", { name: "Publication state" }));

    const activeButton = await screen.findByTestId("agents-publication-row-active");
    const mergedButton = screen.getByTestId("agents-publication-row-merged");
    const closedButton = screen.getByTestId("agents-publication-row-closed");
    const mergedRow = within(mergedButton);
    const closedRow = within(closedButton);
    expect(activeButton).toHaveAttribute("aria-current", "true");
    expect(mergedRow.getByText("Merged")).toBeInTheDocument();
    expect(mergedRow.getByText("1")).toHaveClass("agents-project-count");
    expect(closedRow.getByText("Closed")).toBeInTheDocument();
    expect(closedRow.getByText("1")).toHaveClass("agents-project-count");
    expect(screen.queryByTestId("agents-session-conversation-merged")).not.toBeInTheDocument();

    await user.click(mergedButton);
    expect(mergedButton).toHaveAttribute("aria-current", "true");
    expect(activeButton).not.toHaveAttribute("aria-current");
    expect(screen.getByTestId("agents-session-conversation-merged")).toBeInTheDocument();

    await user.click(closedButton);
    expect(closedButton).toHaveAttribute("aria-current", "true");
    expect(mergedButton).not.toHaveAttribute("aria-current");
    expect(screen.queryByTestId("agents-session-conversation-merged")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-session-conversation-closed")).toBeInTheDocument();
    expect(publicationGroupCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          projectIds: ["project-1"],
          publicationState: "merged",
          archivedOnly: false,
        }),
        expect.objectContaining({
          projectIds: ["project-1"],
          publicationState: "closed",
          archivedOnly: false,
        }),
      ])
    );
    expect(screen.queryByTestId("agents-project-row-project-1")).not.toBeInTheDocument();
  });

  it("groups conversations by automation with existing row interactions", async () => {
    const user = userEvent.setup();
    const onSelectConversation = vi.fn();
    const automationLabel =
      "Nightly release automation with a long label that should truncate cleanly";
    automationLabels.set("automation-release", automationLabel);
    const automationConversation = conversation({
      id: "conversation-automation-release",
      title: "Release automation setup",
      automationId: "automation-release",
    });
    const standaloneConversation = conversation({
      id: "conversation-standalone",
      title: "Standalone planning",
      createdAt: "2026-04-22T09:00:00Z",
    });
    conversationsByProject.set("project-1", {
      data: [automationConversation, standaloneConversation],
      total: 2,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useAgentSessionStore.setState({
      sidebarGroupBy: "automation",
      sidebarPublicationStateFilters: ["active"],
    });

    renderSidebar([project()], { onSelectConversation });

    const automationRow = screen.getByTestId(
      "agents-automation-row-automation-release"
    );
    const automationGroup = within(automationRow);
    expect(automationGroup.getByText(automationLabel)).toHaveClass(
      "min-w-0",
      "truncate"
    );
    expect(automationGroup.getByText("1")).toHaveClass("agents-project-count");
    expect(screen.getByTestId("agents-session-conversation-automation-release"))
      .toBeInTheDocument();
    expect(screen.queryByTestId("agents-session-conversation-standalone"))
      .not.toBeInTheDocument();
    expect(automationGroupCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          groupKey: "automation-release",
          enabled: true,
        }),
        expect.objectContaining({
          groupKey: "__standalone__",
          enabled: false,
        }),
      ])
    );

    await user.click(screen.getByTestId("agents-automation-row-__standalone__"));
    expect(screen.getByTestId("agents-session-conversation-standalone"))
      .toBeInTheDocument();
    expect(automationGroupCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          groupKey: "__standalone__",
          enabled: true,
        }),
      ])
    );
    await waitFor(() =>
      expect(screen.queryByTestId("agents-session-conversation-automation-release"))
        .not.toBeInTheDocument()
    );

    await user.click(screen.getByText("Standalone planning"));
    expect(onSelectConversation).toHaveBeenCalledWith(
      "project-1",
      standaloneConversation
    );
    expect(automationGroupIndexCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          projectIds: ["project-1"],
          archivedOnly: false,
          publicationStates: ["active"],
          sort: "latest",
        }),
      ])
    );
    expect(automationGroupCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          groupKey: "automation-release",
          projectIds: ["project-1"],
          publicationStates: ["active"],
          sort: "latest",
        }),
        expect.objectContaining({
          groupKey: "__standalone__",
          projectIds: ["project-1"],
          publicationStates: ["active"],
          sort: "latest",
        }),
      ])
    );
  });

  it("remembers publication-state scroll when switching open groups", async () => {
    const user = userEvent.setup();
    const activeRows = Array.from({ length: 12 }, (_, index) =>
      conversation({
        id: `conversation-active-${index + 1}`,
        title: `Active row ${index + 1}`,
      })
    );
    const merged = conversation({ id: "conversation-merged", title: "Merged row" });
    useAgentSessionStore.setState({
      sidebarGroupBy: "publication",
      sidebarPublicationStateFilters: ["active", "merged"],
    });
    conversationsByProject.set("project-1", {
      data: [...activeRows, merged],
      total: 13,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: merged.id,
        publicationPrNumber: 91,
        publicationPrStatus: "merged",
      }),
    ]);
    virtuosoMockState.dimensionsByTestId.set(
      "agents-sidebar-session-list-publication-active",
      {
        clientHeight: 460,
        scrollHeight: 920,
      }
    );

    renderSidebar([project()]);

    const activeList = screen.getByTestId(
      "agents-sidebar-session-list-publication-active"
    );
    activeList.scrollTop = 184;
    fireEvent.scroll(activeList);

    await user.click(screen.getByTestId("agents-publication-row-merged"));
    expect(
      screen.queryByTestId("agents-sidebar-session-list-publication-active")
    ).not.toBeInTheDocument();

    await user.click(screen.getByTestId("agents-publication-row-active"));

    await waitFor(() =>
      expect(
        screen.getByTestId("agents-sidebar-session-list-publication-active")
          .scrollTop
      ).toBe(184)
    );
  });

  it("auto-fetches the next publication-state page when its virtual list reaches the end", () => {
    const fetchNextPage = vi.fn().mockResolvedValue(undefined);
    useAgentSessionStore.setState({
      sidebarGroupBy: "publication",
      sidebarPublicationStateFilters: ["active"],
    });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-publication-active", title: "Active row" })],
      isLoading: false,
      hasNextPage: true,
      isFetchingNextPage: false,
      fetchNextPage,
    });

    renderSidebar([project()]);

    expect(
      screen.queryByTestId("agents-load-more-publication-active")
    ).not.toBeInTheDocument();
    virtuosoMockState.endReachedByTestId.get(
      "agents-sidebar-session-list-publication-active"
    )?.();

    expect(fetchNextPage).toHaveBeenCalledTimes(1);
  });

  it("handles row selection and actions inside publication-state groups", async () => {
    const user = userEvent.setup();
    const onSelectConversation = vi.fn();
    const onRenameConversation = vi.fn().mockResolvedValue(undefined);
    const onArchiveConversation = vi.fn();
    const active = conversation({ id: "conversation-publication-active", title: "Active row" });
    useAgentSessionStore.setState({
      sidebarGroupBy: "publication",
      sidebarPublicationStateFilters: ["active"],
    });
    conversationsByProject.set("project-1", {
      data: [active],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({
        conversationId: active.id,
        publicationPrNumber: 45,
        publicationPrStatus: "open",
      }),
    ]);

    renderSidebar([project()], {
      onSelectConversation,
      onRenameConversation,
      onArchiveConversation,
    });

    await user.click(screen.getByText("Active row"));
    expect(onSelectConversation).toHaveBeenCalledWith("project-1", active);

    const row = screen.getByTestId("agents-session-conversation-publication-active");
    await user.click(within(row).getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));
    const input = screen.getByLabelText("Session title");
    await user.clear(input);
    await user.type(input, "Publication rename{Enter}");
    await waitFor(() =>
      expect(onRenameConversation).toHaveBeenCalledWith(
        "conversation-publication-active",
        "Publication rename"
      )
    );

    await user.click(
      within(screen.getByTestId("agents-session-conversation-publication-active")).getByRole(
        "button",
        { name: "Session actions" }
      )
    );
    await user.click(screen.getByText("Archive session"));
    expect(screen.getByText("Archive session?")).toBeInTheDocument();
    const closePullRequest = screen.getByRole("checkbox", { name: "Close pull request" });
    expect(closePullRequest).not.toBeChecked();
    await user.click(closePullRequest);
    await user.click(screen.getByRole("button", { name: "Archive session" }));
    expect(onArchiveConversation).toHaveBeenCalledWith(active, {
      closePullRequest: true,
    });
  });

  it("starts auto rename from the publication session rename dialog", async () => {
    const user = userEvent.setup();
    const onAutoRenameConversation = vi.fn().mockResolvedValue(undefined);
    const active = conversation({
      id: "conversation-publication-auto-rename",
      title: "Discuss stale publication fallback",
    });
    useAgentSessionStore.setState({
      sidebarGroupBy: "publication",
      sidebarPublicationStateFilters: ["active"],
    });
    conversationsByProject.set("project-1", {
      data: [active],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], {
      onAutoRenameConversation,
    });

    const row = screen.getByTestId(
      "agents-session-conversation-publication-auto-rename"
    );
    await user.click(within(row).getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));
    await user.click(screen.getByRole("button", { name: "Auto rename" }));

    await waitFor(() =>
      expect(onAutoRenameConversation).toHaveBeenCalledWith(active)
    );
    await waitFor(() => expect(screen.queryByLabelText("Session title")).toBeNull());
  });

  it("opens the selected conversation destination group when publication state changes", async () => {
    const selected = conversation({
      id: "conversation-selected-publish",
      title: "Selected publish run",
    });
    useAgentSessionStore.setState({
      sidebarGroupBy: "publication",
      sidebarPublicationStateFilters: ["active", "draft"],
    });
    conversationsByProject.set("project-1", {
      data: [selected],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    workspacesByProject.set("project-1", [
      workspace({ conversationId: selected.id }),
    ]);

    const sidebarProps: Partial<ComponentProps<typeof AgentsSidebar>> = {
      selectedConversationId: selected.id,
    };
    const { rerender } = renderSidebar([project()], sidebarProps);

    expect(screen.getByTestId("agents-publication-row-active")).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(screen.getByTestId("agents-session-conversation-selected-publish"))
      .toBeInTheDocument();
    expect(screen.getByTestId("agents-publication-row-draft")).not.toHaveAttribute(
      "aria-current",
    );

    workspacesByProject.set("project-1", [
      workspace({
        conversationId: selected.id,
        publicationPrNumber: 194,
        publicationPrStatus: "draft",
        publicationPushStatus: "pushed",
      }),
    ]);

    rerender(
      <TooltipProvider delayDuration={0}>
        <AgentsSidebar
          projects={[project()]}
          focusedProjectId="project-1"
          selectedConversationId={selected.id}
          onFocusProject={vi.fn()}
          onSelectConversation={vi.fn()}
          onCreateAgent={vi.fn()}
          onCreateProject={vi.fn()}
          onArchiveProject={vi.fn()}
          onAutoRenameConversation={vi.fn()}
          onRenameConversation={vi.fn()}
          onArchiveConversation={vi.fn()}
          onRestoreConversation={vi.fn()}
          onForkConversation={vi.fn()}
          showArchived={false}
          onShowArchivedChange={vi.fn()}
        />
      </TooltipProvider>,
    );

    await waitFor(() =>
      expect(screen.getByTestId("agents-publication-row-draft")).toHaveAttribute(
        "aria-current",
        "true",
      )
    );
    expect(screen.getByTestId("agents-publication-row-active")).not.toHaveAttribute(
      "aria-current",
    );
    expect(screen.getByTestId("agents-session-conversation-selected-publish"))
      .toBeInTheDocument();
    expect(publicationGroupCalls).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          publicationState: "draft",
          pinnedConversationIds: [],
          priorityConversationIds: [],
        }),
      ])
    );
  });

  it("hides publication groups when every publication state is filtered out", async () => {
    const user = userEvent.setup();
    useAgentSessionStore.setState({
      sidebarGroupBy: "publication",
      sidebarPublicationStateFilters: ["active"],
    });
    conversationsByProject.set("project-1", {
      data: [conversation({ id: "conversation-active", title: "Active run" })],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()]);

    expect(screen.getByTestId("agents-publication-row-active")).toBeInTheDocument();
    await user.click(screen.getByTestId("agents-filters-trigger"));
    await user.click(
      screen.getByTestId("agents-filter-publication-section-trigger")
    );
    await user.click(screen.getByTestId("agents-filter-publication-state-active"));

    expect(useAgentSessionStore.getState().sidebarPublicationStateFilters).toEqual([]);
    await waitFor(() =>
      expect(screen.queryByTestId("agents-publication-row-active")).not.toBeInTheDocument()
    );
  });

  it("closes the rename dialog via Escape (onOpenChange false branch)", async () => {
    const user = userEvent.setup();
    const onRenameConversation = vi.fn();
    const conv = conversation({ id: "conversation-esc-rename", title: "old" });
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onRenameConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));
    expect(screen.getByLabelText("Session title")).toBeInTheDocument();

    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByLabelText("Session title")).toBeNull());
    expect(onRenameConversation).not.toHaveBeenCalled();
  });

  it("renders the done status dot when the active runtime status is completed", () => {
    const conv = conversation({ id: "conversation-done", title: "Done run" });
    const storeKey = getAgentConversationStoreKey(conv);
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useChatStore.setState({
      activeConversationIds: { [storeKey]: conv.id },
      agentStatus: { [storeKey]: "completed" },
    });

    renderSidebar();

    // SessionRuntimeLabel only renders for "running" — "done" returns null (line 859).
    expect(screen.queryByText("running")).not.toBeInTheDocument();

    // SessionStatusDot for "done" uses --status-success (line 889 branch).
    const row = screen.getByTestId("agents-session-conversation-done");
    const dot = row.querySelector('span[aria-hidden="true"].rounded-full');
    expect(dot).not.toBeNull();
    expect(dot?.className).toContain("block");
    expect(dot?.getAttribute("style") ?? "").toContain("var(--status-success)");
  });

  it("renders no status dot for blocked failed/error/needs_approval statuses", () => {
    const conv = conversation({ id: "conversation-blocked", title: "Blocked run" });
    const storeKey = getAgentConversationStoreKey(conv);
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });
    useChatStore.setState({
      activeConversationIds: { [storeKey]: conv.id },
      agentStatus: { [storeKey]: "failed" },
    });

    renderSidebar();

    // "blocked" state — neither running label nor success/accent dot rendered (lines 851, 859).
    expect(screen.queryByText("running")).not.toBeInTheDocument();
    const row = screen.getByTestId("agents-session-conversation-blocked");
    const dot = row.querySelector('span[aria-hidden="true"].rounded-full');
    // SessionStatusDot returns null for "blocked", so no rounded-full status dot present.
    expect(dot).toBeNull();
  });

  it("rename Submit no-ops when the dialog is closed before submitting", async () => {
    const user = userEvent.setup();
    const onRenameConversation = vi.fn().mockResolvedValue(undefined);
    const conv = conversation({ id: "conversation-rename-3", title: "" });
    conversationsByProject.set("project-1", {
      data: [conv],
      total: 1,
      isLoading: false,
      hasNextPage: false,
      isFetchingNextPage: false,
      fetchNextPage: vi.fn(),
    });

    renderSidebar([project()], { onRenameConversation });

    await user.click(screen.getByRole("button", { name: "Session actions" }));
    await user.click(screen.getByText("Rename session"));
    const titleInput = screen.getByLabelText("Session title");
    // Clear and submit Enter without any new value — trimmed length === 0 path.
    await user.clear(titleInput);
    await user.keyboard("{Enter}");
    expect(onRenameConversation).not.toHaveBeenCalled();
  });

  it("selects Inbox and renders Recent, PR Reviews, Stale, and Done chips in fixed order", async () => {
    const user = userEvent.setup();
    const lanes = ["needs", "working", "stale", "done"] as const;
    const conversations = lanes.map((lane) => {
      const value = conversation({ id: `conversation-${lane}`, title: lane });
      inboxLaneByConversationId.set(value.id, { lane, actionVerb: "Review" });
      return value;
    });
    conversationsByProject.set("project-1", { data: conversations, isLoading: false });

    renderSidebar();
    await user.click(screen.getByTestId("agents-group-trigger"));
    await user.click(screen.getByRole("radio", { name: "Inbox" }));

    await waitFor(() => {
      expect(screen.getByTestId("agents-inbox-lane-chips")).toBeInTheDocument();
    });
    expect(
      screen
        .getAllByRole("tab")
        .map((element) => element.dataset.testid)
    ).toEqual([
      "agents-inbox-lane-chip-recent",
      "agents-inbox-lane-chip-reviews",
      "agents-inbox-lane-chip-stale",
      "agents-inbox-lane-chip-done",
    ]);
    expect(screen.getByTestId("agents-inbox-lane-chip-recent")).toHaveTextContent("2");
  });

  it("renders the PR Reviews chip with the summed count and a stable filter key", async () => {
    const needs = conversation({ id: "conversation-review-needs", title: "Needs approval" });
    const working = conversation({ id: "conversation-review-working", title: "Reviewing" });
    const watching = conversation({ id: "conversation-review-watching", title: "Approved" });
    inboxLaneByConversationId.set(needs.id, {
      lane: "review_needs",
      actionVerb: "Review",
      reviewState: "needs_approval",
    });
    inboxLaneByConversationId.set(working.id, {
      lane: "review_working",
      actionVerb: "Review",
      reviewState: "reviewing",
    });
    inboxLaneByConversationId.set(watching.id, {
      lane: "review_watching",
      actionVerb: "Review",
      reviewState: "approved",
    });
    conversationsByProject.set("project-1", {
      data: [needs, working, watching],
      isLoading: false,
    });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar();

    const chip = await screen.findByTestId("agents-inbox-lane-chip-reviews");
    // Regression guard for the removed `!` on `laneForInboxFilter`: a composite
    // filter used to render the string "undefined" here.
    await waitFor(() => expect(chip).toHaveTextContent("3"));
    expect(chip).not.toHaveTextContent("undefined");
    // Display copy is "PR Reviews" while the key stays "reviews".
    expect(chip).toHaveAccessibleName("PR Reviews, 3 conversations");
    expect(chip).toHaveAttribute("id", "agents-inbox-lane-chip-reviews");
  });

  it("shows three review groups and keeps review rows out of Recent", async () => {
    const user = userEvent.setup();
    const needs = conversation({ id: "conversation-review-needs", title: "Needs approval" });
    const watching = conversation({ id: "conversation-review-watching", title: "Approved" });
    inboxLaneByConversationId.set(needs.id, {
      lane: "review_needs",
      actionVerb: "Review",
      reviewState: "needs_approval",
    });
    inboxLaneByConversationId.set(watching.id, {
      lane: "review_watching",
      actionVerb: "Review",
      reviewState: "approved",
    });
    conversationsByProject.set("project-1", {
      data: [needs, watching],
      isLoading: false,
    });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar();

    // Review rows carry review_* lanes, so Recent never sees them.
    expect(
      screen.queryByTestId("agents-sidebar-session-list-inbox-needs")
    ).not.toBeInTheDocument();

    await user.click(screen.getByTestId("agents-inbox-lane-chip-reviews"));

    await waitFor(() => {
      expect(screen.getByTestId("agents-inbox-lane-panel-reviews")).toBeInTheDocument();
    });
    expect(
      screen.getByTestId("agents-inbox-reviews-group-review_needs")
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-inbox-reviews-group-review_working")
    ).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-inbox-reviews-group-review_watching")
    ).toBeInTheDocument();
    expect(screen.getByText("Watching")).toBeInTheDocument();
  });

  it("renders the review state in the row meta line instead of the publish verb", async () => {
    const user = userEvent.setup();
    const watching = conversation({ id: "conversation-review-watching", title: "Approved PR" });
    inboxLaneByConversationId.set(watching.id, {
      lane: "review_watching",
      actionVerb: "Publish",
      reviewState: "approved",
    });
    conversationsByProject.set("project-1", { data: [watching], isLoading: false });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar();
    await user.click(screen.getByTestId("agents-inbox-lane-chip-reviews"));

    await waitFor(() => {
      expect(screen.getByText("Approved")).toBeInTheDocument();
    });
    expect(screen.queryByText("Publish")).not.toBeInTheDocument();
  });

  it("shows the reviews empty state when no review conversations exist", async () => {
    const user = userEvent.setup();
    conversationsByProject.set("project-1", { data: [], isLoading: false });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar();
    await user.click(screen.getByTestId("agents-inbox-lane-chip-reviews"));

    await waitFor(() => {
      expect(screen.getByTestId("agents-inbox-lane-empty-reviews")).toBeInTheDocument();
    });
    expect(screen.getByText("No open reviews")).toBeInTheDocument();
  });

  it("renders Recent groups inside one sidebar scroller", async () => {
    const user = userEvent.setup();
    const needs = conversation({ id: "conversation-needs", title: "Needs review" });
    const working = conversation({ id: "conversation-working", title: "Working review" });
    inboxLaneByConversationId.set(needs.id, { lane: "needs", actionVerb: "Review" });
    inboxLaneByConversationId.set(working.id, { lane: "working", actionVerb: "Publish" });
    conversationsByProject.set("project-1", { data: [needs, working], isLoading: false });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar();

    expect(screen.getByTestId("agents-sidebar-session-list-inbox-needs")).toBeInTheDocument();
    expect(screen.getByTestId("agents-sidebar-session-list-inbox-working")).toBeInTheDocument();
    expect(screen.getByTestId("agents-inbox-lane-panel-recent")).toBeInTheDocument();
    expect(screen.getByText("Needs you")).toBeInTheDocument();
    expect(screen.getByText("Working")).toBeInTheDocument();
    expect(screen.getByTestId("agents-sidebar-session-list-inbox-recent").querySelectorAll(".overflow-y-auto")).toHaveLength(0);
    // The lane list owns the pane's only scroller, so the body must not scroll too.
    expect(screen.getByTestId("agents-sidebar-body")).not.toHaveClass("overflow-y-auto");
    expect(screen.getByTestId("agents-sidebar-body")).toHaveClass("overflow-hidden");

    await user.click(screen.getByTestId("agents-inbox-lane-chip-stale"));

    await waitFor(() => {
      expect(screen.getByTestId("agents-inbox-lane-empty-stale")).toBeInTheDocument();
    });
    expect(
      screen.queryByTestId("agents-sidebar-session-list-inbox-recent")
    ).not.toBeInTheDocument();
    await waitFor(() => {
      expect(useAgentSessionStore.getState().sidebarInboxActiveLane).toBe("stale");
    });
  });

  it("keeps Recent group headers and their exhausted pager affordances stable", () => {
    const needs = conversation({ id: "conversation-recent-needs", title: "Needs review" });
    inboxLaneByConversationId.set(needs.id, { lane: "needs", actionVerb: "Review" });
    inboxGroupTotalsByLane.set("needs", 1);
    inboxGroupTotalsByLane.set("working", 0);
    conversationsByProject.set("project-1", { data: [needs], isLoading: false });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar();

    expect(screen.getByTestId("agents-inbox-recent-group-needs")).toHaveTextContent("Needs you");
    expect(screen.getByTestId("agents-inbox-recent-group-working")).toHaveTextContent("Working");
    expect(screen.getByTestId("agents-inbox-lane-empty-working")).toHaveTextContent("Nothing running");
    expect(screen.getByTestId("agents-inbox-recent-pager-needs")).toHaveTextContent("All 1 shown");
  });

  it("pages Needs you explicitly without changing Working rows", async () => {
    const user = userEvent.setup();
    const needs = conversation({ id: "conversation-page-needs", title: "First needs" });
    const olderNeeds = conversation({ id: "conversation-page-needs-older", title: "Older needs" });
    const working = conversation({ id: "conversation-page-working", title: "Working stays put" });
    inboxLaneByConversationId.set(needs.id, { lane: "needs", actionVerb: "Review" });
    inboxLaneByConversationId.set(olderNeeds.id, { lane: "needs", actionVerb: "Review" });
    inboxLaneByConversationId.set(working.id, { lane: "working", actionVerb: "Publish" });
    inboxGroupTotalsByLane.set("needs", 2);
    let addOlderNeeds = false;
    const fetchNextPage = vi.fn().mockImplementation(async () => { addOlderNeeds = true; });
    conversationsByProject.set("project-1", { data: [needs, working], isLoading: false, hasNextPage: true, fetchNextPage });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });
    const view = renderSidebar();

    expect(screen.getByTestId("agents-inbox-recent-pager-needs")).toHaveTextContent("Load 1 older");
    await user.click(screen.getByTestId("agents-inbox-recent-pager-needs"));
    expect(fetchNextPage).toHaveBeenCalledTimes(1);
    conversationsByProject.set("project-1", { data: addOlderNeeds ? [needs, olderNeeds, working] : [needs, working], isLoading: false, hasNextPage: false, fetchNextPage });
    view.rerender(<TooltipProvider delayDuration={0}><AgentsSidebar {...buildSidebarProps()} /></TooltipProvider>);

    expect(screen.getByTestId("agents-inbox-recent-group-working")).toHaveTextContent("Working stays put");
    expect(screen.getByTestId("agents-inbox-recent-pager-needs")).toHaveTextContent("All 2 shown");
    expect(screen.getByTestId("agents-inbox-recent-group-needs")).toHaveTextContent("Older needs");
  });

  it("keeps inbox row order stable when selecting a non-first conversation", async () => {
    const user = userEvent.setup();
    const newer = conversation({
      id: "conversation-inbox-newer",
      title: "Newer inbox row",
      createdAt: "2026-04-22T12:00:00Z",
    });
    const middle = conversation({
      id: "conversation-inbox-middle",
      title: "Middle inbox row",
      createdAt: "2026-04-22T11:00:00Z",
    });
    const older = conversation({
      id: "conversation-inbox-older",
      title: "Older inbox row",
      createdAt: "2026-04-22T10:00:00Z",
    });
    for (const value of [newer, middle, older]) {
      inboxLaneByConversationId.set(value.id, { lane: "needs", actionVerb: "Review" });
    }
    conversationsByProject.set("project-1", {
      data: [newer, middle, older],
      isLoading: false,
    });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    function StatefulInboxSidebar() {
      const [selectedConversation, setSelectedConversation] =
        useState<AgentConversation | null>(null);
      return (
        <TooltipProvider delayDuration={0}>
          <AgentsSidebar
            {...buildSidebarProps([project()])}
            selectedConversationId={selectedConversation?.id ?? null}
            pinnedConversation={selectedConversation}
            onSelectConversation={(_projectId, value) => setSelectedConversation(value)}
          />
        </TooltipProvider>
      );
    }

    render(<StatefulInboxSidebar />);
    const beforeSelection = getSessionRowOrder();
    inboxGroupCalls.length = 0;

    await user.click(
      within(screen.getByTestId("agents-session-conversation-inbox-middle")).getAllByRole(
        "button"
      )[0]!
    );

    await waitFor(() =>
      expect(inboxGroupCalls.filter((call) => call.lane === "needs").at(-1)).toEqual(
        expect.objectContaining({ priorityConversationIds: [] })
      )
    );
    expect(getSessionRowOrder()).toEqual(beforeSelection);
  });

  it("keeps a deep-linked inbox conversation in the priority request", () => {
    const deepLinked = conversation({
      id: "conversation-inbox-deep-link",
      title: "Deep-linked inbox row",
    });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar([project()], {
      selectedConversationId: deepLinked.id,
      pinnedConversation: deepLinked,
    });

    expect(inboxGroupCalls).toContainEqual(
      expect.objectContaining({
        priorityConversationIds: [deepLinked.id],
      })
    );
  });

  it("does not flash inbox empty copy before a zero-row lane query settles", () => {
    conversationsByProject.set("project-1", { data: [], isLoading: true });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    const view = renderSidebar();

    expect(screen.queryByTestId("agents-inbox-lane-empty-recent")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-inbox-recent-pager-needs")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-inbox-recent-pager-working")).not.toBeInTheDocument();

    conversationsByProject.set("project-1", { data: [], isLoading: false });
    view.rerender(
      <TooltipProvider delayDuration={0}>
        <AgentsSidebar {...buildSidebarProps()} />
      </TooltipProvider>
    );

    expect(screen.getByTestId("agents-inbox-lane-empty-recent")).toBeInTheDocument();
  });

  it("keeps zero-count inbox chips selectable and shows tier-specific empty copy", async () => {
    const user = userEvent.setup();
    const onCreateAgent = vi.fn();
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar([project()], { onCreateAgent });

    expect(screen.getByTestId("agents-inbox-lane-chip-recent")).toHaveAttribute(
      "aria-selected",
      "true"
    );
    expect(screen.getByTestId("agents-inbox-lane-empty-recent")).toHaveTextContent(
      "Inbox zero"
    );
    expect(
      within(screen.getByTestId("agents-inbox-lane-empty-recent")).getByRole(
        "button",
        { name: "New agent" }
      )
    ).toBeInTheDocument();
    await user.click(
      within(screen.getByTestId("agents-inbox-lane-empty-recent")).getByRole(
        "button",
        { name: "New agent" }
      )
    );
    expect(onCreateAgent).toHaveBeenCalledOnce();
    expect(screen.queryByRole("button", { name: /Review \d+ done/ })).not.toBeInTheDocument();

    await user.click(screen.getByTestId("agents-inbox-lane-chip-stale"));

    await waitFor(() => {
      expect(screen.getByTestId("agents-inbox-lane-empty-stale")).toHaveTextContent(
        "Nothing has gone stale"
      );
    });
    expect(
      within(screen.getByTestId("agents-inbox-lane-empty-stale")).queryByRole(
        "button",
        { name: "New agent" }
      )
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-inbox-lane-empty-recent")
    ).not.toBeInTheDocument();
    await waitFor(() => {
      expect(useAgentSessionStore.getState().sidebarInboxActiveLane).toBe("stale");
    });
  });

  it("wires the Recent zero secondary action to the live Done count", async () => {
    const doneConversations = [
      conversation({ id: "conversation-done-1", title: "Done one" }),
      conversation({ id: "conversation-done-2", title: "Done two" }),
    ];
    for (const value of doneConversations) {
      inboxLaneByConversationId.set(value.id, { lane: "done", actionVerb: "Merged" });
    }
    conversationsByProject.set("project-1", { data: doneConversations, isLoading: false });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar();

    await waitFor(() => {
      expect(screen.getByTestId("agents-inbox-lane-chip-done")).toHaveTextContent("2");
    });

    act(() => {
      fireEvent.click(screen.getByRole("button", { name: "Review 2 done" }));
    });
    await waitFor(() => {
      expect(screen.getByTestId("agents-inbox-lane-chip-done")).toHaveAttribute(
        "aria-selected",
        "true"
      );
    });
  });

  it("returns from a calm tier empty state through the existing lane selector", async () => {
    const user = userEvent.setup();
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });
    renderSidebar();

    await user.click(screen.getByTestId("agents-inbox-lane-chip-stale"));
    await waitFor(() => {
      expect(useAgentSessionStore.getState().sidebarInboxActiveLane).toBe("stale");
    });
    await user.click(await screen.findByRole("button", { name: "Back to Recent" }));

    await waitFor(() => {
      expect(screen.getByTestId("agents-inbox-lane-chip-recent")).toHaveAttribute(
        "aria-selected",
        "true"
      );
    });
  });

  it("shows a quiet filtered zero state and clears text search from its action", async () => {
    const user = userEvent.setup();
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });
    renderSidebar();

    await user.click(screen.getByTestId("agents-search-toggle"));
    fireEvent.change(screen.getByTestId("agents-search-input"), {
      target: { value: "no inbox result" },
    });

    const emptyState = await screen.findByTestId("agents-inbox-lane-empty-recent");
    await waitFor(() => expect(emptyState).toHaveTextContent("No matches"));
    expect(emptyState).not.toHaveTextContent("Inbox zero");

    await user.click(within(emptyState).getByRole("button", { name: "Clear search" }));
    await waitFor(() => expect(emptyState).toHaveTextContent("Inbox zero"));
  });

  it("repaints the selected lane chip before persisting the selection", async () => {
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar();

    act(() => {
      fireEvent.click(screen.getByTestId("agents-inbox-lane-chip-done"));
    });

    // Visible selection flips in the click commit; the store write is deferred
    // past the paint so serializing the sidebar store never blocks the switch.
    expect(screen.getByTestId("agents-inbox-lane-chip-done")).toHaveAttribute(
      "aria-selected",
      "true"
    );
    expect(useAgentSessionStore.getState().sidebarInboxActiveLane).toBe("recent");

    await waitFor(() => {
      expect(useAgentSessionStore.getState().sidebarInboxActiveLane).toBe("done");
    });
  });

  it("caps lane chip counts at 99+ while keeping the exact total accessible", async () => {
    const conversations = Array.from({ length: 120 }, (_, index) => {
      const value = conversation({
        id: `conversation-needs-${index}`,
        title: `Needs ${index}`,
      });
      inboxLaneByConversationId.set(value.id, { lane: "needs", actionVerb: "Review" });
      return value;
    });
    conversationsByProject.set("project-1", { data: conversations, isLoading: false });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar();

    const needsChip = await screen.findByTestId("agents-inbox-lane-chip-recent");
    await waitFor(() => {
      expect(needsChip).toHaveTextContent("99+");
    });
    expect(needsChip).toHaveAccessibleName("Recent, 120 conversations");
    expect(needsChip).toHaveAttribute("title", "Recent, 120 conversations");
  });

  it("moves lane selection with arrow keys and restores the selection after remount", async () => {
    const user = userEvent.setup();
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    const { unmount } = renderSidebar();

    const needsChip = screen.getByTestId("agents-inbox-lane-chip-recent");
    expect(needsChip).toHaveAttribute("tabindex", "0");
    expect(screen.getByTestId("agents-inbox-lane-chip-done")).toHaveAttribute(
      "tabindex",
      "-1"
    );

    needsChip.focus();
    await user.keyboard("{ArrowRight}{ArrowRight}");

    await waitFor(() => {
      expect(screen.getByTestId("agents-inbox-lane-chip-stale")).toHaveAttribute(
        "aria-selected",
        "true"
      );
    });

    await user.keyboard("{ArrowLeft}{ArrowLeft}{ArrowLeft}");
    await waitFor(() => {
      expect(screen.getByTestId("agents-inbox-lane-chip-done")).toHaveAttribute(
        "aria-selected",
        "true"
      );
    });
    await waitFor(() => {
      expect(useAgentSessionStore.getState().sidebarInboxActiveLane).toBe("done");
    });

    unmount();
    renderSidebar();

    expect(screen.getByTestId("agents-inbox-lane-chip-done")).toHaveAttribute(
      "aria-selected",
      "true"
    );
    expect(screen.getByTestId("agents-inbox-lane-panel-done")).toBeInTheDocument();
  });

  it("honors a persisted Stale inbox filter", () => {
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox", sidebarInboxActiveLane: "stale" });
    renderSidebar();
    expect(screen.getByTestId("agents-inbox-lane-chip-stale")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("agents-inbox-lane-panel-stale")).toBeInTheDocument();
  });

  it("preserves inbox verb and publication metadata and drops it when leaving the inbox", async () => {
    const needs = conversation({ id: "conversation-needs", title: "Needs review" });
    inboxLaneByConversationId.set(needs.id, { lane: "needs", actionVerb: "Review" });
    conversationsByProject.set("project-1", { data: [needs], isLoading: false });
    workspacesByProject.set("project-1", [
      workspace({ conversationId: needs.id, publicationPrStatus: "draft" }),
    ]);
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar();
    expect(screen.getByTestId("agents-session-conversation-needs")).toHaveTextContent(
      "Review"
    );
    expect(screen.getByTestId("agents-session-conversation-needs")).toHaveTextContent(
      "draft"
    );

    act(() => {
      useAgentSessionStore.setState({ sidebarGroupBy: "project" });
    });
    expect(screen.queryByText("Review")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-inbox-lane-chips")).not.toBeInTheDocument();
  });

  it("shows parked delegate work in the inbox row metadata", () => {
    const parked = conversation({ id: "conversation-parked", title: "Parked coordinator" });
    inboxLaneByConversationId.set(parked.id, {
      lane: "working",
      actionVerb: "Discuss",
      parkedDelegateCount: 2,
    });
    conversationsByProject.set("project-1", { data: [parked], isLoading: false });
    useAgentSessionStore.setState({ sidebarGroupBy: "inbox" });

    renderSidebar();

    expect(
      screen.getByTestId("agents-parked-delegates-conversation-parked")
    ).toHaveTextContent("Waiting on 2 delegates");
  });
});
