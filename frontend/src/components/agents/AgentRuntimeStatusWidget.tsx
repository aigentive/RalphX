import { useEffect, useRef } from "react";
import {
  Eye,
  GitPullRequestArrow,
  Lightbulb,
  Loader2,
  MessageSquare,
  Play,
  ShieldCheck,
  type LucideIcon,
} from "lucide-react";

import type {
  AgentConversationRuntimeItem,
  AgentConversationRuntimeSource,
  AgentConversationRuntimeStatus,
} from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  isTaskRuntimeContextType,
  type AgentTaskRuntimeContextType,
} from "./agentTaskRuntimeContext";

export type AgentRuntimeStatusCurrentFocus =
  | { type: "workspace" }
  | { type: "ideation"; sessionId: string }
  | { type: "verification"; parentSessionId: string; childSessionId: string }
  | {
      type: "task_runtime";
      taskId: string;
      contextType: AgentTaskRuntimeContextType;
    };

const EMPTY_RUNTIME_ITEMS: AgentConversationRuntimeItem[] = [];
const VISIBLE_RUNTIME_COUNT = 3;
const RUNTIME_STATUS_ROW_HEIGHT_PX = 32;
const RUNTIME_STATUS_ROW_GAP_PX = 6;
const RUNTIME_STATUS_LIST_MAX_HEIGHT_PX =
  VISIBLE_RUNTIME_COUNT * RUNTIME_STATUS_ROW_HEIGHT_PX +
  (VISIBLE_RUNTIME_COUNT - 1) * RUNTIME_STATUS_ROW_GAP_PX;

function iconForSource(source: AgentConversationRuntimeSource): LucideIcon {
  if (source === "ideation") return Lightbulb;
  if (source === "verification") return ShieldCheck;
  if (source === "review") return ShieldCheck;
  if (source === "merge") return GitPullRequestArrow;
  if (source === "task_execution") return Play;
  return MessageSquare;
}

function ctaLabelForItem(item: AgentConversationRuntimeItem): string {
  if (item.source === "ideation") return "View Ideation";
  if (item.source === "verification") return "View Verification";
  if (
    item.source === "task_execution" ||
    item.source === "review" ||
    item.source === "merge"
  ) {
    return "View Task";
  }
  return "View Workspace";
}

function runtimeItemKey(item: AgentConversationRuntimeItem): string {
  return `${item.source}:${item.contextType}:${item.contextId}`;
}

function isCurrentRuntimeItem(
  item: AgentConversationRuntimeItem,
  currentFocus: AgentRuntimeStatusCurrentFocus | null | undefined,
): boolean {
  if (!currentFocus) {
    return false;
  }
  if (currentFocus.type === "workspace") {
    return item.source === "workspace";
  }
  if (currentFocus.type === "ideation") {
    return (
      item.source === "ideation" && item.contextId === currentFocus.sessionId
    );
  }
  if (currentFocus.type === "verification") {
    return (
      item.source === "verification" &&
      item.parentSessionId === currentFocus.parentSessionId &&
      (item.childSessionId ?? item.contextId) === currentFocus.childSessionId
    );
  }
  return (
    item.taskId === currentFocus.taskId &&
    item.contextType === currentFocus.contextType &&
    isTaskRuntimeContextType(item.contextType)
  );
}

interface AgentRuntimeStatusWidgetProps {
  status: AgentConversationRuntimeStatus | null | undefined;
  showSingleWorkspaceRuntime?: boolean;
  currentFocus?: AgentRuntimeStatusCurrentFocus | null;
  selectedTaskId?: string | null;
  onViewWorkspace: () => void;
  onViewIdeation: (sessionId: string) => void;
  onViewVerification: (parentSessionId: string, childSessionId: string) => void;
  onViewTaskRuntime: (
    taskId: string,
    contextType: AgentTaskRuntimeContextType,
  ) => void;
}

