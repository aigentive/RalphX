import {
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

export type AgentTaskRuntimeContextType = "task_execution" | "review" | "merge";

function isTaskRuntimeContextType(
  contextType: string,
): contextType is AgentTaskRuntimeContextType {
  return (
    contextType === "task_execution" ||
    contextType === "review" ||
    contextType === "merge"
  );
}

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

interface AgentRuntimeStatusWidgetProps {
  status: AgentConversationRuntimeStatus | null | undefined;
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
  onViewWorkspace,
  onViewIdeation,
  onViewVerification,
  onViewTaskRuntime,
}: AgentRuntimeStatusWidgetProps) {
  if (!status?.isRunning || status.items.length === 0) {
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
      className="mx-2 mb-2 rounded-md border px-3 py-2"
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
            {status.summaryLabel ?? "Agent running"}
          </p>
          <p
            className="truncate text-[0.6875rem]"
            style={{ color: "var(--text-muted)" }}
          >
            {status.items.length === 1
              ? status.items[0]?.title
              : `${status.items.length} active runtimes`}
          </p>
        </div>
      </div>
      <div className="mt-2 flex flex-col gap-1.5">
        {status.items.map((item) => {
          const Icon = iconForSource(item.source);
          return (
            <div
              key={`${item.source}:${item.contextType}:${item.contextId}`}
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
            </div>
          );
        })}
      </div>
    </div>
  );
}
