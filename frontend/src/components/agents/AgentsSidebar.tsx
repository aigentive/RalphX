import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import {
  Archive,
  Bell,
  BellOff,
  ArrowDownUp,
  Check,
  CheckCheck,
  CircleOff,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Folder,
  Clock,
  GitBranch,
  GitCompareArrows,
  GitFork,
  GitPullRequest,
  Inbox,
  MoreHorizontal,
  Pencil,
  Pin,
  PinOff,
  Plus,
  RotateCcw,
  Search,
  SearchX,
  SlidersHorizontal,
  Sparkles,
  WandSparkles,
  X,
  Zap,
  type LucideIcon,
} from "lucide-react";
import {
  createContext,
  memo,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import {
  Virtuoso,
  type ListRange,
  type StateSnapshot,
  type VirtuosoHandle,
} from "react-virtuoso";

import { Button } from "@/components/ui/button";
import { useConfirmation } from "@/hooks/useConfirmation";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useChatStore } from "@/stores/chatStore";
import type {
  AgentSidebarAttentionLane,
  AgentConversationWorkspace,
  AgentSidebarConversationRow,
} from "@/api/chat";
import {
  useAgentSessionStore,
  type AgentProjectSort,
  type AgentSidebarGroupBy,
  type AgentSidebarPublicationState,
} from "@/stores/agentSessionStore";
import { withAlpha } from "@/lib/theme-colors";
import type { Project } from "@/types/project";
import {
  formatAgentConversationCreatedAt,
  formatAgentConversationCreatedAtTitle,
  getAgentConversationStoreKey,
  toProjectAgentConversation,
  type AgentConversation,
  type AgentConversationArchiveOptions,
} from "./agentConversations";
import {
  getSidebarPublicationGroupLabel,
  PUBLICATION_STATE_OPTIONS,
} from "./agentSidebarMetadata";
import {
  AgentsInboxGroupEmptyStrip,
  AgentsInboxZeroCard,
} from "./AgentsInboxEmptyState";
import {
  AGENT_SIDEBAR_INBOX_FILTERED_EMPTY,
  AGENT_SIDEBAR_INBOX_FILTERS,
  AGENT_SIDEBAR_RECENT_GROUPS,
  describeInboxLaneCount,
  formatInboxLaneCount,
  formatParkedDelegateMeta,
  getAgeEscalation,
  laneForInboxFilter,
  shouldEscalateAge,
  summarizeInboxLaneCounts,
  type AgentSidebarInboxEmptyState,
  type AgentSidebarInboxFilter,
  type AgeEscalation,
} from "./agentSidebarInboxLanes";
import {
  useAgentSidebarAutomationGroup,
  useAgentSidebarAutomationGroupIndex,
  useAgentSidebarInboxGroup,
  useAgentSidebarProjectGroup,
  useAgentSidebarPublicationGroup,
  useProjectGroupLatestOrder,
} from "./useAgentSidebarPublicationGroup";
import {
  useAgentSidebarPublicationPolling,
  workspacePublicationFingerprint,
} from "./useAgentSidebarPublicationPolling";
import { useAgentSidebarRunningStates } from "./useAgentSidebarRunningStates";
import { useArchivedConversationCounts } from "./useArchivedConversationCounts";
import {
  ArchiveConversationDialog,
  type ArchiveConversationDialogTarget,
} from "./ArchiveConversationDialog";
import { PrTemplateEditorDialog } from "./PrTemplateEditorDialog";
import { BulkArchiveConversationControls } from "./BulkArchiveConversationControls";
import { BulkArchiveConversationCheckbox } from "./BulkArchiveConversationCheckbox";
import {
  type BulkArchiveConversationHandler,
} from "./bulkConversationArchive";
import {
  useBulkConversationArchiveController,
  useRegisterBulkArchiveRows,
} from "./useBulkConversationArchiveSelection";
import {
  BulkArchiveSelectionContext,
  useBulkArchiveSelection,
} from "./bulkConversationArchiveSelectionContext";

const PERSONA_BUILDER_MODE_META = {
  label: "Persona Builder",
  icon: WandSparkles,
} as const;

const CONVERSATION_MODE_META = {
  persona_builder: PERSONA_BUILDER_MODE_META,
} as const;

const AGENTS_SEARCH_DEBOUNCE_MS = 180;
const AGENTS_SIDEBAR_MAX_VISIBLE_SESSION_ROWS = 8;
const AGENTS_SIDEBAR_ADAPTIVE_MAX_VISIBLE_SESSION_ROWS = 48;
const AGENTS_SIDEBAR_ADAPTIVE_PAGE_OVERSCAN_ROWS = 2;
const AGENTS_SIDEBAR_FALLBACK_SESSION_ROW_PX = 46;
const AGENTS_SIDEBAR_SCROLL_MEMORY_LIMIT = 120;
const NO_PROJECT_GROUP_KEY = "__no_project__";
const STANDALONE_AUTOMATION_GROUP_KEY = "__standalone__";

type ArchiveConversationHandler = (
  conversation: AgentConversation,
  options: AgentConversationArchiveOptions
) => void;

const AgentConversationMuteContext = createContext<
  (conversation: AgentConversation, muted: boolean) => void | Promise<void>
>(() => undefined);

interface AgentSidebarSessionScrollMemory {
  inboxGroupRowCounts?: Partial<Record<AgentSidebarAttentionLane, number>>;
  rowCount?: number;
  scrollTop: number;
  stateSnapshot?: StateSnapshot;
}

interface AgentSidebarSessionScrollUpdate {
  inboxGroupRowCounts?: Partial<Record<AgentSidebarAttentionLane, number>>;
  rowCount?: number;
  stateSnapshot?: StateSnapshot;
}

const agentSidebarSessionScrollPositions = new Map<
  string,
  AgentSidebarSessionScrollMemory
>();
const agentSidebarSessionScrollListeners = new Set<() => void>();

function emitAgentSidebarSessionScrollMemoryChange() {
  for (const listener of agentSidebarSessionScrollListeners) {
    listener();
  }
}

function subscribeAgentSidebarSessionScrollMemory(listener: () => void) {
  agentSidebarSessionScrollListeners.add(listener);
  return () => {
    agentSidebarSessionScrollListeners.delete(listener);
  };
}

function rememberAgentSidebarSessionScroll(
  scrollKey: string,
  scrollTop: number,
  update: AgentSidebarSessionScrollUpdate = {}
) {
  const previous = agentSidebarSessionScrollPositions.get(scrollKey);
  const nextScrollTop = Math.max(0, scrollTop);
  const baseStateSnapshot = update.stateSnapshot ?? previous?.stateSnapshot;
  const nextStateSnapshot = baseStateSnapshot
    ? {
        ...baseStateSnapshot,
        scrollTop: nextScrollTop,
      }
    : undefined;
  const nextRowCount = update.rowCount ?? previous?.rowCount;
  const nextMemory: AgentSidebarSessionScrollMemory = {
    scrollTop: nextScrollTop,
  };
  const nextInboxGroupRowCounts =
    update.inboxGroupRowCounts ?? previous?.inboxGroupRowCounts;
  if (nextInboxGroupRowCounts) {
    nextMemory.inboxGroupRowCounts = nextInboxGroupRowCounts;
  }
  if (typeof nextRowCount === "number") {
    nextMemory.rowCount = nextRowCount;
  }
  if (nextStateSnapshot) {
    nextMemory.stateSnapshot = nextStateSnapshot;
  }
  const previousRowCount = previous?.rowCount ?? 0;
  const nextStoredRowCount = nextMemory.rowCount ?? 0;
  let rowCountChanged = previousRowCount !== nextStoredRowCount;
  agentSidebarSessionScrollPositions.delete(scrollKey);
  agentSidebarSessionScrollPositions.set(scrollKey, nextMemory);
  if (agentSidebarSessionScrollPositions.size > AGENTS_SIDEBAR_SCROLL_MEMORY_LIMIT) {
    const oldestKey = agentSidebarSessionScrollPositions.keys().next().value;
    if (oldestKey) {
      const oldestMemory = agentSidebarSessionScrollPositions.get(oldestKey);
      agentSidebarSessionScrollPositions.delete(oldestKey);
      rowCountChanged = rowCountChanged || Boolean(oldestMemory?.rowCount);
    }
  }
  if (!rowCountChanged) {
    return;
  }
  emitAgentSidebarSessionScrollMemoryChange();
}

function getRememberedAgentSidebarSessionRowCount(scrollKey: string) {
  return agentSidebarSessionScrollPositions.get(scrollKey)?.rowCount ?? 0;
}

function useRememberedAgentSidebarSessionRowCount(scrollKey: string) {
  const getSnapshot = useCallback(
    () => getRememberedAgentSidebarSessionRowCount(scrollKey),
    [scrollKey]
  );
  return useSyncExternalStore(
    subscribeAgentSidebarSessionScrollMemory,
    getSnapshot,
    () => 0
  );
}

const PROJECT_SORT_LABELS: Record<AgentProjectSort, string> = {
  latest: "Latest",
  az: "A-Z",
  za: "Z-A",
};

const SIDEBAR_GROUP_LABELS: Record<AgentSidebarGroupBy, string> = {
  inbox: "Inbox",
  project: "Project",
  publication: "Publication state",
  automation: "Automations",
};

// The grouping mode changes what the whole list means, so the trigger shows its
// active value instead of a static noun. Filters and Sort stay noun-labeled —
// they are modal refinements, this one selects a view.
const SIDEBAR_GROUP_ICONS: Record<AgentSidebarGroupBy, LucideIcon> = {
  inbox: Inbox,
  project: Folder,
  publication: GitPullRequest,
  automation: Zap,
};

const SIDEBAR_GROUP_SORT_TARGETS: Record<AgentSidebarGroupBy, string> = {
  inbox: "threads",
  project: "projects",
  publication: "conversations",
  automation: "automations",
};

function afterSidebarControlPaint(callback: () => void) {
  if (typeof window === "undefined") {
    callback();
    return;
  }

  window.requestAnimationFrame(() => {
    window.setTimeout(callback, 0);
  });
}

const STATIC_RECENT_RUNS = [
  {
    title: "Add ranking to reefbot homepage",
    project: "reefbot.ai",
    time: "2h",
  },
  {
    title: "Tighten kanban drag handles",
    project: "shapeapp",
    time: "yesterday",
  },
];

interface ScrollableAgentSessionListProps<T> {
  fetchNextPage: () => Promise<unknown>;
  fillAvailableHeight?: boolean;
  getItemKey: (row: T) => string;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  isLoading: boolean;
  isVisible: boolean;
  onViewportRowCapacityChange?: (rowCapacity: number) => void;
  onVisibleRowsChange?: (rows: T[]) => void;
  renderRow: (row: T) => ReactNode;
  rows: T[];
  scrollKey: string;
  testId: string;
}

function ScrollableAgentSessionList<T>({
  fetchNextPage,
  fillAvailableHeight = false,
  getItemKey,
  hasNextPage,
  isFetchingNextPage,
  isLoading,
  isVisible,
  onViewportRowCapacityChange,
  onVisibleRowsChange,
  renderRow,
  rows,
  scrollKey,
  testId,
}: ScrollableAgentSessionListProps<T>) {
  const scrollerRef = useRef<HTMLElement | null>(null);
  const virtuosoRef = useRef<VirtuosoHandle | null>(null);
  const latestRowCountRef = useRef(0);
  const latestScrollKeyRef = useRef(scrollKey);
  const lastScrollMemoryKeyRef = useRef<string | null>(null);
  const lastScrollTopRef = useRef(0);
  const [scrollerVersion, setScrollerVersion] = useState(0);
  const rowResizeObserverRef = useRef<ResizeObserver | null>(null);
  const viewportResizeObserverRef = useRef<ResizeObserver | null>(null);
  const latestVisibleRangeRef = useRef<ListRange | null>(null);
  const forceRestoreTargetRef = useRef<{
    scroller: HTMLElement;
    scrollKey: string;
  } | null>(null);
  const [measuredRowHeight, setMeasuredRowHeight] = useState<number | null>(null);
  const underflowFetchKeyRef = useRef<string | null>(null);
  const nextPageRequestRowCountRef = useRef<number | null>(null);
  const nextPageRequestIdRef = useRef(0);
  const rowCount = rows.length;
  const rowHeight =
    measuredRowHeight ?? AGENTS_SIDEBAR_FALLBACK_SESSION_ROW_PX;
  const hasRows = rowCount > 0;
  const rememberedScroll = useMemo(() => {
    if (!isVisible || !hasRows) {
      return null;
    }
    return agentSidebarSessionScrollPositions.get(scrollKey) ?? null;
  }, [hasRows, isVisible, scrollKey]);
  const initialScrollTop = rememberedScroll?.scrollTop ?? 0;
  const rememberedRowCount = rememberedScroll?.rowCount ?? 0;
  const hasEnoughRowsForRestore =
    rememberedRowCount === 0 || rowCount >= rememberedRowCount;
  const restoreStateFrom = useMemo<StateSnapshot | undefined>(() => {
    if (!rememberedScroll?.stateSnapshot || !hasEnoughRowsForRestore) {
      return undefined;
    }
    return {
      ...rememberedScroll.stateSnapshot,
      scrollTop: rememberedScroll.scrollTop,
    };
  }, [hasEnoughRowsForRestore, rememberedScroll]);
  const visibleRowSlots = Math.min(
    Math.max(rowCount, isLoading ? 1 : 0),
    AGENTS_SIDEBAR_MAX_VISIBLE_SESSION_ROWS
  );
  const viewportHeight = visibleRowSlots * rowHeight;
  const listStyle = useMemo<CSSProperties>(
    () =>
      fillAvailableHeight
        ? {
            flex: "1 1 auto",
            height: "100%",
            minHeight: 0,
            overflowX: "hidden",
          }
        : {
            height: `${viewportHeight}px`,
            maxHeight: `${AGENTS_SIDEBAR_MAX_VISIBLE_SESSION_ROWS * rowHeight}px`,
            overflowX: "hidden",
          },
    [fillAvailableHeight, rowHeight, viewportHeight]
  );
  const increaseViewportBy = useMemo(
    () => ({
      bottom: rowHeight * 2,
      top: rowHeight,
    }),
    [rowHeight]
  );
  const visibleRowsForRange = useCallback(
    (range: ListRange | null) => {
      if (rowCount === 0) {
        return [];
      }
      if (!range || range.endIndex < range.startIndex) {
        return rows.slice(
          0,
          Math.min(rowCount, AGENTS_SIDEBAR_MAX_VISIBLE_SESSION_ROWS)
        );
      }
      const startIndex = Math.max(0, range.startIndex);
      const endIndex = Math.min(rowCount - 1, range.endIndex);
      if (endIndex < startIndex) {
        return [];
      }
      return rows.slice(startIndex, endIndex + 1);
    },
    [rowCount, rows]
  );
  const handleRangeChanged = useCallback(
    (range: ListRange) => {
      latestVisibleRangeRef.current = range;
      onVisibleRowsChange?.(visibleRowsForRange(range));
    },
    [onVisibleRowsChange, visibleRowsForRange]
  );

  useEffect(() => {
    onVisibleRowsChange?.(visibleRowsForRange(latestVisibleRangeRef.current));
  }, [onVisibleRowsChange, visibleRowsForRange]);

  useLayoutEffect(() => {
    latestRowCountRef.current = rowCount;
    latestScrollKeyRef.current = scrollKey;
    if (lastScrollMemoryKeyRef.current !== scrollKey) {
      lastScrollMemoryKeyRef.current = scrollKey;
      lastScrollTopRef.current = initialScrollTop;
    }
  }, [initialScrollTop, rowCount, scrollKey]);

  const saveLatestScrollMemory = useCallback(
    (stateSnapshot?: StateSnapshot) => {
      const latestRowCount = latestRowCountRef.current;
      if (latestRowCount === 0) {
        return;
      }
      rememberAgentSidebarSessionScroll(
        latestScrollKeyRef.current,
        lastScrollTopRef.current,
        {
          rowCount: latestRowCount,
          ...(stateSnapshot ? { stateSnapshot } : {}),
        }
      );
    },
    []
  );
  const captureCurrentScrollTop = useCallback(() => {
    const scrollTop = scrollerRef.current?.scrollTop;
    if (typeof scrollTop !== "number") {
      return;
    }
    if (scrollTop > 0 || lastScrollTopRef.current === 0) {
      lastScrollTopRef.current = scrollTop;
    }
  }, []);

  const reportViewportRowCapacity = useCallback(() => {
    if (!fillAvailableHeight || !onViewportRowCapacityChange) {
      return;
    }
    const viewportPx = scrollerRef.current?.clientHeight ?? 0;
    if (viewportPx <= 0 || rowHeight <= 0) {
      return;
    }
    onViewportRowCapacityChange(Math.ceil(viewportPx / rowHeight));
  }, [fillAvailableHeight, onViewportRowCapacityChange, rowHeight]);

  const fetchNextPageIfNeeded = useCallback(() => {
    if (!hasNextPage || isFetchingNextPage) {
      return;
    }
    if (nextPageRequestRowCountRef.current === rowCount) {
      return;
    }
    const requestId = nextPageRequestIdRef.current + 1;
    nextPageRequestIdRef.current = requestId;
    nextPageRequestRowCountRef.current = rowCount;
    const clearRequestIfStillCurrent = () => {
      if (
        nextPageRequestIdRef.current === requestId &&
        nextPageRequestRowCountRef.current === rowCount &&
        latestRowCountRef.current <= rowCount
      ) {
        nextPageRequestRowCountRef.current = null;
      }
    };
    void Promise.resolve(fetchNextPage())
      .catch(clearRequestIfStillCurrent)
      .finally(() => {
        afterSidebarControlPaint(clearRequestIfStillCurrent);
      });
  }, [fetchNextPage, hasNextPage, isFetchingNextPage, rowCount]);
  const fetchNextPageFromScrollPosition = useCallback(() => {
    const scroller = scrollerRef.current;
    if (!scroller || scroller.clientHeight <= 0 || scroller.scrollHeight <= 0) {
      return;
    }
    if (scroller.scrollHeight <= scroller.clientHeight) {
      return;
    }

    const distanceFromBottom =
      scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
    if (distanceFromBottom <= rowHeight * 2) {
      fetchNextPageIfNeeded();
    }
  }, [fetchNextPageIfNeeded, rowHeight]);

  useEffect(() => {
    if (
      nextPageRequestRowCountRef.current !== null &&
      rowCount > nextPageRequestRowCountRef.current
    ) {
      nextPageRequestIdRef.current += 1;
      nextPageRequestRowCountRef.current = null;
    }
  }, [rowCount]);

  useEffect(() => {
    nextPageRequestIdRef.current += 1;
    nextPageRequestRowCountRef.current = null;
    underflowFetchKeyRef.current = null;
  }, [scrollKey]);

  const handleScrollerRef = useCallback((node: HTMLElement | Window | null) => {
    const element = node instanceof HTMLElement ? node : null;
    if (scrollerRef.current === element) {
      return;
    }
    if (scrollerRef.current) {
      const previousScrollTop = scrollerRef.current.scrollTop;
      if (previousScrollTop > 0 || lastScrollTopRef.current === 0) {
        lastScrollTopRef.current = previousScrollTop;
      }
    }
    scrollerRef.current = element;
    if (element) {
      const nextScrollTop = element.scrollTop;
      if (nextScrollTop > 0 || lastScrollTopRef.current === 0) {
        lastScrollTopRef.current = nextScrollTop;
      }
    }
    setScrollerVersion((version) => version + 1);
  }, []);
  const handleMeasuredRowRef = useCallback((node: HTMLDivElement | null) => {
    rowResizeObserverRef.current?.disconnect();
    rowResizeObserverRef.current = null;

    if (!node) {
      return;
    }

    const updateRowHeight = () => {
      const measuredHeight = node.getBoundingClientRect().height || node.offsetHeight;
      if (measuredHeight <= 0) {
        return;
      }
      const nextHeight = Math.ceil(measuredHeight);
      setMeasuredRowHeight((currentHeight) =>
        currentHeight === nextHeight ? currentHeight : nextHeight
      );
    };

    updateRowHeight();

    if (typeof ResizeObserver === "undefined") {
      return;
    }

    const observer = new ResizeObserver(updateRowHeight);
    observer.observe(node);
    rowResizeObserverRef.current = observer;
  }, []);

  const handleEndReached = useCallback(
    (index: number) => {
      if (index >= rowCount - 1) {
        fetchNextPageIfNeeded();
      }
    },
    [fetchNextPageIfNeeded, rowCount]
  );
  const computeItemKey = useCallback(
    (_: number, row: T) => getItemKey(row),
    [getItemKey]
  );
  const renderItemContent = useCallback(
    (index: number, row: T) => (
      <div
        ref={index === 0 ? handleMeasuredRowRef : undefined}
        className="pb-0.5"
        data-agent-sidebar-row-slot="true"
      >
        {renderRow(row)}
      </div>
    ),
    [handleMeasuredRowRef, renderRow]
  );

  useEffect(() => {
    return () => {
      rowResizeObserverRef.current?.disconnect();
      rowResizeObserverRef.current = null;
      viewportResizeObserverRef.current?.disconnect();
      viewportResizeObserverRef.current = null;
    };
  }, []);

  useLayoutEffect(() => {
    viewportResizeObserverRef.current?.disconnect();
    viewportResizeObserverRef.current = null;
    if (!fillAvailableHeight) {
      return;
    }

    const scroller = scrollerRef.current;
    reportViewportRowCapacity();
    if (!scroller || typeof ResizeObserver === "undefined") {
      return;
    }

    const observer = new ResizeObserver(reportViewportRowCapacity);
    observer.observe(scroller);
    viewportResizeObserverRef.current = observer;
    return () => {
      observer.disconnect();
      if (viewportResizeObserverRef.current === observer) {
        viewportResizeObserverRef.current = null;
      }
    };
  }, [fillAvailableHeight, reportViewportRowCapacity, scrollerVersion]);

  useLayoutEffect(() => {
    return () => {
      captureCurrentScrollTop();
      saveLatestScrollMemory();
    };
  }, [captureCurrentScrollTop, saveLatestScrollMemory]);

  useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    const virtuoso = virtuosoRef.current;
    if (!scroller) {
      return;
    }

    let frameId: number | null = null;
    const saveScroll = () => {
      saveLatestScrollMemory();
    };
    const handleScroll = () => {
      lastScrollTopRef.current = scroller.scrollTop;
      fetchNextPageFromScrollPosition();
      if (typeof window === "undefined") {
        saveScroll();
        return;
      }
      if (frameId !== null) {
        return;
      }
      frameId = window.requestAnimationFrame(() => {
        frameId = null;
        saveScroll();
      });
    };

    scroller.addEventListener("scroll", handleScroll, { passive: true });

    return () => {
      if (frameId !== null && typeof window !== "undefined") {
        window.cancelAnimationFrame(frameId);
      }
      saveLatestScrollMemory();
      if (virtuoso) {
        virtuoso.getState((stateSnapshot) => {
          saveLatestScrollMemory(stateSnapshot);
        });
      }
      scroller.removeEventListener("scroll", handleScroll);
    };
  }, [fetchNextPageFromScrollPosition, saveLatestScrollMemory, scrollerVersion]);

  useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    if (!isVisible || !scroller || rowCount === 0 || scroller.clientHeight <= 0) {
      return;
    }

    if (
      forceRestoreTargetRef.current?.scrollKey === scrollKey &&
      forceRestoreTargetRef.current.scroller === scroller
    ) {
      return;
    }
    forceRestoreTargetRef.current = { scrollKey, scroller };

    const nextScrollTop = Math.max(0, initialScrollTop);
    scroller.scrollTop = nextScrollTop;
    virtuosoRef.current?.scrollTo?.({ top: nextScrollTop });
  }, [initialScrollTop, isVisible, rowCount, scrollKey, scrollerVersion]);

  useEffect(() => {
    if (rowCount === 0 || rowCount >= rememberedRowCount) {
      return;
    }
    fetchNextPageIfNeeded();
  }, [fetchNextPageIfNeeded, rememberedRowCount, rowCount]);

  useEffect(() => {
    if (!hasNextPage || isFetchingNextPage || rowCount === 0) {
      return;
    }

    const scroller = scrollerRef.current;
    if (!scroller || scroller.clientHeight <= 0) {
      return;
    }

    const fetchKey = `${testId}:${rowCount}`;
    if (underflowFetchKeyRef.current === fetchKey) {
      return;
    }

    if (scroller.scrollHeight <= scroller.clientHeight + 1) {
      underflowFetchKeyRef.current = fetchKey;
      fetchNextPageIfNeeded();
    }
  }, [
    fetchNextPageIfNeeded,
    hasNextPage,
    isFetchingNextPage,
    rowCount,
    scrollerVersion,
    testId,
  ]);

  const frameClassName = fillAvailableHeight
    ? "mb-0 mt-1 flex min-h-0 flex-1 flex-col"
    : "mb-2 mt-1";

  if (!isVisible) {
    return <div className={frameClassName} data-testid={`${testId}-frame`} role="group" />;
  }

  if (rowCount === 0) {
    if (!isLoading) {
      return null;
    }

    return (
      <div className="py-1.5 text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
        Loading...
      </div>
    );
  }

  return (
    <div
      data-testid={`${testId}-frame`}
      className={frameClassName}
      role="group"
    >
      <Virtuoso
        ref={virtuosoRef}
        className="agents-sidebar-session-list"
        computeItemKey={computeItemKey}
        data={rows}
        data-testid={testId}
        defaultItemHeight={rowHeight}
        endReached={handleEndReached}
        increaseViewportBy={increaseViewportBy}
        initialScrollTop={initialScrollTop}
        itemContent={renderItemContent}
        rangeChanged={handleRangeChanged}
        {...(restoreStateFrom ? { restoreStateFrom } : {})}
        scrollerRef={handleScrollerRef}
        style={listStyle}
      />
      {isFetchingNextPage && (
        <div
          className="px-2 pt-1 text-right text-[0.6719rem] font-medium"
          style={{ color: "var(--text-muted)" }}
        >
          Loading...
        </div>
      )}
    </div>
  );
}