export function AgentRuntimeStatusWidget({
  status,
  showSingleWorkspaceRuntime = false,
  currentFocus = null,
  selectedTaskId = null,
  onViewWorkspace,
  onViewIdeation,
  onViewVerification,
  onViewTaskRuntime,
}: AgentRuntimeStatusWidgetProps) {
  const itemRowRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const items = status?.items ?? EMPTY_RUNTIME_ITEMS;
  const [singleItem] = items;
  const shouldRender = Boolean(
    status?.isRunning &&
      items.length > 0 &&
      !(
        items.length === 1 &&
        singleItem?.source === "workspace" &&
        !showSingleWorkspaceRuntime
      ),
  );

  useEffect(() => {
    if (!shouldRender) {
      return;
    }

    const selectedTaskItem = selectedTaskId
      ? items.find((item) => item.taskId === selectedTaskId)
      : undefined;
    const firstRunningItem = items.find(
      (item) => item.agentStatus === "generating",
    );
    const scrollTargetItem = selectedTaskItem ?? firstRunningItem;
    if (!scrollTargetItem) {
      return;
    }

    const scrollTimer = window.setTimeout(() => {
      const node = itemRowRefs.current.get(runtimeItemKey(scrollTargetItem));
      if (typeof node?.scrollIntoView === "function") {
        node.scrollIntoView({
          block: "nearest",
          inline: "nearest",
          behavior: "smooth",
        });
      }
    }, 0);

    return () => window.clearTimeout(scrollTimer);
  }, [items, selectedTaskId, shouldRender]);

  if (!shouldRender) {
    return null;
  }

  const handleItemClick = (item: AgentConversationRuntimeItem) => {
    if (item.source === "ideation") {
      onViewIdeation(item.contextId);
      return;
    }
    if (
      item.source === "verification" &&
      item.parentSessionId &&
      item.childSessionId
    ) {
      onViewVerification(item.parentSessionId, item.childSessionId);
      return;
    }
    if (
      item.taskId &&
      (item.source === "task_execution" ||
        item.source === "review" ||
        item.source === "merge") &&
      isTaskRuntimeContextType(item.contextType)
    ) {
      onViewTaskRuntime(item.taskId, item.contextType);
      return;
    }
    onViewWorkspace();
  };

  return (
    <div
      className="mx-1 mb-1.5 rounded-md border px-3 py-2"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid="agents-runtime-status-widget"
    >
      <div className="flex items-center gap-2">
        <Loader2
          className="h-4 w-4 shrink-0 animate-spin"
          style={{ color: "var(--accent-primary)" }}
          aria-hidden="true"
        />
        <div className="min-w-0 flex-1">
          <p
            className="truncate text-xs font-semibold"
            style={{ color: "var(--text-primary)" }}
          >
            {status?.summaryLabel ?? "Agent running"}
          </p>
          <p
            className="truncate text-[0.6875rem]"
            style={{ color: "var(--text-muted)" }}
          >
            {items.length === 1
              ? items[0]?.title
              : `${items.length} active runtimes`}
          </p>
        </div>
      </div>
      <div
        className="mt-2 flex flex-col gap-1.5 overflow-y-auto overscroll-contain pr-1"
        data-testid="agents-runtime-status-list"
        style={{ maxHeight: `${RUNTIME_STATUS_LIST_MAX_HEIGHT_PX}px` }}
      >
        {items.map((item) => {
          const Icon = iconForSource(item.source);
          const itemKey = runtimeItemKey(item);
          const isCurrentFocus = isCurrentRuntimeItem(item, currentFocus);
          return (
            <div
              key={itemKey}
              ref={(node) => {
                if (node) {
                  itemRowRefs.current.set(itemKey, node);
                } else {
                  itemRowRefs.current.delete(itemKey);
                }
              }}
              className="flex min-h-8 items-center gap-2"
              data-testid={`agents-runtime-status-item-${item.source}`}
            >
              <Icon
                className="h-3.5 w-3.5 shrink-0"
                style={{ color: "var(--text-muted)" }}
                aria-hidden="true"
              />
              <div className="min-w-0 flex-1">
                <p
                  className="truncate text-[0.6875rem] font-medium"
                  style={{ color: "var(--text-primary)" }}
                >
                  {item.label}
                </p>
                <p
                  className="truncate text-[0.6875rem]"
                  style={{ color: "var(--text-muted)" }}
                >
                  {item.title}
                </p>
              </div>
              {isCurrentFocus ? (
                <span
                  className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded border"
                  style={{
                    borderColor: "var(--border-subtle)",
                    color: "var(--text-muted)",
                  }}
                  aria-label={`Currently viewing ${item.title}`}
                  data-testid={`agents-runtime-status-current-${item.source}`}
                >
                  <Eye className="h-3.5 w-3.5" aria-hidden="true" />
                </span>
              ) : (
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  className="h-7 shrink-0 px-2 text-[0.6875rem]"
                  onClick={() => handleItemClick(item)}
                  data-testid={`agents-runtime-status-action-${item.source}`}
                >
                  {ctaLabelForItem(item)}
                </Button>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