function areAgentSidebarRowsSameByConversationId(
  left: AgentSidebarConversationRow[],
  right: AgentSidebarConversationRow[]
) {
  if (left.length !== right.length) {
    return false;
  }
  for (let index = 0; index < left.length; index += 1) {
    if (left[index]?.conversation.id !== right[index]?.conversation.id) {
      return false;
    }
  }
  return true;
}

interface AgentsSidebarProps {
  projects: Project[];
  focusedProjectId: string | null;
  selectedConversationId: string | null;
  pinnedConversation?: AgentConversation | null;
  onFocusProject: (projectId: string) => void;
  onSelectConversation: (projectId: string | null, conversation: AgentConversation) => void;
  onCreateAgent: () => void;
  onCreateProject: () => void;
  onArchiveProject: (projectId: string) => void | Promise<void>;
  onAutoRenameConversation: (conversation: AgentConversation) => void | Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: ArchiveConversationHandler;
  onBulkArchiveConversations: BulkArchiveConversationHandler;
  onBulkMuteConversations: (conversationIds: string[]) => void | Promise<void>;
  onSetConversationMuted: (conversation: AgentConversation, muted: boolean) => void | Promise<void>;
  onRestoreConversation: (conversation: AgentConversation) => void;
  onForkConversation: (conversation: AgentConversation) => void | Promise<void>;
  showArchived: boolean;
  onShowArchivedChange: (showArchived: boolean) => void;
  isVisible?: boolean;
  onCollapse?: () => void;
}

export function AgentsSidebar({
  projects,
  focusedProjectId,
  selectedConversationId,
  pinnedConversation = null,
  onFocusProject,
  onSelectConversation,
  onCreateAgent,
  onCreateProject,
  onArchiveProject,
  onAutoRenameConversation,
  onRenameConversation,
  onArchiveConversation,
  onBulkArchiveConversations,
  onBulkMuteConversations,
  onSetConversationMuted,
  onRestoreConversation,
  onForkConversation,
  showArchived,
  onShowArchivedChange,
  isVisible = true,
  onCollapse,
}: AgentsSidebarProps) {
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const handleClearSearch = useCallback(() => setSearchQuery(""), []);
  const [isSearchFocused, setIsSearchFocused] = useState(false);
  const [inboxLaneCounts, setInboxLaneCounts] = useState<
    Record<AgentSidebarAttentionLane, number>
  >({ needs: 0, working: 0, stale: 0, done: 0 });
  // Stable identity: every lane keeps this in an effect dependency list, so an
  // inline callback would re-run all four lane effects on each parent render.
  const handleInboxLaneCountChange = useCallback(
    (lane: AgentSidebarAttentionLane, total: number) => {
      setInboxLaneCounts((current) =>
        current[lane] === total ? current : { ...current, [lane]: total }
      );
    },
    []
  );
  const handleSetConversationMuted = useCallback(
    (conversation: AgentConversation, muted: boolean) =>
      onSetConversationMuted(conversation, muted),
    [onSetConversationMuted]
  );
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const bulkArchive = useBulkConversationArchiveController(
    onBulkArchiveConversations
  );
  const cancelBulkArchive = bulkArchive.cancel;
  const normalizedSearchInput = searchQuery.trim().toLowerCase();
  const normalizedSearch = useDebouncedValue(
    normalizedSearchInput,
    AGENTS_SEARCH_DEBOUNCE_MS
  );
  const showAllProjects = useAgentSessionStore((s) => s.showAllProjects);
  const setShowAllProjects = useAgentSessionStore((s) => s.setShowAllProjects);
  const showEmptyProjectGroups = useAgentSessionStore(
    (s) => s.showEmptyProjectGroups
  );
  const setShowEmptyProjectGroups = useAgentSessionStore(
    (s) => s.setShowEmptyProjectGroups
  );
  const projectSort = useAgentSessionStore((s) => s.projectSort);
  const setProjectSort = useAgentSessionStore((s) => s.setProjectSort);
  const sidebarGroupBy = useAgentSessionStore((s) => s.sidebarGroupBy);
  const setSidebarGroupBy = useAgentSessionStore((s) => s.setSidebarGroupBy);
  const expandedProjectIds = useAgentSessionStore((s) => s.expandedProjectIds);
  const sidebarProjectFilterIds = useAgentSessionStore(
    (s) => s.sidebarProjectFilterIds
  );
  const setSidebarProjectFilterIds = useAgentSessionStore(
    (s) => s.setSidebarProjectFilterIds
  );
  const toggleSidebarProjectFilter = useAgentSessionStore(
    (s) => s.toggleSidebarProjectFilter
  );
  const sidebarPublicationStateFilters = useAgentSessionStore(
    (s) => s.sidebarPublicationStateFilters
  );
  const toggleSidebarPublicationStateFilter = useAgentSessionStore(
    (s) => s.toggleSidebarPublicationStateFilter
  );
  const pinnedConversationIds = useAgentSessionStore((s) => s.pinnedConversationIds);
  const togglePinnedConversation = useAgentSessionStore(
    (s) => s.togglePinnedConversation
  );
  const pinnedConversationIdList = useMemo(
    () => Object.keys(pinnedConversationIds),
    [pinnedConversationIds]
  );
  // Visible project-group selections are already in loaded pages; publication
  // grouping also prioritizes the current selection so state moves keep it visible.
  const [sidebarSelectedConversationIds, setSidebarSelectedConversationIds] = useState<
    Record<string, true>
  >({});
  const handleSelectVisibleConversation = useCallback(
    (projectId: string | null, conversation: AgentConversation) => {
      setSidebarSelectedConversationIds((selectedIds) =>
        selectedIds[conversation.id]
          ? selectedIds
          : { ...selectedIds, [conversation.id]: true }
      );
      onSelectConversation(projectId, conversation);
    },
    [onSelectConversation]
  );
  const handleForkConversation = useCallback(
    async (conversation: AgentConversation) => {
      const confirmed = await confirm({
        title: "Fork session?",
        description:
          "Create a new agent conversation copied from this one. The original conversation will stay unchanged.",
        confirmText: "Fork session",
      });
      if (!confirmed) {
        return;
      }
      await onForkConversation(conversation);
    },
    [confirm, onForkConversation],
  );
  const selectedPriorityConversationIds = useMemo(() => {
    const ids = new Set<string>();
    if (
      pinnedConversation &&
      !pinnedConversationIds[pinnedConversation.id] &&
      !sidebarSelectedConversationIds[pinnedConversation.id]
    ) {
      ids.add(pinnedConversation.id);
    }
    return Array.from(ids);
  }, [
    pinnedConversation,
    pinnedConversationIds,
    sidebarSelectedConversationIds,
  ]);
  const fallbackPriorityProjectId = useMemo(() => {
    const candidateProjectId = pinnedConversation?.projectId;
    if (
      !candidateProjectId ||
      candidateProjectId === NO_PROJECT_GROUP_KEY ||
      candidateProjectId === STANDALONE_AUTOMATION_GROUP_KEY
    ) {
      return null;
    }
    return projects.some((project) => project.id === candidateProjectId)
      ? candidateProjectId
      : null;
  }, [pinnedConversation?.projectId, projects]);
  const selectedProjectFilterIds = useMemo(() => {
    if (showAllProjects) {
      return projects.map((project) => project.id);
    }
    if (sidebarProjectFilterIds.length > 0) {
      return sidebarProjectFilterIds;
    }
    if (focusedProjectId) {
      return [focusedProjectId];
    }
    return projects[0] ? [projects[0].id] : [];
  }, [focusedProjectId, projects, showAllProjects, sidebarProjectFilterIds]);
  const selectedProjectFilterSet = useMemo(
    () => new Set(selectedProjectFilterIds),
    [selectedProjectFilterIds]
  );
  const pinnedProjectId = pinnedConversation?.projectId ?? null;
  const archivedCountProjectIds = useMemo(() => {
    if (selectedProjectFilterIds.length > 0) {
      return selectedProjectFilterIds;
    }

    const projectIds = new Set<string>();
    if (focusedProjectId) {
      projectIds.add(focusedProjectId);
    }
    if (pinnedProjectId) {
      projectIds.add(pinnedProjectId);
    }
    if (projectIds.size === 0 && projects[0]) {
      projectIds.add(projects[0].id);
    }
    return Array.from(projectIds);
  }, [
    focusedProjectId,
    pinnedProjectId,
    projects,
    selectedProjectFilterIds,
  ]);
  const { totalArchivedCount } = useArchivedConversationCounts(archivedCountProjectIds);
  const { data: latestProjectOrder } = useProjectGroupLatestOrder({
    projectIds: selectedProjectFilterIds,
    archivedOnly: showArchived,
    publicationStates: sidebarPublicationStateFilters,
    enabled: sidebarGroupBy === "project" && projectSort === "latest",
  });
  const orderedProjects = useMemo(() => {
    if (projectSort === "latest") {
      const filteredProjects = projects.filter((project) =>
        selectedProjectFilterSet.has(project.id)
      );
      if (latestProjectOrder && latestProjectOrder.length > 0) {
        const orderIndex = new Map(
          latestProjectOrder.map((id, idx) => [id, idx])
        );
        return [...filteredProjects].sort((a, b) => {
          const aIdx = orderIndex.get(a.id) ?? Number.MAX_SAFE_INTEGER;
          const bIdx = orderIndex.get(b.id) ?? Number.MAX_SAFE_INTEGER;
          return aIdx - bIdx;
        });
      }
      return filteredProjects;
    }

    const sortedProjects = [...projects].sort((a, b) =>
      a.name.localeCompare(b.name, undefined, { sensitivity: "base" })
    );
    const nextProjects = projectSort === "za" ? sortedProjects.reverse() : sortedProjects;
    return nextProjects.filter((project) => selectedProjectFilterSet.has(project.id));
  }, [projectSort, projects, selectedProjectFilterSet, latestProjectOrder]);
  const selectedPublicationStates = sidebarPublicationStateFilters;
  const expandedProjectIdForFill = useMemo(() => {
    if (sidebarGroupBy !== "project" || showAllProjects || normalizedSearch.length > 0) {
      return null;
    }
    if (orderedProjects.length === 1) {
      return orderedProjects[0]?.id ?? null;
    }

    const expandedProjects = orderedProjects.filter(
      (project) => expandedProjectIds[project.id] ?? focusedProjectId === project.id
    );
    return expandedProjects.length === 1 ? expandedProjects[0]?.id ?? null : null;
  }, [
    expandedProjectIds,
    focusedProjectId,
    normalizedSearch.length,
    orderedProjects,
    showAllProjects,
    sidebarGroupBy,
  ]);
  // The inbox lane switcher owns the pane's only scroller, so the body stops
  // scrolling and lets the selected lane's list fill the remaining height.
  const fillSidebarBody =
    expandedProjectIdForFill !== null || sidebarGroupBy === "inbox";
  const standaloneGroupQuery = useAgentSidebarProjectGroup({
    projectId: NO_PROJECT_GROUP_KEY,
    archivedOnly: showArchived,
    search: normalizedSearch,
    publicationStates: selectedPublicationStates,
    pinnedConversationIds: pinnedConversationIdList,
    priorityConversationIds:
      pinnedConversation?.projectId === null
        ? selectedPriorityConversationIds
        : [],
    enabled: sidebarGroupBy === "project",
  });

  useEffect(() => {
    if (showArchived) {
      cancelBulkArchive();
    }
  }, [cancelBulkArchive, showArchived]);

  return (
    <AgentConversationMuteContext.Provider value={handleSetConversationMuted}>
    <BulkArchiveSelectionContext.Provider value={bulkArchive.contextValue}>
    <aside
      className="w-full h-full flex flex-col border-r overflow-hidden"
      style={{
        backgroundColor: "var(--app-sidebar-bg)",
        borderRightColor: "var(--app-sidebar-border)",
        borderRightStyle: "solid",
        borderRightWidth: "1px",
        boxShadow: "none",
      }}
      data-testid="agents-sidebar"
    >
      <div
        className="flex shrink-0 items-center gap-3 px-3 pb-2 pt-3"
      >
        <button
          type="button"
          className="inline-flex h-7 items-center gap-1.5 rounded-[6px] border bg-[var(--bg-elevated)] border-[var(--border-subtle)] px-2 pr-2.5 text-[0.7812rem] font-medium text-[var(--text-primary)] transition-[background-color,border-color,color,box-shadow] duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:border-[var(--accent-primary)] hover:shadow-[var(--shadow-glow-accent-soft)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          onClick={onCreateAgent}
          aria-label="New agent"
          data-testid="agents-new-agent"
          style={{
            letterSpacing: "-0.005em",
          }}
        >
          <Plus className="h-[13px] w-[13px]" style={{ color: "var(--text-muted)" }} />
          <span>New</span>
        </button>
        <div className="ml-auto flex items-center gap-1">
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="grid h-7 w-7 place-items-center rounded-[6px] border-0 p-0 transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                onClick={() => {
                  setIsSearchOpen((open) => {
                    if (open) {
                      setSearchQuery("");
                    }
                    return !open;
                  });
                }}
                aria-label={isSearchOpen ? "Close search" : "Search"}
                data-testid="agents-search-toggle"
                style={{ color: "var(--text-muted)", boxShadow: "none" }}
              >
                {isSearchOpen ? <X className="h-3.5 w-3.5" /> : <Search className="h-3.5 w-3.5" />}
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs">
              {isSearchOpen ? "Close search" : "Search"}
            </TooltipContent>
          </Tooltip>
          {onCollapse && (
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  className="grid h-7 w-7 place-items-center rounded-[6px] border-0 p-0 transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
                  onClick={onCollapse}
                  aria-label="Collapse sidebar"
                  data-testid="agents-sidebar-collapse-button"
                  style={{ color: "var(--text-muted)", boxShadow: "none" }}
                >
                  <ChevronLeft className="h-3.5 w-3.5" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="bottom" className="text-xs">
                Collapse sidebar
              </TooltipContent>
            </Tooltip>
          )}
        </div>
      </div>

      {isSearchOpen && (
        <div className="px-3.5 pb-2 shrink-0">
          <div
            className="relative flex items-center"
            style={{
              backgroundColor: "var(--overlay-faint)",
              borderColor: isSearchFocused
                ? "var(--accent-border)"
                : "var(--overlay-weak)",
              borderStyle: "solid",
              borderWidth: "1px",
              borderRadius: "6px",
            }}
          >
            <Search
              className="absolute left-2.5 w-3.5 h-3.5 pointer-events-none"
              style={{ color: "var(--text-muted)" }}
            />
            <input
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              onFocus={() => setIsSearchFocused(true)}
              onBlur={() => setIsSearchFocused(false)}
              placeholder="Search"
              className="w-full h-7 pl-8 pr-8 text-[0.75rem] bg-transparent outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none border-0"
              style={{
                color: "var(--text-primary)",
                caretColor: "var(--accent-primary)",
              }}
              autoFocus
              data-testid="agents-search-input"
              data-agent-sidebar-search="true"
            />
            {searchQuery !== "" && (
              <button
                type="button"
                aria-label="Clear search"
                onClick={() => setSearchQuery("")}
                className="absolute right-2 w-4 h-4 flex items-center justify-center rounded-sm transition-colors duration-100 outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none"
                style={{ color: "var(--text-muted)" }}
              >
                <X className="w-3.5 h-3.5" />
              </button>
            )}
          </div>
        </div>
      )}

      {projects.length > 0 && (
        <AgentsSidebarToolbar
          projects={projects}
          focusedProjectId={focusedProjectId}
          projectSort={projectSort}
          selectedProjectFilterSet={selectedProjectFilterSet}
          selectedPublicationStates={selectedPublicationStates}
          setProjectSort={setProjectSort}
          setShowAllProjects={setShowAllProjects}
          setShowEmptyProjectGroups={setShowEmptyProjectGroups}
          setSidebarGroupBy={setSidebarGroupBy}
          setSidebarProjectFilterIds={setSidebarProjectFilterIds}
          showAllProjects={showAllProjects}
          showEmptyProjectGroups={showEmptyProjectGroups}
          showArchived={showArchived}
          sidebarGroupBy={sidebarGroupBy}
          toggleSidebarProjectFilter={toggleSidebarProjectFilter}
          toggleSidebarPublicationStateFilter={toggleSidebarPublicationStateFilter}
          totalArchivedCount={totalArchivedCount}
          onShowArchivedChange={onShowArchivedChange}
          bulkArchiveActive={bulkArchive.active}
          onEnterBulkArchive={bulkArchive.enter}
        />
      )}

      {bulkArchive.active && (
        <BulkArchiveConversationControls
          confirmationOpen={bulkArchive.confirmationOpen}
          onCancel={bulkArchive.cancel}
          onCloseConfirmation={bulkArchive.closeConfirmation}
          onConfirm={bulkArchive.confirm}
          onOpenConfirmation={bulkArchive.openConfirmation}
          onMute={() => void onBulkMuteConversations(Array.from(bulkArchive.contextValue.selectedIds))}
          pending={bulkArchive.pending}
          selectedCount={bulkArchive.selectedCount}
        />
      )}

      <div
        data-testid="agents-sidebar-body"
        className={
          fillSidebarBody
            ? "flex min-h-0 flex-1 flex-col overflow-hidden px-3 pb-3 pt-0.5"
            : "flex-1 overflow-y-auto px-3 pb-3 pt-0.5"
        }
      >
        {projects.length === 0 && sidebarGroupBy !== "project" ? (
          <div className="h-full px-5 flex flex-col items-center justify-center text-center gap-3">
            <div className="space-y-1">
              <div className="text-sm font-medium" style={{ color: "var(--text-primary)" }}>
                No agent conversations yet.
              </div>
              <div className="text-xs leading-5" style={{ color: "var(--text-muted)" }}>
                Open the starter from the + button to begin a conversation and create a
                project inline if you need one.
              </div>
            </div>
            <Button type="button" size="sm" onClick={onCreateAgent} className="gap-2">
              <Plus className="w-4 h-4" />
              Open starter
            </Button>
          </div>
        ) : sidebarGroupBy === "publication" ? (
          <PublicationStateGroups
            projects={orderedProjects}
            isSidebarVisible={isVisible}
            pinnedConversationIdList={pinnedConversationIdList}
            priorityConversationIds={selectedPriorityConversationIds}
            pinnedConversationIds={pinnedConversationIds}
            rowSort={projectSort}
            selectedConversationId={selectedConversationId}
            searchQuery={normalizedSearch}
            selectedPublicationStates={selectedPublicationStates}
            onArchiveConversation={onArchiveConversation}
            onAutoRenameConversation={onAutoRenameConversation}
            onRenameConversation={onRenameConversation}
            onRestoreConversation={onRestoreConversation}
            onForkConversation={handleForkConversation}
            onSelectConversation={handleSelectVisibleConversation}
            onTogglePinnedConversation={togglePinnedConversation}
            showArchived={showArchived}
          />
        ) : sidebarGroupBy === "inbox" ? (
          <InboxLaneGroups
            projects={orderedProjects}
            isSidebarVisible={isVisible}
            pinnedConversationIdList={pinnedConversationIdList}
            priorityConversationIds={selectedPriorityConversationIds}
            pinnedConversationIds={pinnedConversationIds}
            rowSort={projectSort}
            selectedConversationId={selectedConversationId}
            searchQuery={normalizedSearch}
            selectedPublicationStates={selectedPublicationStates}
            laneCounts={inboxLaneCounts}
            onCreateAgent={onCreateAgent}
            onClearSearch={handleClearSearch}
            onLaneCountChange={handleInboxLaneCountChange}
            onArchiveConversation={onArchiveConversation}
            onAutoRenameConversation={onAutoRenameConversation}
            onRenameConversation={onRenameConversation}
            onRestoreConversation={onRestoreConversation}
            onForkConversation={handleForkConversation}
            onSelectConversation={handleSelectVisibleConversation}
            onTogglePinnedConversation={togglePinnedConversation}
            showArchived={showArchived}
          />
        ) : sidebarGroupBy === "automation" ? (
          <AutomationGroups
            projects={orderedProjects}
            isSidebarVisible={isVisible}
            pinnedConversationIdList={pinnedConversationIdList}
            priorityConversationIds={selectedPriorityConversationIds}
            pinnedConversationIds={pinnedConversationIds}
            rowSort={projectSort}
            selectedConversationId={selectedConversationId}
            searchQuery={normalizedSearch}
            selectedPublicationStates={selectedPublicationStates}
            onArchiveConversation={onArchiveConversation}
            onAutoRenameConversation={onAutoRenameConversation}
            onRenameConversation={onRenameConversation}
            onRestoreConversation={onRestoreConversation}
            onForkConversation={handleForkConversation}
            onSelectConversation={handleSelectVisibleConversation}
            onTogglePinnedConversation={togglePinnedConversation}
            showArchived={showArchived}
          />
        ) : (
          <>
            {orderedProjects.map((project) => (
            <ProjectSessionGroup
              key={project.id}
              project={project}
              isFocused={focusedProjectId === project.id}
              isSidebarVisible={isVisible}
              selectedConversationId={selectedConversationId}
              searchQuery={normalizedSearch}
              onFocusProject={onFocusProject}
              onSelectConversation={handleSelectVisibleConversation}
              onArchiveProject={onArchiveProject}
              onAutoRenameConversation={onAutoRenameConversation}
              onRenameConversation={onRenameConversation}
              onArchiveConversation={onArchiveConversation}
              onRestoreConversation={onRestoreConversation}
              onForkConversation={handleForkConversation}
              onTogglePinnedConversation={togglePinnedConversation}
              pinnedConversationIdList={pinnedConversationIdList}
              priorityConversationIds={
                fallbackPriorityProjectId === project.id
                  ? selectedPriorityConversationIds
                  : []
              }
              pinnedConversationIds={pinnedConversationIds}
              selectedPublicationStates={selectedPublicationStates}
              showArchived={showArchived}
              showEmptyProjectGroups={showEmptyProjectGroups}
              showProjectHeader
              showProjectNameInMeta={false}
              fillAvailableHeight={expandedProjectIdForFill === project.id}
            />
            ))}
            <StandaloneSessionGroup
              groupQuery={standaloneGroupQuery}
              isSidebarVisible={isVisible}
              selectedConversationId={selectedConversationId}
              searchQuery={normalizedSearch}
              onSelectConversation={handleSelectVisibleConversation}
              onAutoRenameConversation={onAutoRenameConversation}
              onRenameConversation={onRenameConversation}
              onArchiveConversation={onArchiveConversation}
              onRestoreConversation={onRestoreConversation}
              onForkConversation={handleForkConversation}
              onTogglePinnedConversation={togglePinnedConversation}
              pinnedConversationIds={pinnedConversationIds}
              showArchived={showArchived}
              showEmptyState={projects.length === 0}
              onCreateAgent={onCreateAgent}
            />
          </>
        )}
      </div>

      <StaticRecentRuns />

      <div
        className="shrink-0 border-t px-3 py-3"
        style={{
          borderTopColor: "var(--app-sidebar-border)",
          borderTopStyle: "solid",
          borderTopWidth: "1px",
        }}
      >
        {sidebarGroupBy === "inbox" && (
          <div
            className="mb-2 text-center text-[0.7188rem]"
            data-testid="agents-inbox-footer"
            style={{ color: "var(--text-muted)" }}
          >
            {summarizeInboxLaneCounts(inboxLaneCounts).footerLabel}
          </div>
        )}
        <button
          type="button"
          onClick={onCreateProject}
          data-testid="agents-add-project"
          className="inline-flex w-full items-center justify-center gap-2 rounded-[6px] border border-dashed border-[var(--border-strong)] bg-transparent px-3 py-2 text-[0.7812rem] font-medium text-[var(--text-muted)] transition-[background-color,border-color,color,box-shadow] duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:border-[var(--accent-primary)] hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] hover:shadow-[var(--shadow-glow-accent-soft)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
        >
          <Plus className="h-[13px] w-[13px]" />
          Add project
        </button>
      </div>
      <ConfirmationDialog {...confirmationDialogProps} />
    </aside>
    </BulkArchiveSelectionContext.Provider>
    </AgentConversationMuteContext.Provider>
  );
}

interface AgentsSidebarToolbarProps {
  bulkArchiveActive: boolean;
  projects: Project[];
  focusedProjectId: string | null;
  projectSort: AgentProjectSort;
  selectedProjectFilterSet: Set<string>;
  selectedPublicationStates: AgentSidebarPublicationState[];
  setProjectSort: (projectSort: AgentProjectSort) => void;
  setShowAllProjects: (showAllProjects: boolean) => void;
  setShowEmptyProjectGroups: (showEmptyProjectGroups: boolean) => void;
  setSidebarGroupBy: (groupBy: AgentSidebarGroupBy) => void;
  setSidebarProjectFilterIds: (projectIds: string[]) => void;
  showAllProjects: boolean;
  showEmptyProjectGroups: boolean;
  showArchived: boolean;
  sidebarGroupBy: AgentSidebarGroupBy;
  toggleSidebarProjectFilter: (projectId: string) => void;
  toggleSidebarPublicationStateFilter: (
    state: AgentSidebarPublicationState
  ) => void;
  totalArchivedCount: number;
  onShowArchivedChange: (showArchived: boolean) => void;
  onEnterBulkArchive: () => void;
}

function AgentsSidebarToolbar({
  bulkArchiveActive,
  projects,
  focusedProjectId,
  projectSort,
  selectedProjectFilterSet,
  selectedPublicationStates,
  setProjectSort,
  setShowAllProjects,
  setShowEmptyProjectGroups,
  setSidebarGroupBy,
  setSidebarProjectFilterIds,
  showAllProjects,
  showEmptyProjectGroups,
  showArchived,
  sidebarGroupBy,
  toggleSidebarProjectFilter,
  toggleSidebarPublicationStateFilter,
  totalArchivedCount,
  onShowArchivedChange,
  onEnterBulkArchive,
}: AgentsSidebarToolbarProps) {
  const sortTarget = SIDEBAR_GROUP_SORT_TARGETS[sidebarGroupBy];
  const ensureScopedProjectSelection = () => {
    if (selectedProjectFilterSet.size > 0) {
      return;
    }
    const fallbackProjectId = focusedProjectId ?? projects[0]?.id;
    if (fallbackProjectId) {
      setSidebarProjectFilterIds([fallbackProjectId]);
    }
  };

  const handleAllProjectsChange = (checked: boolean | "indeterminate") => {
    const nextChecked = checked === true;
    setShowAllProjects(nextChecked);
    if (!nextChecked) {
      ensureScopedProjectSelection();
    }
  };

  const handleProjectFilterChange = (
    projectId: string,
    checked: boolean | "indeterminate"
  ) => {
    if (showAllProjects) {
      setShowAllProjects(false);
      const nextProjectIds = projects
        .map((project) => project.id)
        .filter((candidateProjectId) =>
          checked === true
            ? true
            : candidateProjectId !== projectId
        );
      setSidebarProjectFilterIds(nextProjectIds);
      return;
    }

    toggleSidebarProjectFilter(projectId);
  };

  const handleSortChange = (value: string) => {
    const nextSort = value as AgentProjectSort;
    afterSidebarControlPaint(() => setProjectSort(nextSort));
  };

  const handleGroupChange = (groupBy: AgentSidebarGroupBy) => {
    afterSidebarControlPaint(() => setSidebarGroupBy(groupBy));
  };

  const SidebarGroupIcon = SIDEBAR_GROUP_ICONS[sidebarGroupBy];

  return (
    <div
      className="mb-2 flex h-8 shrink-0 items-center gap-1 px-3"
      role="toolbar"
      aria-label="Agent list filters"
      data-testid="agents-filter-toolbar"
      style={{
        backgroundColor: "var(--bg-surface)",
      }}
    >
      <Popover modal={false}>
        <PopoverTrigger asChild>
          <button
            type="button"
            data-testid="agents-group-trigger"
            aria-label={`Group conversations: ${SIDEBAR_GROUP_LABELS[sidebarGroupBy]}`}
            className="inline-flex h-full min-w-0 items-center gap-1.5 rounded-[4px] border border-transparent bg-transparent px-2 text-[0.7188rem] font-medium text-[var(--text-muted)] transition-colors duration-[120ms] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]"
          >
            <SidebarGroupIcon
              className="h-3.5 w-3.5 shrink-0"
              aria-hidden="true"
            />
            <span className="min-w-0 truncate">
              {SIDEBAR_GROUP_LABELS[sidebarGroupBy]}
            </span>
            <ChevronDown className="h-3 w-3 shrink-0" aria-hidden="true" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className="w-56 px-1.5 py-2.5"
          data-testid="agents-group-popover"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
            boxShadow: "var(--shadow-sm)",
          }}
        >
          <div className="space-y-1 text-xs">
            <FilterSectionLabel>Group by</FilterSectionLabel>
            <div className="grid gap-1" role="radiogroup" aria-label="Group by">
              {(
                [
                  ["inbox", "Inbox"],
                  ["project", "Project"],
                  ["publication", "Publication state"],
                  ["automation", "Automations"],
                ] as const
              ).map(([value, label]) => (
                <button
                  key={value}
                  type="button"
                  role="radio"
                  aria-checked={sidebarGroupBy === value}
                  className="truncate rounded-[4px] px-1.5 py-1 text-left whitespace-nowrap outline-none focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:0px]"
                  onClick={() => handleGroupChange(value)}
                  style={{
                    backgroundColor:
                      sidebarGroupBy === value
                        ? "var(--accent-muted)"
                        : "transparent",
                    color:
                      sidebarGroupBy === value
                        ? "var(--text-primary)"
                        : "var(--text-muted)",
                  }}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
        </PopoverContent>
      </Popover>

      <Popover modal={false}>
        <PopoverTrigger asChild>
          <button
            type="button"
            data-testid="agents-filters-trigger"
            className="inline-flex h-full min-w-0 shrink-0 items-center gap-1.5 rounded-[4px] border border-transparent bg-transparent px-2 text-[0.7188rem] font-medium text-[var(--text-muted)] transition-colors duration-[120ms] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]"
          >
            <span>Filters</span>
            <SlidersHorizontal className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className="w-60 px-1.5 py-2.5"
          data-testid="agents-filter-popover"
          style={{
            backgroundColor: "var(--bg-elevated)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
            boxShadow: "var(--shadow-sm)",
          }}
        >
          <div className="space-y-3 text-xs">
            <div className="space-y-1">
              <FilterSectionLabel>Visibility</FilterSectionLabel>
              <FilterToggleRow
                selected={showArchived}
                onToggle={() => onShowArchivedChange(!showArchived)}
                label="Archived"
                ariaLabel="Show archived conversations"
                testId="agents-filter-archived"
                rightSlot={
                  <span
                    className="rounded-full px-1.5 text-[0.625rem] font-semibold leading-[1.6]"
                    style={{
                      backgroundColor: "var(--overlay-weak)",
                      color: "var(--text-secondary)",
                    }}
                  >
                    {totalArchivedCount}
                  </span>
                }
              />
              <FilterToggleRow
                selected={showEmptyProjectGroups}
                onToggle={() =>
                  setShowEmptyProjectGroups(!showEmptyProjectGroups)
                }
                label="Show empty groups"
                ariaLabel="Show empty groups"
                testId="agents-filter-empty-project-groups"
              />
            </div>

            <FilterCollapsibleSection
              label="Projects"
              testId="agents-filter-projects-section"
              summary={
                showAllProjects
                  ? "All"
                  : `${selectedProjectFilterSet.size}/${projects.length}`
              }
            >
              <div className="max-h-44 space-y-0.5 overflow-y-auto">
                <FilterToggleRow
                  selected={showAllProjects}
                  onToggle={() => handleAllProjectsChange(!showAllProjects)}
                  label="All projects"
                  ariaLabel="All projects"
                  testId="agents-filter-all-projects"
                />
                {projects.map((project) => {
                  const projectSelected =
                    showAllProjects || selectedProjectFilterSet.has(project.id);
                  return (
                    <FilterToggleRow
                      key={project.id}
                      selected={projectSelected}
                      onToggle={() =>
                        handleProjectFilterChange(project.id, !projectSelected)
                      }
                      label={project.name}
                      ariaLabel={`Show ${project.name}`}
                      testId={`agents-filter-project-${project.id}`}
                    />
                  );
                })}
              </div>
            </FilterCollapsibleSection>

            <FilterCollapsibleSection
              label="Publication state"
              testId="agents-filter-publication-section"
              summary={`${selectedPublicationStates.length}/${PUBLICATION_STATE_OPTIONS.length}`}
            >
              <div className="space-y-0.5">
                {PUBLICATION_STATE_OPTIONS.map((option) => (
                  <FilterToggleRow
                    key={option.value}
                    selected={selectedPublicationStates.includes(option.value)}
                    onToggle={() =>
                      toggleSidebarPublicationStateFilter(option.value)
                    }
                    label={option.label}
                    ariaLabel={option.label}
                    testId={`agents-filter-publication-state-${option.value}`}
                  />
                ))}
              </div>
            </FilterCollapsibleSection>
          </div>
        </PopoverContent>
      </Popover>

      <DropdownMenu modal={false}>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            data-testid="agents-sort-trigger"
            aria-label={`Sort ${sortTarget}: ${PROJECT_SORT_LABELS[projectSort]}`}
            className="inline-flex h-full min-w-0 shrink-0 items-center gap-1.5 rounded-[4px] border border-transparent bg-transparent px-2 text-[0.7188rem] font-medium text-[var(--text-muted)] transition-colors duration-[120ms] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px]"
          >
            <span>Sort</span>
            <ArrowDownUp className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="min-w-[120px]">
          <DropdownMenuRadioGroup
            value={projectSort}
            onValueChange={handleSortChange}
          >
            {(["latest", "az", "za"] as AgentProjectSort[]).map((sort) => (
              <DropdownMenuRadioItem key={sort} value={sort} className="text-xs">
                {PROJECT_SORT_LABELS[sort]}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      {!showArchived && (
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="ml-auto grid h-7 w-7 shrink-0 place-items-center rounded-[4px] border border-transparent bg-transparent p-0 text-[var(--text-muted)] transition-colors duration-[120ms] outline-none hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:-2px] disabled:opacity-50"
              aria-label="Bulk archive sessions"
              aria-pressed={bulkArchiveActive}
              data-testid="agents-bulk-archive-trigger"
              disabled={bulkArchiveActive}
              onClick={onEnterBulkArchive}
            >
              <Archive className="h-3.5 w-3.5" aria-hidden="true" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="bottom" className="text-xs">
            Bulk archive sessions
          </TooltipContent>
        </Tooltip>
      )}
    </div>
  );
}

function FilterSectionLabel({
  children,
  inline = false,
}: {
  children: string;
  inline?: boolean;
}) {
  return (
    <div
      className={`text-[0.625rem] font-semibold uppercase leading-none tracking-[0.12em] ${
        inline ? "" : "px-1.5"
      }`}
      style={{ color: "var(--text-muted)" }}
    >
      {children}
    </div>
  );
}

interface FilterToggleRowProps {
  selected: boolean;
  onToggle: () => void;
  label: string;
  ariaLabel: string;
  testId: string;
  rightSlot?: React.ReactNode;
}

function FilterToggleRow({
  selected,
  onToggle,
  label,
  ariaLabel,
  testId,
  rightSlot,
}: FilterToggleRowProps) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={selected}
      aria-label={ariaLabel}
      data-testid={testId}
      onClick={onToggle}
      className="flex w-full min-w-0 items-center justify-between gap-2 rounded-[4px] px-1.5 py-1 text-left text-xs transition-colors duration-[120ms] outline-none hover:bg-[var(--overlay-weak)] focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:0px]"
      style={{
        backgroundColor: "transparent",
        color: selected ? "var(--text-primary)" : "var(--text-muted)",
      }}
    >
      <span className="truncate">{label}</span>
      <span className="inline-flex shrink-0 items-center gap-2">
        {rightSlot}
        <Check
          className="h-3.5 w-3.5"
          aria-hidden="true"
          style={{
            color: selected ? "var(--accent-primary)" : "var(--text-muted)",
            opacity: selected ? 1 : 0.35,
          }}
        />
      </span>
    </button>
  );
}

interface FilterCollapsibleSectionProps {
  label: string;
  summary: string;
  testId: string;
  defaultOpen?: boolean;
  children: React.ReactNode;
}

function FilterCollapsibleSection({
  label,
  summary,
  testId,
  defaultOpen = false,
  children,
}: FilterCollapsibleSectionProps) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <Collapsible open={open} onOpenChange={setOpen} data-testid={testId}>
      <CollapsibleTrigger
        data-testid={`${testId}-trigger`}
        aria-label={`${label} filter`}
        className="flex w-full items-center justify-between gap-2 rounded-[4px] px-1.5 py-1 text-left transition-colors duration-[120ms] outline-none hover:bg-[var(--overlay-weak)] focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:0px]"
      >
        <FilterSectionLabel inline>{label}</FilterSectionLabel>
        <span
          className="inline-flex shrink-0 items-center gap-1.5 text-[0.625rem] font-medium"
          style={{ color: "var(--text-secondary)" }}
        >
          <span>{summary}</span>
          <ChevronDown
            className="h-3 w-3 transition-transform duration-[120ms]"
            aria-hidden="true"
            style={{
              transform: open ? "rotate(180deg)" : "rotate(0deg)",
            }}
          />
        </span>
      </CollapsibleTrigger>
      <CollapsibleContent className="pt-1">{children}</CollapsibleContent>
    </Collapsible>
  );
}

interface AgentSidebarConversationRowsPanelProps {
  rows: AgentSidebarConversationRow[];
  fetchNextPage: () => Promise<unknown>;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  isLoading: boolean;
  expanded: boolean;
  fillAvailableHeight?: boolean;
  inline?: boolean;
  inlineFooter?: ReactNode;
  isSidebarVisible: boolean;
  onViewportRowCapacityChange?: (rowCapacity: number) => void;
  projectById: Map<string, Project>;
  pinnedConversationIds: Record<string, true>;
  scrollKey: string;
  selectedConversationId: string | null;
  showProjectNameInMeta: boolean;
  testId: string;
  inboxLane?: AgentSidebarAttentionLane;
  onSelectConversation: (projectId: string | null, conversation: AgentConversation) => void;
  onAutoRenameConversation: (conversation: AgentConversation) => void | Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: ArchiveConversationHandler;
  onRestoreConversation: (conversation: AgentConversation) => void;
  onForkConversation: (conversation: AgentConversation) => void | Promise<void>;
  onTogglePinnedConversation: (conversationId: string) => void;
}

function AgentSidebarConversationRowsPanel({
  rows,
  fetchNextPage,
  hasNextPage,
  isFetchingNextPage,
  isLoading,
  expanded,
  fillAvailableHeight = false,
  inline = false,
  inlineFooter,
  isSidebarVisible,
  onViewportRowCapacityChange,
  projectById,
  pinnedConversationIds,
  scrollKey,
  selectedConversationId,
  showProjectNameInMeta,
  testId,
  inboxLane,
  onArchiveConversation,
  onAutoRenameConversation,
  onRenameConversation,
  onRestoreConversation,
  onForkConversation,
  onSelectConversation,
  onTogglePinnedConversation,
}: AgentSidebarConversationRowsPanelProps) {
  const activeConversationIds = useChatStore((s) => s.activeConversationIds);
  const agentStatuses = useChatStore((s) => s.agentStatus);
  const agentActivityLabels = useChatStore((s) => s.agentActivityLabels);
  const sessionActionsTriggerRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const [renameDialogConversation, setRenameDialogConversation] =
    useState<AgentConversation | null>(null);
  const [renameDraftTitle, setRenameDraftTitle] = useState("");
  const [autoRenameDialogConversationId, setAutoRenameDialogConversationId] =
    useState<string | null>(null);
  const [archiveDialogTarget, setArchiveDialogTarget] =
    useState<ArchiveConversationDialogTarget | null>(null);
  const [openSessionActionsId, setOpenSessionActionsId] = useState<string | null>(null);
  const [visibleEffectRows, setVisibleEffectRows] = useState<
    AgentSidebarConversationRow[]
  >([]);
  useRegisterBulkArchiveRows(scrollKey, rows);

  const visibleEffectConversations = useMemo(
    () => visibleEffectRows.map((row) => toProjectAgentConversation(row.conversation)),
    [visibleEffectRows]
  );
  useAgentSidebarRunningStates(visibleEffectConversations, isSidebarVisible && expanded);
  const publicationCurrentStates = useMemo(() => {
    const map = new Map<string, string>();
    for (const row of visibleEffectRows) {
      map.set(
        row.conversation.id,
        workspacePublicationFingerprint(
          row.publicationState,
          row.publicationLabel,
        ),
      );
    }
    return map;
  }, [visibleEffectRows]);
  useAgentSidebarPublicationPolling(
    visibleEffectConversations,
    isSidebarVisible && expanded,
    publicationCurrentStates
  );

  const openRenameDialog = useCallback((conversation: AgentConversation) => {
    setRenameDraftTitle(conversation.title || "Untitled agent");
    setRenameDialogConversation(conversation);
  }, []);

  const handleRenameSubmit = useCallback(async () => {
    if (!renameDialogConversation) return;
    const trimmed = renameDraftTitle.trim();
    if (!trimmed) return;
    await onRenameConversation(renameDialogConversation.id, trimmed);
    setRenameDialogConversation(null);
  }, [onRenameConversation, renameDialogConversation, renameDraftTitle]);

  const handleAutoRenameSubmit = useCallback(async () => {
    if (!renameDialogConversation) return;
    setAutoRenameDialogConversationId(renameDialogConversation.id);
    try {
      await onAutoRenameConversation(renameDialogConversation);
      setRenameDialogConversation(null);
    } catch {
      // The owning action reports the failure and the dialog stays open.
    } finally {
      setAutoRenameDialogConversationId(null);
    }
  }, [onAutoRenameConversation, renameDialogConversation]);

  const handleVisibleRowsChange = useCallback((nextRows: AgentSidebarConversationRow[]) => {
    setVisibleEffectRows((currentRows) =>
      areAgentSidebarRowsSameByConversationId(currentRows, nextRows)
        ? currentRows
        : nextRows
    );
  }, []);

  useEffect(() => {
    if (!inline) {
      return;
    }
    handleVisibleRowsChange(rows);
  }, [handleVisibleRowsChange, inline, rows]);

  const getRowKey = useCallback(
    (row: AgentSidebarConversationRow) => row.conversation.id,
    []
  );

  const renderRow = useCallback(
    (row: AgentSidebarConversationRow) => {
      const conversation = toProjectAgentConversation(row.conversation);
      const project = conversation.projectId
        ? projectById.get(conversation.projectId)
        : undefined;
      const rowKey = getAgentConversationStoreKey(conversation);
      const activeConversationId = activeConversationIds[rowKey] ?? null;
      const agentStatus = agentStatuses[rowKey] ?? "idle";
      const runtimeLabel = agentActivityLabels[rowKey] ?? null;
      const isSelected = selectedConversationId === conversation.id;
      const isActiveRuntime = activeConversationId === conversation.id;
      const isPinned = Boolean(pinnedConversationIds[conversation.id]);
      const runtimeState = getSessionRuntimeState(
        conversation,
        isActiveRuntime,
        agentStatus
      );
      const publicationLabel = getVisiblePublicationLabel(
        row.publicationLabel,
        runtimeState,
        runtimeLabel
      );
      const showRuntimeState = shouldShowSessionRuntimeLabel(
        runtimeState,
        publicationLabel
      );
      const sessionActionsOpen = openSessionActionsId === conversation.id;
      const calculatedAgeEscalation = inboxLane
        ? getAgeEscalation(
            conversation.lastMessageAt ?? conversation.updatedAt,
            new Date()
          )
        : undefined;
      const ageEscalation = calculatedAgeEscalation
        ? {
            ...calculatedAgeEscalation,
            tone: shouldEscalateAge(row.attentionLane)
              ? calculatedAgeEscalation.tone
              : "normal" as const,
          }
        : undefined;

      return (
        <MemoizedAgentSessionRow
          conversation={conversation}
          workspace={row.workspace}
          projectName={project?.name ?? conversation.projectId}
          showProjectNameInMeta={showProjectNameInMeta}
          refKind={row.refKind}
          refLabel={row.refLabel}
          publicationState={row.publicationState}
          publicationLabel={publicationLabel}
          parkedDelegateCount={row.parkedDelegateCount}
          {...(inboxLane && row.actionVerb ? { actionVerb: row.actionVerb } : {})}
          {...(inboxLane && ageEscalation ? { ageEscalation } : {})}
          isSelected={isSelected}
          isPinned={isPinned}
          isMuted={row.isMuted}
          runtimeState={runtimeState}
          runtimeLabel={runtimeLabel}
          showRuntimeState={showRuntimeState}
          sessionActionsOpen={sessionActionsOpen}
          onSelect={() => onSelectConversation(conversation.projectId, conversation)}
          onRename={() => openRenameDialog(conversation)}
          onTogglePinned={() => onTogglePinnedConversation(conversation.id)}
          onFork={() => onForkConversation(conversation)}
          onRestore={() => onRestoreConversation(conversation)}
          onArchiveRequest={() =>
            setArchiveDialogTarget({ conversation, workspace: row.workspace })
          }
          setActionsTriggerRef={(node) => {
            sessionActionsTriggerRefs.current[conversation.id] = node;
          }}
          onActionsOpenChange={(open) => {
            setOpenSessionActionsId(open ? conversation.id : null);
            if (!open) {
              requestAnimationFrame(() => {
                sessionActionsTriggerRefs.current[conversation.id]?.blur();
              });
            }
          }}
        />
      );
    },
    [
      activeConversationIds,
      agentActivityLabels,
      agentStatuses,
      inboxLane,
      onForkConversation,
      onRestoreConversation,
      onSelectConversation,
      onTogglePinnedConversation,
      openRenameDialog,
      openSessionActionsId,
      pinnedConversationIds,
      projectById,
      selectedConversationId,
      showProjectNameInMeta,
    ]
  );

  const isAutoRenamingDialog =
    renameDialogConversation !== null &&
    autoRenameDialogConversationId === renameDialogConversation.id;

  return (
    <>
      <Dialog
        open={renameDialogConversation !== null}
        onOpenChange={(open) => {
          if (!open) {
            setRenameDialogConversation(null);
          }
        }}
      >
        <DialogContent hideCloseButton className="max-w-md">
          <DialogHeader className="block space-y-1.5">
            <DialogTitle className="text-base">Rename session</DialogTitle>
            <DialogDescription>
              Update the title shown in the Agents sidebar for this conversation.
            </DialogDescription>
          </DialogHeader>
          <div className="px-6 py-4">
            <Input
              value={renameDraftTitle}
              onChange={(event) => setRenameDraftTitle(event.target.value)}
              aria-label="Session title"
              placeholder="Untitled agent"
              autoFocus
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void handleRenameSubmit();
                }
              }}
            />
          </div>
          <DialogFooter className="justify-between">
            <Button
              type="button"
              variant="secondary"
              className="mr-auto"
              onClick={() => void handleAutoRenameSubmit()}
              disabled={isAutoRenamingDialog}
            >
              <Sparkles className="h-4 w-4" aria-hidden="true" />
              {isAutoRenamingDialog ? "Starting..." : "Auto rename"}
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={() => setRenameDialogConversation(null)}
              disabled={isAutoRenamingDialog}
            >
              Cancel
            </Button>
            <Button
              type="button"
              onClick={() => void handleRenameSubmit()}
              disabled={isAutoRenamingDialog || renameDraftTitle.trim().length === 0}
            >
              Rename session
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ArchiveConversationDialog
        target={archiveDialogTarget}
        onClose={() => setArchiveDialogTarget(null)}
        onArchive={(conversation, options) => {
          void onArchiveConversation(conversation, options);
        }}
      />

      {expanded && (inline ? (
        <div role="group" data-testid={testId}>
          {rows.map((row) => (
            <div key={getRowKey(row)}>{renderRow(row)}</div>
          ))}
          {inlineFooter}
        </div>
      ) : (
        <ScrollableAgentSessionList
          fetchNextPage={fetchNextPage}
          fillAvailableHeight={fillAvailableHeight}
          getItemKey={getRowKey}
          hasNextPage={hasNextPage}
          isFetchingNextPage={isFetchingNextPage}
          isLoading={isLoading}
          isVisible={isSidebarVisible}
          {...(onViewportRowCapacityChange ? { onViewportRowCapacityChange } : {})}
          onVisibleRowsChange={handleVisibleRowsChange}
          renderRow={renderRow}
          rows={rows}
          scrollKey={scrollKey}
          testId={testId}
        />
      ))}
    </>
  );
}

interface PublicationStateGroupsProps {
  projects: Project[];
  isSidebarVisible: boolean;
  pinnedConversationIdList: string[];
  priorityConversationIds: string[];
  pinnedConversationIds: Record<string, true>;
  rowSort: AgentProjectSort;
  selectedConversationId: string | null;
  searchQuery: string;
  selectedPublicationStates: AgentSidebarPublicationState[];
  onSelectConversation: (projectId: string | null, conversation: AgentConversation) => void;
  onAutoRenameConversation: (conversation: AgentConversation) => void | Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: ArchiveConversationHandler;
  onRestoreConversation: (conversation: AgentConversation) => void;
  onForkConversation: (conversation: AgentConversation) => void | Promise<void>;
  onTogglePinnedConversation: (conversationId: string) => void;
  showArchived: boolean;
}

function PublicationStateGroups({
  projects,
  isSidebarVisible,
  pinnedConversationIdList,
  priorityConversationIds,
  pinnedConversationIds,
  rowSort,
  selectedConversationId,
  searchQuery,
  selectedPublicationStates,
  onArchiveConversation,
  onAutoRenameConversation,
  onRenameConversation,
  onRestoreConversation,
  onForkConversation,
  onSelectConversation,
  onTogglePinnedConversation,
  showArchived,
}: PublicationStateGroupsProps) {
  const [expandedPublicationState, setExpandedPublicationState] =
    useState<AgentSidebarPublicationState | null>(() => selectedPublicationStates[0] ?? null);
  const handleSelectedConversationPublicationState = useCallback(
    (publicationState: AgentSidebarPublicationState) => {
      setExpandedPublicationState((current) =>
        current === publicationState ? current : publicationState
      );
    },
    []
  );

  useEffect(() => {
    if (selectedPublicationStates.length === 0) {
      setExpandedPublicationState(null);
      return;
    }
    if (
      expandedPublicationState !== null &&
      !selectedPublicationStates.includes(expandedPublicationState)
    ) {
      setExpandedPublicationState(selectedPublicationStates[0] ?? null);
    }
  }, [expandedPublicationState, selectedPublicationStates]);

  return (
    <>
      {selectedPublicationStates.map((publicationState) => (
        <PublicationStateGroup
          key={publicationState}
          expandedPublicationState={expandedPublicationState}
          isSidebarVisible={isSidebarVisible}
          projects={projects}
          pinnedConversationIdList={pinnedConversationIdList}
          priorityConversationIds={priorityConversationIds}
          pinnedConversationIds={pinnedConversationIds}
          publicationState={publicationState}
          rowSort={rowSort}
          searchQuery={searchQuery}
          selectedConversationId={selectedConversationId}
          showArchived={showArchived}
          onArchiveConversation={onArchiveConversation}
          onAutoRenameConversation={onAutoRenameConversation}
          onRenameConversation={onRenameConversation}
          onRestoreConversation={onRestoreConversation}
          onForkConversation={onForkConversation}
          onSelectConversation={onSelectConversation}
          onTogglePinnedConversation={onTogglePinnedConversation}
          onTogglePublicationState={(state, expanded) =>
            setExpandedPublicationState(expanded ? state : null)
          }
          onSelectedConversationPublicationState={
            handleSelectedConversationPublicationState
          }
        />
      ))}
    </>
  );
}

interface PublicationStateGroupProps {
  expandedPublicationState: AgentSidebarPublicationState | null;
  isSidebarVisible: boolean;
  projects: Project[];
  pinnedConversationIdList: string[];
  priorityConversationIds: string[];
  pinnedConversationIds: Record<string, true>;
  publicationState: AgentSidebarPublicationState;
  rowSort: AgentProjectSort;
  searchQuery: string;
  selectedConversationId: string | null;
  showArchived: boolean;
  onSelectConversation: (projectId: string | null, conversation: AgentConversation) => void;
  onAutoRenameConversation: (conversation: AgentConversation) => void | Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: ArchiveConversationHandler;
  onRestoreConversation: (conversation: AgentConversation) => void;
  onForkConversation: (conversation: AgentConversation) => void | Promise<void>;
  onTogglePinnedConversation: (conversationId: string) => void;
  onTogglePublicationState: (
    publicationState: AgentSidebarPublicationState,
    expanded: boolean,
  ) => void;
  onSelectedConversationPublicationState: (
    publicationState: AgentSidebarPublicationState
  ) => void;
}

function PublicationStateGroup({
  expandedPublicationState,
  isSidebarVisible,
  projects,
  pinnedConversationIdList,
  priorityConversationIds,
  pinnedConversationIds,
  publicationState,
  rowSort,
  searchQuery,
  selectedConversationId,
  showArchived,
  onArchiveConversation,
  onAutoRenameConversation,
  onRenameConversation,
  onRestoreConversation,
  onForkConversation,
  onSelectConversation,
  onTogglePinnedConversation,
  onTogglePublicationState,
  onSelectedConversationPublicationState,
}: PublicationStateGroupProps) {
  const projectIds = useMemo(() => projects.map((project) => project.id), [projects]);
  const projectById = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects]
  );
  const publicationScrollKey = useMemo(
    () =>
      [
        "publication",
        publicationState,
        showArchived ? "archived" : "active",
        searchQuery,
        rowSort,
        projectIds.join(","),
        pinnedConversationIdList.join(","),
      ].join("::"),
    [
      pinnedConversationIdList,
      projectIds,
      publicationState,
      rowSort,
      searchQuery,
      showArchived,
    ]
  );
  const rememberedPublicationRowCount =
    useRememberedAgentSidebarSessionRowCount(publicationScrollKey);
  const groupQuery = useAgentSidebarPublicationGroup({
    projectIds,
    publicationState,
    archivedOnly: showArchived,
    search: searchQuery,
    pinnedConversationIds: pinnedConversationIdList,
    priorityConversationIds,
    sort: rowSort,
    minimumRowCount: rememberedPublicationRowCount,
  });
  const isCurrentPublicationState = expandedPublicationState === publicationState;
  const expanded = searchQuery.length > 0 ? true : isCurrentPublicationState;
  const groupLabel =
    groupQuery.group.label || getSidebarPublicationGroupLabel(publicationState);
  const totalConversationCount = groupQuery.group.total;
  const selectedConversationInGroup = useMemo(
    () =>
      selectedConversationId !== null &&
      groupQuery.group.rows.some(
        (row) => row.conversation.id === selectedConversationId
      ),
    [groupQuery.group.rows, selectedConversationId]
  );
  useEffect(() => {
    if (selectedConversationInGroup) {
      onSelectedConversationPublicationState(publicationState);
    }
  }, [
    onSelectedConversationPublicationState,
    publicationState,
    selectedConversationInGroup,
  ]);

  return (
    <div
      className="my-1 flex flex-col gap-0.5"
      data-testid={`agents-publication-group-${publicationState}`}
    >
      <div className="group/publication-row relative">
        <button
          type="button"
          className="agents-project-row grid w-full grid-cols-[12px_14px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-[6px] px-2 py-1.5 text-left text-[0.8438rem] transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-elevated)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          data-testid={`agents-publication-row-${publicationState}`}
          aria-expanded={expanded}
          aria-current={isCurrentPublicationState ? "true" : undefined}
          aria-label={`${expanded ? "Collapse" : "Expand"} publication state ${groupLabel}`}
          onClick={() => onTogglePublicationState(publicationState, !expanded)}
        >
          <span
            className="agents-project-chevron grid h-3 w-3 place-items-center rounded"
            aria-hidden="true"
          >
            <ChevronRight
              className={`h-2.5 w-2.5 transition-transform duration-[120ms] ${expanded ? "rotate-90" : ""}`}
              strokeWidth={2}
            />
          </span>
          <PublicationStateGroupIcon state={publicationState} />
          <span className="min-w-0 truncate">{groupLabel}</span>
          <span className="agents-project-count agents-publication-count grid min-w-[18px] place-items-center rounded-full border px-1.5 text-[0.6562rem] leading-[1.6]">
            {totalConversationCount}
          </span>
        </button>
      </div>

      <AgentSidebarConversationRowsPanel
        rows={groupQuery.group.rows}
        fetchNextPage={groupQuery.fetchNextPage}
        hasNextPage={Boolean(groupQuery.hasNextPage)}
        isFetchingNextPage={Boolean(groupQuery.isFetchingNextPage)}
        isLoading={Boolean(groupQuery.isLoading)}
        expanded={expanded}
        isSidebarVisible={isSidebarVisible}
        projectById={projectById}
        pinnedConversationIds={pinnedConversationIds}
        scrollKey={publicationScrollKey}
        selectedConversationId={selectedConversationId}
        showProjectNameInMeta
        testId={`agents-sidebar-session-list-publication-${publicationState}`}
        onArchiveConversation={onArchiveConversation}
        onAutoRenameConversation={onAutoRenameConversation}
        onRenameConversation={onRenameConversation}
        onRestoreConversation={onRestoreConversation}
        onForkConversation={onForkConversation}
        onSelectConversation={onSelectConversation}
        onTogglePinnedConversation={onTogglePinnedConversation}
      />
    </div>
  );
}

function hasActiveInboxFilters(
  searchQuery: string,
  selectedPublicationStates: AgentSidebarPublicationState[]
): boolean {
  return (
    searchQuery.length > 0 ||
    selectedPublicationStates.length !== PUBLICATION_STATE_OPTIONS.length
  );
}

interface InboxLaneGroupsProps extends PublicationStateGroupsProps {
  laneCounts: Record<AgentSidebarAttentionLane, number>;
  onCreateAgent: () => void;
  onClearSearch: () => void;
  onLaneCountChange: (lane: AgentSidebarAttentionLane, total: number) => void;
}

function InboxLaneGroups({
  laneCounts,
  onCreateAgent,
  onClearSearch,
  onLaneCountChange,
  ...props
}: InboxLaneGroupsProps) {
  const storedActiveLane = useAgentSessionStore((s) => s.sidebarInboxActiveLane);
  const setSidebarInboxActiveLane = useAgentSessionStore(
    (s) => s.setSidebarInboxActiveLane
  );
  // Switching lanes repaints synchronously; the persist write waits for that
  // paint so a click is never blocked by serializing the sidebar store.
  const [activeLane, setActiveLane] = useState(storedActiveLane);
  const chipRefs = useRef<Partial<Record<AgentSidebarInboxFilter, HTMLButtonElement | null>>>(
    {}
  );

  useEffect(() => {
    setActiveLane(storedActiveLane);
  }, [storedActiveLane]);

  const selectLane = useCallback(
    (lane: AgentSidebarInboxFilter) => {
      setActiveLane(lane);
      afterSidebarControlPaint(() => setSidebarInboxActiveLane(lane));
    },
    [setSidebarInboxActiveLane]
  );

  const handleChipKeyDown = useCallback(
    (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
      const delta =
        event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
      if (delta === 0) {
        return;
      }
      event.preventDefault();
      const laneCount = AGENT_SIDEBAR_INBOX_FILTERS.length;
      const nextIndex = (index + delta + laneCount) % laneCount;
      const nextLane = AGENT_SIDEBAR_INBOX_FILTERS[nextIndex]?.filter;
      if (!nextLane) {
        return;
      }
      selectLane(nextLane);
      chipRefs.current[nextLane]?.focus();
    },
    [selectLane]
  );

  const activeLaneDescriptor =
    AGENT_SIDEBAR_INBOX_FILTERS.find(({ filter }) => filter === activeLane) ??
    AGENT_SIDEBAR_INBOX_FILTERS[0];
  const resolvedActiveLane = activeLaneDescriptor?.filter ?? "recent";

  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-testid="agents-inbox-lane-switcher"
    >
      <div
        role="tablist"
        aria-label="Inbox lanes"
        aria-orientation="horizontal"
        className="mb-1 flex shrink-0 items-center gap-1"
        data-testid="agents-inbox-lane-chips"
      >
        {AGENT_SIDEBAR_INBOX_FILTERS.map(({ filter, label }, index) => {
          const count = filter === "recent"
            ? laneCounts.needs + laneCounts.working
            : laneCounts[laneForInboxFilter(filter)!];
          const isActive = filter === resolvedActiveLane;
          return (
            <button
              key={filter}
              ref={(node) => {
                chipRefs.current[filter] = node;
              }}
              type="button"
              role="tab"
              id={`agents-inbox-lane-chip-${filter}`}
              aria-selected={isActive}
              aria-controls="agents-inbox-lane-panel"
              aria-label={describeInboxLaneCount(label, count)}
              tabIndex={isActive ? 0 : -1}
              title={describeInboxLaneCount(label, count)}
              data-testid={`agents-inbox-lane-chip-${filter}`}
              onClick={() => selectLane(filter)}
              onKeyDown={(event) => handleChipKeyDown(event, index)}
              // Content-sized rather than equal-width: at 268px an even split
              // truncates "Needs you" even when the row has room to spare.
              className="agents-inbox-lane-chip inline-flex min-w-0 items-center justify-center gap-1 rounded-[6px] px-1.5 py-1 text-[0.6875rem] font-medium transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:1px]"
              style={{
                backgroundColor: isActive
                  ? "var(--accent-muted)"
                  : "var(--bg-surface)",
                borderColor: isActive
                  ? "var(--accent-border)"
                  : "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: isActive ? "var(--text-primary)" : "var(--text-muted)",
              }}
            >
              <span className="min-w-0 truncate">{label}</span>
              <span
                aria-hidden="true"
                className="agents-inbox-lane-chip-count shrink-0 text-[0.625rem] tabular-nums"
              >
                {formatInboxLaneCount(count)}
              </span>
            </button>
          );
        })}
      </div>

      <div
        id="agents-inbox-lane-panel"
        role="tabpanel"
        aria-labelledby={`agents-inbox-lane-chip-${resolvedActiveLane}`}
        className="flex min-h-0 flex-1 flex-col"
        data-testid={`agents-inbox-lane-panel-${resolvedActiveLane}`}
      >
        <RecentInboxLane
          isActive={resolvedActiveLane === "recent"}
          doneCount={laneCounts.done}
          onClearSearch={onClearSearch}
          onCreateAgent={onCreateAgent}
          onLaneCountChange={onLaneCountChange}
          onSelectLane={selectLane}
          {...props}
        />
        {AGENT_SIDEBAR_INBOX_FILTERS.map(({ filter, emptyState }) => {
          const lane = laneForInboxFilter(filter);
          return lane ? (
            <InboxLane
              key={lane}
              lane={lane}
              emptyState={emptyState}
              isActive={filter === resolvedActiveLane}
              onClearSearch={onClearSearch}
              onLaneCountChange={onLaneCountChange}
              onSelectLane={selectLane}
              {...props}
            />
          ) : null;
        })}
      </div>
    </div>
  );
}

interface RecentInboxLaneProps extends PublicationStateGroupsProps {
  doneCount: number;
  isActive: boolean;
  onClearSearch: () => void;
  onCreateAgent: () => void;
  onLaneCountChange: (lane: AgentSidebarAttentionLane, total: number) => void;
  onSelectLane: (lane: AgentSidebarInboxFilter) => void;
}

// Recent is a frontend grouping over the existing `needs` and `working` lane
// queries, so the two queries stay separate and only the presentation merges.
// The scroller moves up here, out of the row panels: two `fillAvailableHeight`
// panels stacked would be two nested scrollers.
function RecentInboxLane({
  doneCount,
  isActive,
  onClearSearch,
  onCreateAgent,
  onLaneCountChange,
  onSelectLane,
  projects,
  isSidebarVisible,
  pinnedConversationIdList,
  priorityConversationIds,
  pinnedConversationIds,
  rowSort,
  selectedConversationId,
  searchQuery,
  selectedPublicationStates,
  onArchiveConversation,
  onAutoRenameConversation,
  onRenameConversation,
  onRestoreConversation,
  onForkConversation,
  onSelectConversation,
  onTogglePinnedConversation,
  showArchived,
}: RecentInboxLaneProps) {
  const projectIds = useMemo(() => projects.map((project) => project.id), [projects]);
  const projectById = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects]
  );
  const inboxScrollKey = useMemo(
    () =>
      [
        "inbox",
        "recent",
        showArchived ? "archived" : "active",
        searchQuery,
        rowSort,
        selectedPublicationStates.join(","),
        projectIds.join(","),
        pinnedConversationIdList.join(","),
      ].join("::"),
    [
      pinnedConversationIdList,
      projectIds,
      rowSort,
      searchQuery,
      selectedPublicationStates,
      showArchived,
    ]
  );
  // One scroll-memory entry per filter, not per group: Recent restores its
  // offset and how many pages each group had loaded when it was last visible.
  const rememberedNeedsRowCount = useRememberedRecentInboxGroupRowCount(
    inboxScrollKey,
    "needs"
  );
  const rememberedWorkingRowCount = useRememberedRecentInboxGroupRowCount(
    inboxScrollKey,
    "working"
  );
  const needsQuery = useAgentSidebarInboxGroup({
    lane: "needs",
    projectIds,
    archivedOnly: showArchived,
    search: searchQuery,
    publicationStates: selectedPublicationStates,
    pinnedConversationIds: pinnedConversationIdList,
    priorityConversationIds,
    sort: rowSort,
    minimumRowCount: rememberedNeedsRowCount,
  });
  const workingQuery = useAgentSidebarInboxGroup({
    lane: "working",
    projectIds,
    archivedOnly: showArchived,
    search: searchQuery,
    publicationStates: selectedPublicationStates,
    pinnedConversationIds: pinnedConversationIdList,
    priorityConversationIds,
    sort: rowSort,
    minimumRowCount: rememberedWorkingRowCount,
  });
  const scrollerRef = useRef<HTMLDivElement | null>(null);
  const groups = useMemo(
    () => [
      { descriptor: AGENT_SIDEBAR_RECENT_GROUPS[0], query: needsQuery },
      { descriptor: AGENT_SIDEBAR_RECENT_GROUPS[1], query: workingQuery },
    ],
    [needsQuery, workingQuery]
  );
  const needsTotal = needsQuery.group.total;
  const workingTotal = workingQuery.group.total;
  const isNeedsSettled = !needsQuery.isPending && !needsQuery.isLoading;
  const isWorkingSettled = !workingQuery.isPending && !workingQuery.isLoading;
  const needsRowCount = needsQuery.group.rows.length;
  const workingRowCount = workingQuery.group.rows.length;
  const isFiltered = hasActiveInboxFilters(searchQuery, selectedPublicationStates);

  // A lane that has not settled reports no count: a transient 0 would blink the
  // Recent chip, which sums both lanes.
  useEffect(() => {
    if (needsTotal !== 0 || isNeedsSettled) {
      onLaneCountChange("needs", needsTotal);
    }
  }, [isNeedsSettled, needsTotal, onLaneCountChange]);

  useEffect(() => {
    if (workingTotal !== 0 || isWorkingSettled) {
      onLaneCountChange("working", workingTotal);
    }
  }, [isWorkingSettled, onLaneCountChange, workingTotal]);

  // Restore once rows exist — restoring against an empty scroller would clamp
  // the offset back to 0.
  const restoredScrollKeyRef = useRef<string | null>(null);
  const hasRecentRows = needsRowCount + workingRowCount > 0;
  useLayoutEffect(() => {
    const scroller = scrollerRef.current;
    if (!isActive || !scroller || !hasRecentRows) {
      return;
    }
    if (restoredScrollKeyRef.current === inboxScrollKey) {
      return;
    }
    restoredScrollKeyRef.current = inboxScrollKey;
    scroller.scrollTop =
      agentSidebarSessionScrollPositions.get(inboxScrollKey)?.scrollTop ?? 0;
  }, [hasRecentRows, inboxScrollKey, isActive]);

  useEffect(() => {
    const scroller = scrollerRef.current;
    if (!isActive || !scroller) {
      return;
    }
    const save = () =>
      rememberAgentSidebarSessionScroll(inboxScrollKey, scroller.scrollTop, {
        inboxGroupRowCounts: { needs: needsRowCount, working: workingRowCount },
      });
    scroller.addEventListener("scroll", save, { passive: true });
    return () => {
      save();
      scroller.removeEventListener("scroll", save);
    };
  }, [inboxScrollKey, isActive, needsRowCount, workingRowCount]);

  if (!isActive) {
    return null;
  }

  if (
    needsTotal === 0 &&
    workingTotal === 0 &&
    isNeedsSettled &&
    isWorkingSettled
  ) {
    const emptyState = isFiltered
      ? AGENT_SIDEBAR_INBOX_FILTERED_EMPTY
      : AGENT_SIDEBAR_INBOX_FILTERS[0].emptyState;
    return (
      <AgentsInboxZeroCard
        testId="agents-inbox-lane-empty-recent"
        tone={emptyState.tone}
        icon={isFiltered ? SearchX : CheckCheck}
        headline={emptyState.headline}
        subline={emptyState.subline}
        {...(!isFiltered && {
          primaryAction: { label: "New agent", onClick: onCreateAgent },
        })}
        {...(isFiltered && searchQuery.length > 0
          ? { secondaryAction: { label: "Clear search", onClick: onClearSearch } }
          : !isFiltered && doneCount > 0
            ? {
                secondaryAction: {
                  label: `Review ${doneCount} done`,
                  onClick: () => onSelectLane("done"),
                },
              }
            : {})}
      />
    );
  }

  return (
    <div
      ref={scrollerRef}
      className="agents-sidebar-session-list mt-1 flex min-h-0 flex-1 flex-col overflow-x-hidden overflow-y-auto"
      data-testid="agents-sidebar-session-list-inbox-recent"
    >
      {groups.map(({ descriptor, query }, index) => {
        const isSettled = !query.isPending && !query.isLoading;
        const remaining = Math.max(0, query.group.total - query.group.rows.length);
        return (
          <section
            key={descriptor.lane}
            className={`agents-inbox-recent-group flex flex-col gap-0.5${
              index > 0 ? " mt-1.5" : ""
            }`}
            data-testid={`agents-inbox-recent-group-${descriptor.lane}`}
          >
            {/* The sidebar's existing group row with the icon column dropped —
                inbox groups have no icon. Opaque background so rows scrolling
                under a pinned header stay hidden. */}
            <div
              className="agents-project-row sticky top-0 z-[5] grid w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-[7px] rounded-[6px] px-2 py-1.5 text-left text-[0.8438rem]"
              style={{ backgroundColor: "var(--app-sidebar-bg)" }}
              data-testid={`agents-inbox-recent-group-header-${descriptor.lane}`}
            >
              <h3 className="min-w-0 truncate">{descriptor.label}</h3>
              <span
                className="agents-project-count grid min-w-[18px] place-items-center rounded-full px-1.5 text-[0.6562rem] leading-[1.6]"
                style={{ borderStyle: "solid", borderWidth: "1px" }}
              >
                {query.group.total}
              </span>
            </div>
            {query.group.total === 0 && isSettled ? (
              <AgentsInboxGroupEmptyStrip
                testId={`agents-inbox-lane-empty-${descriptor.lane}`}
                label={descriptor.emptyLabel}
              />
            ) : (
              <AgentSidebarConversationRowsPanel
                rows={query.group.rows}
                fetchNextPage={query.fetchNextPage}
                hasNextPage={Boolean(query.hasNextPage)}
                isFetchingNextPage={Boolean(query.isFetchingNextPage)}
                isLoading={Boolean(query.isLoading)}
                expanded
                inline
                inlineFooter={
                  <RecentInboxPager
                    lane={descriptor.lane}
                    remaining={remaining}
                    total={query.group.total}
                    hasNextPage={Boolean(query.hasNextPage)}
                    isFetchingNextPage={Boolean(query.isFetchingNextPage)}
                    onFetchNextPage={query.fetchNextPage}
                  />
                }
                isSidebarVisible={isSidebarVisible}
                projectById={projectById}
                pinnedConversationIds={pinnedConversationIds}
                scrollKey={`${inboxScrollKey}::${descriptor.lane}`}
                selectedConversationId={selectedConversationId}
                showProjectNameInMeta
                testId={`agents-sidebar-session-list-inbox-${descriptor.lane}`}
                inboxLane={descriptor.lane}
                onArchiveConversation={onArchiveConversation}
                onAutoRenameConversation={onAutoRenameConversation}
                onRenameConversation={onRenameConversation}
                onRestoreConversation={onRestoreConversation}
                onForkConversation={onForkConversation}
                onSelectConversation={onSelectConversation}
                onTogglePinnedConversation={onTogglePinnedConversation}
              />
            )}
          </section>
        );
      })}
    </div>
  );
}

function useRememberedRecentInboxGroupRowCount(
  scrollKey: string,
  lane: AgentSidebarAttentionLane
) {
  const getSnapshot = useCallback(
    () =>
      agentSidebarSessionScrollPositions.get(scrollKey)?.inboxGroupRowCounts?.[
        lane
      ] ?? 0,
    [lane, scrollKey]
  );
  return useSyncExternalStore(
    subscribeAgentSidebarSessionScrollMemory,
    getSnapshot,
    () => 0
  );
}

// The one genuinely new element. Inside Recent the pager replaces scroll-driven
// paging: in a shared scroller, auto-loading lets the group you are not looking
// at grow and shove the one you are.
function RecentInboxPager({
  lane,
  remaining,
  total,
  hasNextPage,
  isFetchingNextPage,
  onFetchNextPage,
}: {
  lane: AgentSidebarAttentionLane;
  remaining: number;
  total: number;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  onFetchNextPage: () => Promise<unknown>;
}) {
  if (total === 0) {
    return null;
  }

  // Exhausted keeps the line rather than removing it, so the group's height
  // does not change under the cursor on the last page.
  if (!hasNextPage || remaining <= 0) {
    return (
      <div
        className="px-2.5 pt-1 pb-1.5 text-left text-[0.6875rem] font-normal"
        style={{ color: "var(--text-subtle)" }}
        data-testid={`agents-inbox-recent-pager-${lane}`}
      >
        All {total} shown
      </div>
    );
  }

  return (
    <button
      type="button"
      className="block w-full px-2.5 pt-1 pb-1.5 text-left text-[0.6875rem] font-medium transition-colors duration-[120ms] outline-none hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)]"
      style={{ color: "var(--text-muted)" }}
      data-testid={`agents-inbox-recent-pager-${lane}`}
      onClick={() => void onFetchNextPage()}
      disabled={isFetchingNextPage}
    >
      Load {remaining} older
    </button>
  );
}

interface InboxLaneProps extends PublicationStateGroupsProps {
  lane: AgentSidebarAttentionLane;
  emptyState: AgentSidebarInboxEmptyState;
  isActive: boolean;
  onClearSearch: () => void;
  onLaneCountChange: (lane: AgentSidebarAttentionLane, total: number) => void;
  onSelectLane: (lane: AgentSidebarInboxFilter) => void;
}

// Single-lane filters — Stale and Done. Every lane keeps its own query so the
// chip counts stay live, but only the selected one renders rows, and it renders
// them as the pane's single full-height scroller. Recent's two lanes take the
// grouped path in RecentInboxLane instead.
function InboxLane({
  lane,
  emptyState,
  isActive,
  onClearSearch,
  onLaneCountChange,
  onSelectLane,
  projects,
  isSidebarVisible,
  pinnedConversationIdList,
  priorityConversationIds,
  pinnedConversationIds,
  rowSort,
  selectedConversationId,
  searchQuery,
  selectedPublicationStates,
  onArchiveConversation,
  onAutoRenameConversation,
  onRenameConversation,
  onRestoreConversation,
  onForkConversation,
  onSelectConversation,
  onTogglePinnedConversation,
  showArchived,
}: InboxLaneProps) {
  const [adaptiveLanePageSize, setAdaptiveLanePageSize] = useState(
    AGENTS_SIDEBAR_MAX_VISIBLE_SESSION_ROWS
  );
  const projectIds = useMemo(() => projects.map((project) => project.id), [projects]);
  const projectById = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects]
  );
  const inboxScrollKey = useMemo(
    () =>
      [
        "inbox",
        lane,
        showArchived ? "archived" : "active",
        searchQuery,
        rowSort,
        selectedPublicationStates.join(","),
        projectIds.join(","),
        pinnedConversationIdList.join(","),
      ].join("::"),
    [
      lane,
      pinnedConversationIdList,
      projectIds,
      rowSort,
      searchQuery,
      selectedPublicationStates,
      showArchived,
    ]
  );
  const rememberedInboxRowCount = useRememberedAgentSidebarSessionRowCount(inboxScrollKey);
  const groupQuery = useAgentSidebarInboxGroup({
    lane,
    projectIds,
    archivedOnly: showArchived,
    search: searchQuery,
    publicationStates: selectedPublicationStates,
    pinnedConversationIds: pinnedConversationIdList,
    priorityConversationIds,
    sort: rowSort,
    minimumRowCount: rememberedInboxRowCount,
    ...(isActive ? { pageSize: adaptiveLanePageSize } : {}),
  });
  const totalConversationCount = groupQuery.group.total;
  const isGroupQuerySettled = !groupQuery.isPending && !groupQuery.isLoading;
  const isFiltered = hasActiveInboxFilters(searchQuery, selectedPublicationStates);

  const handleViewportRowCapacityChange = useCallback((rowCapacity: number) => {
    const nextPageSize = Math.min(
      AGENTS_SIDEBAR_ADAPTIVE_MAX_VISIBLE_SESSION_ROWS,
      Math.max(
        AGENTS_SIDEBAR_MAX_VISIBLE_SESSION_ROWS,
        rowCapacity + AGENTS_SIDEBAR_ADAPTIVE_PAGE_OVERSCAN_ROWS
      )
    );
    setAdaptiveLanePageSize((currentPageSize) =>
      currentPageSize === nextPageSize ? currentPageSize : nextPageSize
    );
  }, []);

  useEffect(() => {
    if (totalConversationCount === 0 && !isGroupQuerySettled) {
      return;
    }
    onLaneCountChange(lane, totalConversationCount);
  }, [isGroupQuerySettled, lane, onLaneCountChange, totalConversationCount]);

  if (!isActive) {
    return null;
  }

  if (totalConversationCount === 0 && isGroupQuerySettled) {
    const resolvedEmptyState = isFiltered
      ? AGENT_SIDEBAR_INBOX_FILTERED_EMPTY
      : emptyState;
    const secondaryAction = isFiltered
      ? searchQuery.length > 0
        ? { label: "Clear search", onClick: onClearSearch }
        : undefined
      : { label: "Back to Recent", onClick: () => onSelectLane("recent") };
    return (
      <AgentsInboxZeroCard
        testId={`agents-inbox-lane-empty-${lane}`}
        tone={resolvedEmptyState.tone}
        icon={isFiltered ? SearchX : lane === "stale" ? Clock : Archive}
        headline={resolvedEmptyState.headline}
        subline={resolvedEmptyState.subline}
        {...(secondaryAction ? { secondaryAction } : {})}
      />
    );
  }

  return (
    <AgentSidebarConversationRowsPanel
      rows={groupQuery.group.rows}
      fetchNextPage={groupQuery.fetchNextPage}
      hasNextPage={Boolean(groupQuery.hasNextPage)}
      isFetchingNextPage={Boolean(groupQuery.isFetchingNextPage)}
      isLoading={Boolean(groupQuery.isLoading)}
      expanded
      fillAvailableHeight
      isSidebarVisible={isSidebarVisible}
      onViewportRowCapacityChange={handleViewportRowCapacityChange}
      projectById={projectById}
      pinnedConversationIds={pinnedConversationIds}
      scrollKey={inboxScrollKey}
      selectedConversationId={selectedConversationId}
      showProjectNameInMeta
      testId={`agents-sidebar-session-list-inbox-${lane}`}
      inboxLane={lane}
      onArchiveConversation={onArchiveConversation}
      onAutoRenameConversation={onAutoRenameConversation}
      onRenameConversation={onRenameConversation}
      onRestoreConversation={onRestoreConversation}
      onForkConversation={onForkConversation}
      onSelectConversation={onSelectConversation}
      onTogglePinnedConversation={onTogglePinnedConversation}
    />
  );
}

interface AutomationGroupsProps {
  projects: Project[];
  isSidebarVisible: boolean;
  pinnedConversationIdList: string[];
  priorityConversationIds: string[];
  pinnedConversationIds: Record<string, true>;
  rowSort: AgentProjectSort;
  selectedConversationId: string | null;
  searchQuery: string;
  selectedPublicationStates: AgentSidebarPublicationState[];
  onSelectConversation: (projectId: string | null, conversation: AgentConversation) => void;
  onAutoRenameConversation: (conversation: AgentConversation) => void | Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: ArchiveConversationHandler;
  onRestoreConversation: (conversation: AgentConversation) => void;
  onForkConversation: (conversation: AgentConversation) => void | Promise<void>;
  onTogglePinnedConversation: (conversationId: string) => void;
  showArchived: boolean;
}

function AutomationGroups({
  projects,
  isSidebarVisible,
  pinnedConversationIdList,
  priorityConversationIds,
  pinnedConversationIds,
  rowSort,
  selectedConversationId,
  searchQuery,
  selectedPublicationStates,
  onArchiveConversation,
  onAutoRenameConversation,
  onRenameConversation,
  onRestoreConversation,
  onForkConversation,
  onSelectConversation,
  onTogglePinnedConversation,
  showArchived,
}: AutomationGroupsProps) {
  const projectIds = useMemo(() => projects.map((project) => project.id), [projects]);
  const projectById = useMemo(
    () => new Map(projects.map((project) => [project.id, project])),
    [projects]
  );
  const groupIndexQuery = useAgentSidebarAutomationGroupIndex({
    projectIds,
    archivedOnly: showArchived,
    search: searchQuery,
    publicationStates: selectedPublicationStates,
    pinnedConversationIds: pinnedConversationIdList,
    priorityConversationIds,
    sort: rowSort,
  });
  const groups = useMemo(() => groupIndexQuery.data ?? [], [groupIndexQuery.data]);
  const [expandedAutomationGroup, setExpandedAutomationGroup] = useState<string | null>(
    () => groups[0]?.key ?? null
  );

  useEffect(() => {
    if (groups.length === 0) {
      setExpandedAutomationGroup(null);
      return;
    }
    if (
      expandedAutomationGroup === null ||
      !groups.some((group) => group.key === expandedAutomationGroup)
    ) {
      setExpandedAutomationGroup(groups[0]?.key ?? null);
    }
  }, [expandedAutomationGroup, groups]);

  return (
    <>
      {groups.map((group) => (
        <AutomationGroup
          key={group.key}
          groupKey={group.key}
          groupLabel={group.label}
          groupPreviewRows={group.rows}
          groupTotal={group.total}
          expandedAutomationGroup={expandedAutomationGroup}
          isSidebarVisible={isSidebarVisible}
          projectById={projectById}
          projectIds={projectIds}
          pinnedConversationIdList={pinnedConversationIdList}
          priorityConversationIds={priorityConversationIds}
          pinnedConversationIds={pinnedConversationIds}
          rowSort={rowSort}
          searchQuery={searchQuery}
          selectedConversationId={selectedConversationId}
          selectedPublicationStates={selectedPublicationStates}
          showArchived={showArchived}
          onArchiveConversation={onArchiveConversation}
          onAutoRenameConversation={onAutoRenameConversation}
          onRenameConversation={onRenameConversation}
          onRestoreConversation={onRestoreConversation}
          onForkConversation={onForkConversation}
          onSelectConversation={onSelectConversation}
          onToggleAutomationGroup={(key, expanded) =>
            setExpandedAutomationGroup(expanded ? key : null)
          }
          onSelectedConversationAutomationGroup={setExpandedAutomationGroup}
          onTogglePinnedConversation={onTogglePinnedConversation}
        />
      ))}
    </>
  );
}

interface AutomationGroupProps {
  groupKey: string;
  groupLabel: string;
  groupPreviewRows: AgentSidebarConversationRow[];
  groupTotal: number;
  expandedAutomationGroup: string | null;
  isSidebarVisible: boolean;
  projectById: Map<string, Project>;
  projectIds: string[];
  pinnedConversationIdList: string[];
  priorityConversationIds: string[];
  pinnedConversationIds: Record<string, true>;
  rowSort: AgentProjectSort;
  searchQuery: string;
  selectedConversationId: string | null;
  selectedPublicationStates: AgentSidebarPublicationState[];
  showArchived: boolean;
  onSelectConversation: (projectId: string | null, conversation: AgentConversation) => void;
  onAutoRenameConversation: (conversation: AgentConversation) => void | Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: ArchiveConversationHandler;
  onRestoreConversation: (conversation: AgentConversation) => void;
  onForkConversation: (conversation: AgentConversation) => void | Promise<void>;
  onToggleAutomationGroup: (groupKey: string, expanded: boolean) => void;
  onSelectedConversationAutomationGroup: (groupKey: string) => void;
  onTogglePinnedConversation: (conversationId: string) => void;
}

function AutomationGroup({
  groupKey,
  groupLabel,
  groupPreviewRows,
  groupTotal,
  expandedAutomationGroup,
  isSidebarVisible,
  projectById,
  projectIds,
  pinnedConversationIdList,
  priorityConversationIds,
  pinnedConversationIds,
  rowSort,
  searchQuery,
  selectedConversationId,
  selectedPublicationStates,
  showArchived,
  onArchiveConversation,
  onAutoRenameConversation,
  onRenameConversation,
  onRestoreConversation,
  onForkConversation,
  onSelectConversation,
  onToggleAutomationGroup,
  onSelectedConversationAutomationGroup,
  onTogglePinnedConversation,
}: AutomationGroupProps) {
  const automationScrollKey = useMemo(
    () =>
      [
        "automation",
        groupKey,
        showArchived ? "archived" : "active",
        searchQuery,
        rowSort,
        selectedPublicationStates.join(","),
        projectIds.join(","),
        pinnedConversationIdList.join(","),
      ].join("::"),
    [
      groupKey,
      pinnedConversationIdList,
      projectIds,
      rowSort,
      searchQuery,
      selectedPublicationStates,
      showArchived,
    ]
  );
  const rememberedAutomationRowCount =
    useRememberedAgentSidebarSessionRowCount(automationScrollKey);
  const isCurrentAutomationGroup = expandedAutomationGroup === groupKey;
  const expanded = searchQuery.length > 0 ? true : isCurrentAutomationGroup;
  const selectedConversationInIndexGroup = useMemo(
    () =>
      selectedConversationId !== null &&
      groupPreviewRows.some(
        (row) => row.conversation.id === selectedConversationId
      ),
    [groupPreviewRows, selectedConversationId]
  );
  const shouldLoadGroupRows = expanded || selectedConversationInIndexGroup;
  const groupQuery = useAgentSidebarAutomationGroup({
    groupKey,
    projectIds,
    archivedOnly: showArchived,
    search: searchQuery,
    publicationStates: selectedPublicationStates,
    pinnedConversationIds: pinnedConversationIdList,
    priorityConversationIds,
    sort: rowSort,
    enabled: shouldLoadGroupRows,
    minimumRowCount: rememberedAutomationRowCount,
  });
  const label = groupQuery.group.label || groupLabel || groupKey;
  const totalConversationCount = groupQuery.group.total || groupTotal;
  const selectedConversationInGroup = useMemo(
    () =>
      selectedConversationId !== null &&
      (selectedConversationInIndexGroup ||
        groupQuery.group.rows.some(
          (row) => row.conversation.id === selectedConversationId
        )),
    [
      groupQuery.group.rows,
      selectedConversationId,
      selectedConversationInIndexGroup,
    ]
  );

  useEffect(() => {
    if (selectedConversationInGroup) {
      onSelectedConversationAutomationGroup(groupKey);
    }
  }, [
    groupKey,
    onSelectedConversationAutomationGroup,
    selectedConversationInGroup,
  ]);

  return (
    <div
      className="my-1 flex flex-col gap-0.5"
      data-testid={`agents-automation-group-${groupKey}`}
    >
      <div className="group/automation-row relative">
        <button
          type="button"
          className="agents-project-row grid w-full grid-cols-[12px_14px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-[6px] px-2 py-1.5 text-left text-[0.8438rem] transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-elevated)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          data-testid={`agents-automation-row-${groupKey}`}
          aria-expanded={expanded}
          aria-current={isCurrentAutomationGroup ? "true" : undefined}
          aria-label={`${expanded ? "Collapse" : "Expand"} automation group ${label}`}
          onClick={() => onToggleAutomationGroup(groupKey, !expanded)}
        >
          <span
            className="agents-project-chevron grid h-3 w-3 place-items-center rounded"
            aria-hidden="true"
          >
            <ChevronRight
              className={`h-2.5 w-2.5 transition-transform duration-[120ms] ${expanded ? "rotate-90" : ""}`}
              strokeWidth={2}
            />
          </span>
          <Sparkles
            className="agents-project-icon h-3.5 w-3.5 shrink-0"
            strokeWidth={1.8}
            aria-hidden="true"
          />
          <span
            className="min-w-0 truncate"
            data-testid={`agents-automation-label-${groupKey}`}
          >
            {label}
          </span>
          <span className="agents-project-count agents-automation-count grid min-w-[18px] place-items-center rounded-full border px-1.5 text-[0.6562rem] leading-[1.6]">
            {totalConversationCount}
          </span>
        </button>
      </div>

      <AgentSidebarConversationRowsPanel
        rows={groupQuery.group.rows}
        fetchNextPage={groupQuery.fetchNextPage}
        hasNextPage={Boolean(groupQuery.hasNextPage)}
        isFetchingNextPage={Boolean(groupQuery.isFetchingNextPage)}
        isLoading={Boolean(groupQuery.isLoading)}
        expanded={expanded}
        isSidebarVisible={isSidebarVisible}
        projectById={projectById}
        pinnedConversationIds={pinnedConversationIds}
        scrollKey={automationScrollKey}
        selectedConversationId={selectedConversationId}
        showProjectNameInMeta
        testId={`agents-sidebar-session-list-automation-${groupKey}`}
        onArchiveConversation={onArchiveConversation}
        onAutoRenameConversation={onAutoRenameConversation}
        onRenameConversation={onRenameConversation}
        onRestoreConversation={onRestoreConversation}
        onForkConversation={onForkConversation}
        onSelectConversation={onSelectConversation}
        onTogglePinnedConversation={onTogglePinnedConversation}
      />
    </div>
  );
}

function PublicationStateGroupIcon({
  state,
}: {
  state: AgentSidebarPublicationState;
}) {
  if (state === "active") {
    return (
      <GitBranch
        className="agents-project-icon h-3.5 w-3.5 shrink-0"
        strokeWidth={1.8}
        aria-hidden="true"
      />
    );
  }

  return (
    <GitPullRequest
      className="agents-project-icon h-3.5 w-3.5 shrink-0"
      strokeWidth={1.8}
      aria-hidden="true"
    />
  );
}

interface AgentSessionRowProps {
  conversation: AgentConversation;
  workspace: AgentConversationWorkspace | null;
  projectName: string | null;
  showProjectNameInMeta: boolean;
  refKind: AgentSidebarConversationRow["refKind"];
  refLabel: string;
  publicationState: AgentSidebarPublicationState;
  publicationLabel: string | null;
  parkedDelegateCount: number;
  actionVerb?: string;
  ageEscalation?: AgeEscalation;
  isSelected: boolean;
  isPinned: boolean;
  isMuted: boolean;
  runtimeState: SessionRuntimeState;
  runtimeLabel: string | null;
  showRuntimeState: boolean;
  sessionActionsOpen: boolean;
  onSelect: () => void;
  onRename: () => void;
  onTogglePinned: () => void;
  onFork: () => void;
  onRestore: () => void;
  onArchiveRequest: () => void;
  setActionsTriggerRef: (node: HTMLButtonElement | null) => void;
  onActionsOpenChange: (open: boolean) => void;
}

function AgentSessionRow({
  conversation,
  workspace,
  projectName,
  showProjectNameInMeta,
  refKind,
  refLabel,
  publicationState,
  publicationLabel,
  parkedDelegateCount,
  actionVerb,
  ageEscalation,
  isSelected,
  isPinned,
  isMuted,
  runtimeState,
  runtimeLabel,
  showRuntimeState,
  sessionActionsOpen,
  onSelect,
  onRename,
  onTogglePinned,
  onFork,
  onRestore,
  onArchiveRequest,
  setActionsTriggerRef,
  onActionsOpenChange,
}: AgentSessionRowProps) {
  const bulkArchiveSelection = useBulkArchiveSelection();
  const setConversationMuted = useContext(AgentConversationMuteContext);
  const title = conversation.title || "Untitled agent";
  const modeMeta =
    conversation.agentMode === "persona_builder"
      ? CONVERSATION_MODE_META.persona_builder
      : null;
  const createdLabel = formatAgentConversationCreatedAt(conversation.createdAt);
  const createdTitle = formatAgentConversationCreatedAtTitle(conversation.createdAt);
  const parkedDelegateMeta = formatParkedDelegateMeta(parkedDelegateCount);

  return (
    <div
      className="group/session relative"
      data-testid={`agents-session-${conversation.id}`}
    >
      <BulkArchiveConversationCheckbox
        conversation={conversation}
        workspace={workspace}
      />
      <button
        type="button"
        className={`agents-session-row grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2 rounded-[6px] py-1.5 text-left transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-elevated)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px] ${
          bulkArchiveSelection.active ? "pl-9 pr-2.5" : "px-2.5"
        }`}
        onClick={onSelect}
        aria-current={isSelected ? "true" : undefined}
        style={{
          opacity: conversation.archivedAt ? 0.58 : 1,
          boxShadow: "none",
        }}
      >
        <span className="min-w-0 flex flex-col gap-px">
          <span className="agents-session-title min-w-0 truncate text-[0.8125rem] leading-[1.35]">
            {title}
          </span>
          <span
            className="agents-session-meta flex min-w-0 items-center gap-1 overflow-hidden whitespace-nowrap text-[0.6875rem] leading-[1.35]"
            style={{
              fontFamily: "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
            }}
          >
            {actionVerb && (
              <>
                <span className="shrink-0 font-medium">{actionVerb}</span>
                <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
                  ·
                </span>
              </>
            )}
            {parkedDelegateMeta && (
              <>
                <span
                  className="shrink-0 font-medium"
                  data-testid={`agents-parked-delegates-${conversation.id}`}
                >
                  {parkedDelegateMeta}
                </span>
                <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
                  ·
                </span>
              </>
            )}
            {showProjectNameInMeta && projectName && (
              <>
                <span className="max-w-24 shrink-0 truncate">{projectName}</span>
                <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
                  ·
                </span>
              </>
            )}
            {modeMeta && (
              <span className="inline-flex shrink-0 items-center gap-1">
                <modeMeta.icon
                  className="h-3 w-3"
                  data-testid={`agents-mode-icon-${conversation.agentMode}`}
                  aria-hidden="true"
                />
                <span>{modeMeta.label}</span>
                <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
                  ·
                </span>
              </span>
            )}
            <span className="inline-flex min-w-0 items-center gap-1">
              {refKind === "pull-request" ? (
                <GitPullRequest
                  className="h-3 w-3 shrink-0 -translate-y-px"
                  data-ref-kind="pull-request"
                  data-testid={`agents-ref-icon-${conversation.id}`}
                  aria-hidden="true"
                />
              ) : (
                <GitBranch
                  className="h-3 w-3 shrink-0 -translate-y-px"
                  data-ref-kind="branch"
                  data-testid={`agents-ref-icon-${conversation.id}`}
                  aria-hidden="true"
                />
              )}
              <span className="min-w-0 truncate">{refLabel}</span>
            </span>
            <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
              ·
            </span>
            {workspace?.staleBaseDetectedAt && (
              <>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <span
                      role="img"
                      aria-label="Base branch has moved ahead — this workspace needs an update"
                      className="inline-flex shrink-0 items-center"
                      data-testid={`agents-session-base-ahead-${conversation.id}`}
                    >
                      <GitCompareArrows
                        className="h-3 w-3"
                        style={{ color: "var(--status-warning)" }}
                        aria-hidden="true"
                      />
                    </span>
                  </TooltipTrigger>
                  <TooltipContent side="bottom" className="text-xs">
                    Base branch has moved ahead
                  </TooltipContent>
                </Tooltip>
                <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
                  ·
                </span>
              </>
            )}
            {publicationLabel && (
              <>
                <span
                  className="agents-session-publication-state shrink-0 font-medium"
                  style={{
                    color:
                      publicationState === "merged"
                        ? "var(--status-success)"
                        : publicationState === "closed"
                          ? "var(--text-muted)"
                          : "var(--status-warning)",
                  }}
                >
                  {publicationLabel}
                </span>
                <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
                  ·
                </span>
              </>
            )}
            <span className="shrink-0" title={createdTitle || undefined}>
              {createdLabel}
            </span>
            {showRuntimeState && (
              <>
                <span className="flex h-[1em] shrink-0 items-center" aria-hidden="true">
                  ·
                </span>
                <SessionRuntimeLabel state={runtimeState} label={runtimeLabel} />
              </>
            )}
          </span>
        </span>
        <span className="flex items-center gap-1.5 justify-self-end">
          {ageEscalation?.label && (
            <span
              className="shrink-0 text-[0.6875rem] leading-none"
              data-testid={`agents-session-age-${conversation.id}`}
              style={{
                // --accent-primary is reserved for live/running state, so age
                // escalation uses the status scale instead of the brand accent.
                color:
                  ageEscalation.tone === "alert"
                    ? "var(--status-error)"
                    : ageEscalation.tone === "warn"
                      ? "var(--status-warning)"
                      : "var(--text-muted)",
              }}
            >
              {ageEscalation.label}
            </span>
          )}
          <span
            className={`agents-session-status-slot grid h-4 w-4 place-items-center transition-opacity duration-150 ${
            sessionActionsOpen
              ? "opacity-0"
              : "opacity-100 group-hover/session:opacity-0 group-focus-within/session:opacity-0"
            }`}
          >
            <SessionStatusIcon
              isPinned={isPinned}
              isMuted={isMuted}
              state={runtimeState}
              conversationId={conversation.id}
              selected={isSelected}
            />
          </span>
        </span>
      </button>
      <DropdownMenu modal={false} onOpenChange={onActionsOpenChange}>
        <DropdownMenuTrigger asChild>
          <Button
            ref={setActionsTriggerRef}
            type="button"
            variant="ghost"
            size="sm"
            className="absolute right-2 top-1/2 h-6 w-6 -translate-y-1/2 rounded-[6px] border-0 bg-transparent p-0 opacity-0 outline-none ring-0 transition-opacity hover:bg-transparent focus:bg-transparent focus:outline-none focus:ring-0 focus-visible:bg-transparent focus-visible:outline-none focus-visible:ring-0 group-hover/session:opacity-100 group-focus-within/session:opacity-100 data-[state=open]:bg-transparent data-[state=open]:opacity-100"
            aria-label="Session actions"
            style={{ boxShadow: "none" }}
          >
            <MoreHorizontal className="h-3.5 w-3.5" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          onCloseAutoFocus={(event) => {
            event.preventDefault();
          }}
        >
          <DropdownMenuItem className="gap-2 text-xs" onClick={onRename}>
            <Pencil className="w-3.5 h-3.5" />
            Rename session
          </DropdownMenuItem>
          <DropdownMenuItem className="gap-2 text-xs" onClick={onTogglePinned}>
            {isPinned ? (
              <PinOff className="w-3.5 h-3.5" />
            ) : (
              <Pin className="w-3.5 h-3.5" />
            )}
            {isPinned ? "Unpin session" : "Pin session"}
          </DropdownMenuItem>
          <DropdownMenuItem className="gap-2 text-xs" onClick={onFork}>
            <GitFork className="w-3.5 h-3.5" />
            Fork session
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          {/* Mute and Archive answer the same question — "clear this off my
              list" — so they share the trailing section as two intensities of
              one intent, each stating what it costs. Only Archive is toned, and
              only on its icon: mute reads as the neutral default because it is
              the one of the two that is not marked. */}
          {!conversation.archivedAt && (
            <DropdownMenuItem
              className="items-start gap-2 text-xs"
              onClick={() => void setConversationMuted(conversation, !isMuted)}
            >
              {isMuted ? (
                <Bell className="mt-0.5 h-3.5 w-3.5" />
              ) : (
                <BellOff className="mt-0.5 h-3.5 w-3.5" />
              )}
              <span className="flex flex-col">
                <span>{isMuted ? "Unmute session" : "Mute until it changes"}</span>
                <span className="text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
                  {isMuted
                    ? "Returns it to its normal lane."
                    : "Drops to Stale. Comes back on its next change."}
                </span>
              </span>
            </DropdownMenuItem>
          )}
          {conversation.archivedAt ? (
            <DropdownMenuItem className="gap-2 text-xs" onClick={onRestore}>
              <RotateCcw className="w-3.5 h-3.5" />
              Restore session
            </DropdownMenuItem>
          ) : (
            <DropdownMenuItem className="items-start gap-2 text-xs" onClick={onArchiveRequest}>
              <Archive className="mt-0.5 h-3.5 w-3.5" style={{ color: "var(--destructive)" }} />
              <span className="flex flex-col">
                <span>Archive session</span>
                <span className="text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
                  Deletes the local workspace and branch.
                </span>
              </span>
            </DropdownMenuItem>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

const MemoizedAgentSessionRow = memo(AgentSessionRow);

interface StandaloneSessionGroupProps {
  groupQuery: ReturnType<typeof useAgentSidebarProjectGroup>;
  isSidebarVisible: boolean;
  selectedConversationId: string | null;
  searchQuery: string;
  onSelectConversation: (
    projectId: string | null,
    conversation: AgentConversation,
  ) => void;
  onAutoRenameConversation: (conversation: AgentConversation) => void | Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: ArchiveConversationHandler;
  onRestoreConversation: (conversation: AgentConversation) => void;
  onForkConversation: (conversation: AgentConversation) => void | Promise<void>;
  onTogglePinnedConversation: (conversationId: string) => void;
  pinnedConversationIds: Record<string, true>;
  showArchived: boolean;
  showEmptyState: boolean;
  onCreateAgent: () => void;
}

function StandaloneSessionGroup({
  groupQuery,
  isSidebarVisible,
  selectedConversationId,
  searchQuery,
  onSelectConversation,
  onAutoRenameConversation,
  onRenameConversation,
  onArchiveConversation,
  onRestoreConversation,
  onForkConversation,
  onTogglePinnedConversation,
  pinnedConversationIds,
  showArchived,
  showEmptyState,
  onCreateAgent,
}: StandaloneSessionGroupProps) {
  const expandedProjectIds = useAgentSessionStore((state) => state.expandedProjectIds);
  const setProjectExpanded = useAgentSessionStore((state) => state.setProjectExpanded);
  const expanded = searchQuery.length > 0 || (expandedProjectIds.__no_project__ ?? true);
  const rows = groupQuery.group.rows;

  if (!groupQuery.isLoading && rows.length === 0) {
    if (!showEmptyState) {
      return null;
    }
    return (
      <div className="h-full px-5 flex flex-col items-center justify-center text-center gap-3">
        <div className="space-y-1">
          <div className="text-sm font-medium" style={{ color: "var(--text-primary)" }}>
            No agent conversations yet.
          </div>
          <div className="text-xs leading-5" style={{ color: "var(--text-muted)" }}>
            Open the starter from the + button to begin a conversation.
          </div>
        </div>
        <Button type="button" size="sm" onClick={onCreateAgent} className="gap-2">
          <Plus className="w-4 h-4" />
          Open starter
        </Button>
      </div>
    );
  }

  if (rows.length === 0) {
    return null;
  }

  return (
    <div className="my-1 flex flex-col gap-0.5" data-testid="agents-project-__no_project__">
      <button
        type="button"
        className="agents-project-row grid w-full grid-cols-[12px_14px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-[6px] px-2 py-1.5 text-left text-[0.8438rem] outline-none hover:bg-[var(--bg-elevated)] focus-visible:[outline:2px_solid_var(--border-focus)]"
        aria-expanded={expanded}
        aria-label={`${expanded ? "Collapse" : "Expand"} No project`}
        onClick={() => setProjectExpanded("__no_project__", !expanded)}
        data-testid="agents-project-row-__no_project__"
      >
        <ChevronRight
          className={`h-2.5 w-2.5 transition-transform ${expanded ? "rotate-90" : ""}`}
          aria-hidden="true"
        />
        <CircleOff className="h-3.5 w-3.5" aria-hidden="true" />
        <span>No project</span>
        <span className="agents-project-count grid min-w-[18px] place-items-center rounded-full border px-1.5 text-[0.6562rem] leading-[1.6]">
          {groupQuery.group.total}
        </span>
      </button>
      <AgentSidebarConversationRowsPanel
        rows={rows}
        fetchNextPage={groupQuery.fetchNextPage}
        hasNextPage={Boolean(groupQuery.hasNextPage)}
        isFetchingNextPage={Boolean(groupQuery.isFetchingNextPage)}
        isLoading={Boolean(groupQuery.isLoading)}
        expanded={expanded}
        isSidebarVisible={isSidebarVisible}
        projectById={new Map()}
        pinnedConversationIds={pinnedConversationIds}
        scrollKey={`project::__no_project__::${showArchived ? "archived" : "active"}::${searchQuery}`}
        selectedConversationId={selectedConversationId}
        showProjectNameInMeta={false}
        testId="agents-sidebar-session-list-__no_project__"
        onArchiveConversation={onArchiveConversation}
        onAutoRenameConversation={onAutoRenameConversation}
        onRenameConversation={onRenameConversation}
        onRestoreConversation={onRestoreConversation}
        onForkConversation={onForkConversation}
        onSelectConversation={onSelectConversation}
        onTogglePinnedConversation={onTogglePinnedConversation}
      />
    </div>
  );
}

interface ProjectSessionGroupProps {
  project: Project;
  isFocused: boolean;
  isSidebarVisible: boolean;
  selectedConversationId: string | null;
  searchQuery: string;
  onFocusProject: (projectId: string) => void;
  onSelectConversation: (projectId: string | null, conversation: AgentConversation) => void;
  onArchiveProject: (projectId: string) => void | Promise<void>;
  onAutoRenameConversation: (conversation: AgentConversation) => void | Promise<void>;
  onRenameConversation: (conversationId: string, title: string) => void | Promise<void>;
  onArchiveConversation: ArchiveConversationHandler;
  onRestoreConversation: (conversation: AgentConversation) => void;
  onForkConversation: (conversation: AgentConversation) => void | Promise<void>;
  onTogglePinnedConversation: (conversationId: string) => void;
  pinnedConversationIdList: string[];
  priorityConversationIds: string[];
  pinnedConversationIds: Record<string, true>;
  selectedPublicationStates: AgentSidebarPublicationState[];
  showArchived: boolean;
  showEmptyProjectGroups: boolean;
  showProjectHeader: boolean;
  showProjectNameInMeta: boolean;
  fillAvailableHeight?: boolean;
}

function ProjectSessionGroup({
  project,
  isFocused,
  isSidebarVisible,
  selectedConversationId,
  searchQuery,
  onFocusProject,
  onSelectConversation,
  onArchiveProject,
  onAutoRenameConversation,
  onRenameConversation,
  onArchiveConversation,
  onRestoreConversation,
  onForkConversation,
  onTogglePinnedConversation,
  pinnedConversationIdList,
  priorityConversationIds,
  pinnedConversationIds,
  selectedPublicationStates,
  showArchived,
  showEmptyProjectGroups,
  showProjectHeader,
  showProjectNameInMeta,
  fillAvailableHeight = false,
}: ProjectSessionGroupProps) {
  const projectActionsTriggerRef = useRef<HTMLButtonElement | null>(null);
  const sessionActionsTriggerRefs = useRef<Record<string, HTMLButtonElement | null>>({});
  const [projectActionsOpen, setProjectActionsOpen] = useState(false);
  const [archiveDialogOpen, setArchiveDialogOpen] = useState(false);
  const [prTemplateDialogOpen, setPrTemplateDialogOpen] = useState(false);
  const [renameDialogConversation, setRenameDialogConversation] =
    useState<AgentConversation | null>(null);
  const [renameDraftTitle, setRenameDraftTitle] = useState("");
  const [autoRenameDialogConversationId, setAutoRenameDialogConversationId] =
    useState<string | null>(null);
  const [archiveDialogTarget, setArchiveDialogTarget] =
    useState<ArchiveConversationDialogTarget | null>(null);
  const [openSessionActionsId, setOpenSessionActionsId] = useState<string | null>(null);
  const [visibleEffectRows, setVisibleEffectRows] = useState<
    AgentSidebarConversationRow[]
  >([]);
  const [adaptiveProjectPageSize, setAdaptiveProjectPageSize] = useState(
    AGENTS_SIDEBAR_MAX_VISIBLE_SESSION_ROWS
  );
  const expandedProjectIds = useAgentSessionStore((s) => s.expandedProjectIds);
  const setProjectExpanded = useAgentSessionStore((s) => s.setProjectExpanded);
  const expanded = searchQuery.length > 0 ? true : expandedProjectIds[project.id] ?? isFocused;
  const projectScrollKey = useMemo(
    () =>
      [
        "project",
        project.id,
        showArchived ? "archived" : "active",
        searchQuery,
        selectedPublicationStates.join(","),
        pinnedConversationIdList.join(","),
      ].join("::"),
    [
      pinnedConversationIdList,
      project.id,
      searchQuery,
      selectedPublicationStates,
      showArchived,
    ]
  );
  const rememberedProjectRowCount =
    useRememberedAgentSidebarSessionRowCount(projectScrollKey);
  const groupQuery = useAgentSidebarProjectGroup({
    projectId: project.id,
    archivedOnly: showArchived,
    search: searchQuery,
    publicationStates: selectedPublicationStates,
    pinnedConversationIds: pinnedConversationIdList,
    priorityConversationIds,
    minimumRowCount: rememberedProjectRowCount,
    ...(fillAvailableHeight ? { pageSize: adaptiveProjectPageSize } : {}),
  });
  const activeConversationIds = useChatStore((s) => s.activeConversationIds);
  const agentStatuses = useChatStore((s) => s.agentStatus);
  const agentActivityLabels = useChatStore((s) => s.agentActivityLabels);
  const visibleRows = groupQuery.group.rows;
  useRegisterBulkArchiveRows(projectScrollKey, visibleRows);
  const visibleConversations = useMemo(
    () => visibleRows.map((row) => toProjectAgentConversation(row.conversation)),
    [visibleRows]
  );
  const visibleEffectConversations = useMemo(
    () => visibleEffectRows.map((row) => toProjectAgentConversation(row.conversation)),
    [visibleEffectRows]
  );
  useAgentSidebarRunningStates(
    visibleEffectConversations,
    isSidebarVisible && (showProjectHeader ? expanded : true)
  );
  const projectPublicationCurrentStates = useMemo(() => {
    const map = new Map<string, string>();
    for (const row of visibleEffectRows) {
      map.set(
        row.conversation.id,
        workspacePublicationFingerprint(
          row.publicationState,
          row.publicationLabel,
        ),
      );
    }
    return map;
  }, [visibleEffectRows]);
  useAgentSidebarPublicationPolling(
    visibleEffectConversations,
    isSidebarVisible && (showProjectHeader ? expanded : true),
    projectPublicationCurrentStates
  );
  const handleVisibleProjectRowsChange = useCallback(
    (rows: AgentSidebarConversationRow[]) => {
      setVisibleEffectRows((currentRows) =>
        areAgentSidebarRowsSameByConversationId(currentRows, rows)
          ? currentRows
          : rows
      );
    },
    []
  );
  const handleViewportRowCapacityChange = useCallback((rowCapacity: number) => {
    const nextPageSize = Math.min(
      AGENTS_SIDEBAR_ADAPTIVE_MAX_VISIBLE_SESSION_ROWS,
      Math.max(
        AGENTS_SIDEBAR_MAX_VISIBLE_SESSION_ROWS,
        rowCapacity + AGENTS_SIDEBAR_ADAPTIVE_PAGE_OVERSCAN_ROWS
      )
    );
    setAdaptiveProjectPageSize((currentPageSize) =>
      currentPageSize === nextPageSize ? currentPageSize : nextPageSize
    );
  }, []);
  const totalConversationCount = groupQuery.group.total;
  const activeRuntimeCount = visibleConversations.filter((conversation) => {
    const rowKey = getAgentConversationStoreKey(conversation);
    return (
      activeConversationIds[rowKey] === conversation.id &&
      (agentStatuses[rowKey] ?? "idle") !== "idle"
    );
  }).length;
  const isCurrentProject = expanded && isFocused;
  const handleProjectRowToggle = () => {
    const nextExpanded = !expanded;
    setProjectExpanded(project.id, nextExpanded);
    if (nextExpanded) {
      onFocusProject(project.id);
    }
  };
  const openRenameDialog = useCallback((conversation: AgentConversation) => {
    setRenameDraftTitle(conversation.title || "Untitled agent");
    setRenameDialogConversation(conversation);
  }, []);
  const handleRenameSubmit = useCallback(async () => {
    if (!renameDialogConversation) {
      return;
    }
    const trimmed = renameDraftTitle.trim();
    if (!trimmed) {
      return;
    }

    await onRenameConversation(renameDialogConversation.id, trimmed);
    setRenameDialogConversation(null);
  }, [onRenameConversation, renameDialogConversation, renameDraftTitle]);
  const handleAutoRenameSubmit = useCallback(async () => {
    if (!renameDialogConversation) {
      return;
    }
    setAutoRenameDialogConversationId(renameDialogConversation.id);
    try {
      await onAutoRenameConversation(renameDialogConversation);
      setRenameDialogConversation(null);
    } catch {
      // The owning action reports the failure and the dialog stays open.
    } finally {
      setAutoRenameDialogConversationId(null);
    }
  }, [onAutoRenameConversation, renameDialogConversation]);
  const isAutoRenamingDialog =
    renameDialogConversation !== null &&
    autoRenameDialogConversationId === renameDialogConversation.id;
  const getProjectRowKey = useCallback(
    (row: AgentSidebarConversationRow) => row.conversation.id,
    []
  );
  const renderProjectRow = useCallback(
    (row: AgentSidebarConversationRow) => {
      const conversation = toProjectAgentConversation(row.conversation);
      const rowKey = getAgentConversationStoreKey(conversation);
      const activeConversationId = activeConversationIds[rowKey] ?? null;
      const agentStatus = agentStatuses[rowKey] ?? "idle";
      const runtimeLabel = agentActivityLabels[rowKey] ?? null;
      const isSelected = selectedConversationId === conversation.id;
      const isActiveRuntime = activeConversationId === conversation.id;
      const isPinned = Boolean(pinnedConversationIds[conversation.id]);
      const runtimeState = getSessionRuntimeState(
        conversation,
        isActiveRuntime,
        agentStatus
      );
      const publicationLabel = getVisiblePublicationLabel(
        row.publicationLabel,
        runtimeState,
        runtimeLabel
      );
      const showRuntimeState = shouldShowSessionRuntimeLabel(
        runtimeState,
        publicationLabel
      );
      const sessionActionsOpen = openSessionActionsId === conversation.id;

      return (
        <MemoizedAgentSessionRow
          conversation={conversation}
          workspace={row.workspace}
          projectName={project.name}
          showProjectNameInMeta={showProjectNameInMeta}
          refKind={row.refKind}
          refLabel={row.refLabel}
          publicationState={row.publicationState}
          publicationLabel={publicationLabel}
          parkedDelegateCount={row.parkedDelegateCount}
          isSelected={isSelected}
          isPinned={isPinned}
          isMuted={row.isMuted}
          runtimeState={runtimeState}
          runtimeLabel={runtimeLabel}
          showRuntimeState={showRuntimeState}
          sessionActionsOpen={sessionActionsOpen}
          onSelect={() => onSelectConversation(project.id, conversation)}
          onRename={() => openRenameDialog(conversation)}
          onTogglePinned={() => onTogglePinnedConversation(conversation.id)}
          onFork={() => onForkConversation(conversation)}
          onRestore={() => onRestoreConversation(conversation)}
          onArchiveRequest={() =>
            setArchiveDialogTarget({ conversation, workspace: row.workspace })
          }
          setActionsTriggerRef={(node) => {
            sessionActionsTriggerRefs.current[conversation.id] = node;
          }}
          onActionsOpenChange={(open) => {
            setOpenSessionActionsId(open ? conversation.id : null);
            if (!open) {
              requestAnimationFrame(() => {
                sessionActionsTriggerRefs.current[conversation.id]?.blur();
              });
            }
          }}
        />
      );
    },
    [
      activeConversationIds,
      agentActivityLabels,
      agentStatuses,
      onForkConversation,
      onRestoreConversation,
      onSelectConversation,
      onTogglePinnedConversation,
      openRenameDialog,
      openSessionActionsId,
      pinnedConversationIds,
      project.id,
      project.name,
      selectedConversationId,
      showProjectNameInMeta,
    ]
  );

  if (
    !groupQuery.isLoading &&
    visibleConversations.length === 0 &&
    (!showProjectHeader ||
      showArchived ||
      searchQuery.length > 0 ||
      !showEmptyProjectGroups)
  ) {
    return null;
  }

  return (
    <div
      className={
        fillAvailableHeight
          ? "my-1 flex min-h-0 flex-1 flex-col gap-0.5"
          : "my-1 flex flex-col gap-0.5"
      }
      data-testid={
        showProjectHeader
          ? `agents-project-${project.id}`
          : `agents-project-${project.id}-state`
      }
    >
        <div
          className={
            fillAvailableHeight
              ? "relative flex min-h-0 flex-1 flex-col"
              : "relative"
          }
        >
          {showProjectHeader && (
          <div className="group/project-row relative">
          <button
            type="button"
            className="agents-project-row grid w-full grid-cols-[12px_14px_minmax(0,1fr)_auto] items-center gap-[7px] rounded-[6px] px-2 py-1.5 text-left text-[0.8438rem] transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-elevated)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
            data-testid={`agents-project-row-${project.id}`}
            aria-expanded={expanded}
            aria-label={`${expanded ? "Collapse" : "Expand"} project ${project.name}`}
            aria-current={isCurrentProject ? "true" : undefined}
            onClick={handleProjectRowToggle}
          >
            <span
              className="agents-project-chevron grid h-3 w-3 place-items-center rounded"
              aria-hidden="true"
            >
              <ChevronRight
                className={`h-2.5 w-2.5 transition-transform duration-[120ms] ${expanded ? "rotate-90" : ""}`}
                strokeWidth={2}
              />
            </span>
            <Folder
              className="agents-project-icon h-3.5 w-3.5 shrink-0"
              strokeWidth={1.8}
            />
            <span className="min-w-0 truncate">
              {project.name}
            </span>
            {totalConversationCount > 0 && (
              <span
                className={`agents-project-count grid min-w-[18px] place-items-center rounded-full border px-1.5 text-[0.6562rem] leading-[1.6] transition-opacity duration-150 ${
                  projectActionsOpen
                    ? "opacity-0"
                    : "opacity-100 group-hover/project-row:opacity-0 group-focus-within/project-row:opacity-0"
                }`}
              >
                {totalConversationCount}
              </span>
            )}
            {totalConversationCount === 0 && !expanded && activeRuntimeCount > 0 && (
              <span
                className={`agents-project-active-count grid min-w-[18px] place-items-center rounded-full px-1.5 text-[0.6562rem] font-medium leading-[1.6] transition-opacity duration-150 ${
                  projectActionsOpen
                    ? "opacity-0"
                    : "opacity-100 group-hover/project-row:opacity-0 group-focus-within/project-row:opacity-0"
                }`}
                style={{
                  color: "var(--accent-primary)",
                  backgroundColor: withAlpha("var(--accent-primary)", 15),
                }}
              >
                {activeRuntimeCount}
              </span>
            )}
          </button>
            <div
              className={`absolute right-1 top-1/2 flex -translate-y-1/2 items-center gap-0.5 rounded-[6px] transition-opacity duration-150 ${
                projectActionsOpen
                  ? "opacity-100"
                  : "opacity-0 group-hover/project-row:opacity-100 group-focus-within/project-row:opacity-100"
              }`}
              data-testid={`agents-project-actions-${project.id}`}
              onClick={(event) => event.stopPropagation()}
            >
              <DropdownMenu
                modal={false}
                onOpenChange={(open) => {
                  setProjectActionsOpen(open);
                  if (!open) {
                    requestAnimationFrame(() => {
                      projectActionsTriggerRef.current?.blur();
                    });
                  }
                }}
              >
                <DropdownMenuTrigger asChild>
                  <Button
                    ref={projectActionsTriggerRef}
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-5.5 w-5.5 rounded border-0 bg-transparent p-0 outline-none ring-0 hover:bg-transparent focus:bg-transparent focus:outline-none focus:ring-0 focus-visible:bg-transparent focus-visible:outline-none focus-visible:ring-0 data-[state=open]:bg-transparent"
                    aria-label="Project actions"
                    data-theme-button-skip="true"
                    style={{ boxShadow: "none" }}
                  >
                    <MoreHorizontal className="w-3.5 h-3.5" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent
                  align="end"
                  onCloseAutoFocus={(event) => {
                    event.preventDefault();
                    projectActionsTriggerRef.current?.blur();
                  }}
                >
                  <DropdownMenuItem
                    className="gap-2 text-xs"
                    onClick={() => setPrTemplateDialogOpen(true)}
                  >
                    <Pencil className="w-3.5 h-3.5" />
                    Edit PR Template
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem
                    className="gap-2 text-xs"
                    onClick={() => setArchiveDialogOpen(true)}
                  >
                    <Archive className="w-3.5 h-3.5" />
                    Archive project
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>
          </div>
          )}

          <PrTemplateEditorDialog
            open={prTemplateDialogOpen}
            onOpenChange={setPrTemplateDialogOpen}
            project={project}
          />

          <AlertDialog open={archiveDialogOpen} onOpenChange={setArchiveDialogOpen}>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Archive project?</AlertDialogTitle>
                <AlertDialogDescription>
                  This removes <span className="font-medium">{project.name}</span> from the
                  sidebar without deleting it. You can add the same repository again later
                  from the normal project flow.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>Cancel</AlertDialogCancel>
                <AlertDialogAction
                  onClick={() => {
                    setArchiveDialogOpen(false);
                    void onArchiveProject(project.id);
                  }}
                  variant="destructive"
                >
                  Archive project
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>

          <Dialog
            open={renameDialogConversation !== null}
            onOpenChange={(open) => {
              if (!open) {
                setRenameDialogConversation(null);
              }
            }}
          >
            <DialogContent hideCloseButton className="max-w-md">
              <DialogHeader className="block space-y-1.5">
                <DialogTitle className="text-base">Rename session</DialogTitle>
                <DialogDescription>
                  Update the title shown in the Agents sidebar for this conversation.
                </DialogDescription>
              </DialogHeader>
              <div className="px-6 py-4">
                <Input
                  value={renameDraftTitle}
                  onChange={(event) => setRenameDraftTitle(event.target.value)}
                  aria-label="Session title"
                  placeholder="Untitled agent"
                  autoFocus
                  onKeyDown={(event) => {
                    if (event.key === "Enter") {
                      event.preventDefault();
                      void handleRenameSubmit();
                    }
                  }}
                />
              </div>
              <DialogFooter className="justify-between">
                <Button
                  type="button"
                  variant="secondary"
                  className="mr-auto"
                  onClick={() => void handleAutoRenameSubmit()}
                  disabled={isAutoRenamingDialog}
                >
                  <Sparkles className="h-4 w-4" aria-hidden="true" />
                  {isAutoRenamingDialog ? "Starting..." : "Auto rename"}
                </Button>
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => setRenameDialogConversation(null)}
                  disabled={isAutoRenamingDialog}
                >
                  Cancel
                </Button>
                <Button
                  type="button"
                  onClick={() => void handleRenameSubmit()}
                  disabled={isAutoRenamingDialog || renameDraftTitle.trim().length === 0}
                >
                  Rename session
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>

          <ArchiveConversationDialog
            target={archiveDialogTarget}
            onClose={() => setArchiveDialogTarget(null)}
            onArchive={(conversation, options) => {
              void onArchiveConversation(conversation, options);
            }}
          />

          {(showProjectHeader ? expanded : true) && (
            <ScrollableAgentSessionList
              fetchNextPage={groupQuery.fetchNextPage}
              fillAvailableHeight={fillAvailableHeight}
              getItemKey={getProjectRowKey}
              hasNextPage={Boolean(groupQuery.hasNextPage)}
              isFetchingNextPage={Boolean(groupQuery.isFetchingNextPage)}
              isLoading={Boolean(groupQuery.isLoading)}
              isVisible={isSidebarVisible}
              onViewportRowCapacityChange={handleViewportRowCapacityChange}
              onVisibleRowsChange={handleVisibleProjectRowsChange}
              renderRow={renderProjectRow}
              rows={visibleRows}
              scrollKey={projectScrollKey}
              testId={`agents-sidebar-session-list-${project.id}`}
            />
          )}
        </div>
    </div>
  );
}

function useDebouncedValue<T>(value: T, delayMs: number): T {
  const [debouncedValue, setDebouncedValue] = useState(value);

  useEffect(() => {
    const timeout = window.setTimeout(() => setDebouncedValue(value), delayMs);
    return () => window.clearTimeout(timeout);
  }, [delayMs, value]);

  return debouncedValue;
}

function StaticRecentRuns() {
  return (
    <div
      className="shrink-0 border-t px-3 pb-1.5 pt-3"
      data-testid="agents-static-recent"
      aria-hidden="true"
      title="Coming soon"
      style={{
        borderColor: "var(--app-sidebar-border)",
        display: "none",
      }}
    >
      <div className="mb-2 flex items-center justify-between px-1">
        <span
          className="text-[0.6562rem] font-semibold uppercase leading-none tracking-[0.12em]"
          style={{ color: "var(--text-muted)" }}
        >
          Recent
        </span>
        <button
          type="button"
          className="rounded-[4px] px-1 text-[0.6875rem] font-medium leading-none outline-none transition-colors hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
          style={{ color: "var(--text-muted)", boxShadow: "none" }}
        >
          View all
        </button>
      </div>
      <div className="flex flex-col gap-0.5">
        {STATIC_RECENT_RUNS.map((run) => (
          <button
            type="button"
            key={run.title}
            className="group/recent grid w-full grid-cols-[7px_minmax(0,1fr)_12px] items-center gap-[9px] rounded-[6px] px-2 py-1.5 text-left text-[var(--text-secondary)] transition-colors duration-[120ms] ease-[cubic-bezier(.2,.8,.2,1)] outline-none hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] focus-visible:[outline:2px_solid_var(--border-focus)] focus-visible:[outline-offset:2px]"
            style={{ boxShadow: "none" }}
          >
            <span
              className="block h-[7px] w-[7px] rounded-full"
              style={{ background: "var(--status-success)" }}
            />
            <span className="min-w-0">
              <span
                className="block whitespace-normal break-words text-[0.7812rem] font-medium leading-[1.4] [text-overflow:clip]"
                style={{
                  overflow: "visible",
                  textOverflow: "clip",
                  whiteSpace: "normal",
                  width: "168px",
                }}
              >
                {run.title}
              </span>
              <span
                className="block truncate text-[0.6562rem] leading-[1.4]"
                style={{
                  color: "var(--text-muted)",
                  fontFamily: "var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace)",
                }}
              >
                {run.project}
                <span>{" · "}</span>
                {run.time}
              </span>
            </span>
            <ChevronRight
              aria-hidden="true"
              className="h-3 w-3 opacity-0 transition-opacity duration-[120ms] group-hover/recent:opacity-100"
              style={{ color: "var(--text-subtle)" }}
              strokeWidth={2}
            />
          </button>
        ))}
      </div>
    </div>
  );
}

type SessionRuntimeState = "running" | "waiting" | "queued" | "done" | "blocked" | "archived";
const PUBLICATION_LABELS_WITH_OWN_RUNTIME = new Set([
  "auto-merge",
  "blocked",
  "fixing",
  "waiting",
]);

function getSessionRuntimeState(
  conversation: AgentConversation,
  isActiveRuntime: boolean,
  status: string
): SessionRuntimeState {
  if (conversation.archivedAt) {
    return "archived";
  }

  if (!isActiveRuntime || status === "idle") {
    return "queued";
  }

  if (status === "waiting_for_input") {
    return "waiting";
  }

  if (status === "completed") {
    return "done";
  }

  if (status === "failed" || status === "error" || status === "needs_approval") {
    return "blocked";
  }

  return "running";
}

function getVisiblePublicationLabel(
  publicationLabel: string | null,
  state: SessionRuntimeState,
  runtimeLabel: string | null
) {
  const normalizedPublicationLabel =
    publicationLabel?.trim().toLowerCase() ?? null;
  const normalizedRuntimeLabel = runtimeLabel?.trim().toLowerCase() ?? null;
  if (
    state === "running" &&
    normalizedPublicationLabel === "blocked" &&
    normalizedRuntimeLabel === "reviewing"
  ) {
    return null;
  }

  return publicationLabel;
}

function shouldShowSessionRuntimeLabel(
  state: SessionRuntimeState,
  publicationLabel: string | null
) {
  if (state !== "running" && state !== "waiting") {
    return false;
  }

  const normalizedPublicationLabel = publicationLabel?.trim().toLowerCase() ?? null;
  return (
    !normalizedPublicationLabel ||
    !PUBLICATION_LABELS_WITH_OWN_RUNTIME.has(normalizedPublicationLabel)
  );
}

function SessionRuntimeLabel({
  state,
  label,
}: {
  state: SessionRuntimeState;
  label: string | null;
}) {
  if (state === "running") {
    return (
      <span className="agents-session-runtime-label font-medium">
        {label ?? "running"}
      </span>
    );
  }

  if (state === "waiting") {
    return (
      <span className="font-medium" style={{ color: "var(--text-muted)" }}>
        awaiting input
      </span>
    );
  }

  return null;
}

function SessionStatusIcon({
  conversationId,
  isPinned,
  isMuted,
  state,
  selected,
}: {
  conversationId: string;
  isPinned: boolean;
  isMuted: boolean;
  state: SessionRuntimeState;
  selected: boolean;
}) {
  if (isPinned) {
    return (
      <Pin
        aria-hidden="true"
        className="h-3.5 w-3.5"
        data-testid={`agents-pin-icon-${conversationId}`}
        style={{
          color: state === "running" ? "var(--accent-primary)" : "var(--text-subtle)",
        }}
      />
    );
  }

  if (isMuted) {
    return (
      <BellOff
        aria-label="Muted until it changes"
        className="h-3.5 w-3.5"
        data-testid={`agents-mute-icon-${conversationId}`}
        style={{ color: "var(--text-subtle)" }}
      />
    );
  }

  return <SessionStatusDot state={state} selected={selected} />;
}

function SessionStatusDot({
  state,
}: {
  state: SessionRuntimeState;
  selected: boolean;
}) {
  if (state === "running") {
    return (
      <span
        aria-hidden="true"
        className="block h-[7px] w-[7px] shrink-0 rounded-full"
        style={{
          backgroundColor: "var(--accent-primary)",
          border: "1.5px solid transparent",
        }}
      />
    );
  }

  if (state === "done") {
    return (
      <span
        aria-hidden="true"
        className="block h-[7px] w-[7px] shrink-0 rounded-full"
        style={{
          backgroundColor: "var(--status-success)",
          border: "1.5px solid transparent",
        }}
      />
    );
  }

  if (state === "waiting") {
    return (
      <span
        aria-hidden="true"
        className="block h-[7px] w-[7px] shrink-0 rounded-full"
        style={{
          backgroundColor: "transparent",
          borderColor: "var(--text-subtle)",
          borderStyle: "solid",
          borderWidth: "1.5px",
        }}
      />
    );
  }

  if (state === "queued") {
    return (
      <span
        aria-hidden="true"
        className="block h-[7px] w-[7px] shrink-0 rounded-full"
        style={{
          backgroundColor: "transparent",
          borderColor: "var(--text-subtle)",
          borderStyle: "solid",
          borderWidth: "1.5px",
        }}
      />
    );
  }

  return null;
}
