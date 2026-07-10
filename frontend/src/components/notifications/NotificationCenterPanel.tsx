import { useCallback, useEffect, useMemo, useState } from "react";
import { CheckCircle2, Inbox, X } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";

import { requestAutomationRunOpen } from "@/components/automations/automationRunNavigation";
import { TaskReviewCard } from "@/components/reviews/TaskReviewCard";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useAttentionItems } from "@/hooks/useAttentionItems";
import { navigateToIdeationSession } from "@/lib/navigation";
import { cn } from "@/lib/utils";
import { useProjectStore } from "@/stores/projectStore";
import { useTaskStore } from "@/stores/taskStore";
import { useUiStore } from "@/stores/uiStore";
import type { AttentionItem } from "@/types/notifications";

import { ATTENTION_CATEGORY_MAPPING, type AttentionGroup } from "./categoryMapping";
import { ReviewDetailModal } from "@/components/reviews/ReviewDetailModal";

const GROUP_ORDER: AttentionGroup[] = ["Agent requests", "Reviews", "Tasks", "Automations", "Git"];

function useDeferredDrawerContent(isOpen: boolean): boolean {
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    let timeoutId: ReturnType<typeof setTimeout> | null = null;
    const frameId = window.requestAnimationFrame(() => {
      timeoutId = setTimeout(() => setMounted(isOpen), 0);
    });
    return () => {
      window.cancelAnimationFrame(frameId);
      if (timeoutId !== null) clearTimeout(timeoutId);
    };
  }, [isOpen]);

  return mounted;
}

function relativeTime(createdAt?: string): string | null {
  if (!createdAt) return null;
  const milliseconds = Date.now() - new Date(createdAt).getTime();
  if (!Number.isFinite(milliseconds)) return null;
  const minutes = Math.max(0, Math.floor(milliseconds / 60_000));
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return hours < 24 ? `${hours}h` : `${Math.floor(hours / 24)}d`;
}

function permissionRequestId(item: AttentionItem): string {
  return item.id.replace(/^(?:permission|perm):/, "");
}

function permissionExpiry(createdAt: string | undefined, now: number): string | null {
  if (!createdAt) return null;
  const remaining = Math.max(0, 5 * 60_000 - (now - new Date(createdAt).getTime()));
  return remaining === 0 ? "expired" : `expires in ${Math.ceil(remaining / 60_000)}m`;
}

function AttentionItemRow({ item, onOpen }: { item: AttentionItem; onOpen: (item: AttentionItem) => void }) {
  const presentation = ATTENTION_CATEGORY_MAPPING[item.category];
  const Icon = presentation.icon;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (item.category !== "permission_request") return undefined;
    const intervalId = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(intervalId);
  }, [item.category]);
  const time = item.category === "permission_request"
    ? permissionExpiry(item.createdAt, now)
    : relativeTime(item.createdAt);

  return (
    <button
      type="button"
      data-testid={`attention-item-${item.id}`}
      onClick={() => onOpen(item)}
      className="w-full rounded-md p-3 text-left outline-none hover:bg-[var(--bg-hover)]/30 focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
      style={{
        backgroundColor: "var(--bg-elevated)", borderColor: "var(--border-subtle)",
        borderStyle: "solid", borderWidth: "1px",
      }}
    >
      <div className="flex gap-2">
        <Icon className="mt-0.5 h-4 w-4 shrink-0" style={{ color: presentation.iconColor }} />
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium" style={{ color: "var(--text-primary)" }}>{item.title}</p>
          {item.detail && <p className="mt-1 line-clamp-2 text-sm" style={{ color: "var(--text-muted)" }}>{item.detail}</p>}
          <div className="mt-2 flex items-center justify-between gap-2 text-xs" style={{ color: "var(--text-muted)" }}>
            <span className="truncate">{[time, item.projectId].filter(Boolean).join(" · ")}</span>
            {presentation.action && <span className="shrink-0 font-medium" style={{ color: "var(--accent-primary)" }}>{presentation.action}</span>}
          </div>
        </div>
      </div>
    </button>
  );
}

function SkeletonRows() {
  return <div className="space-y-2 p-4" data-testid="notification-skeletons">{[0, 1, 2].map((index) => (
    <div key={index} className="h-[92px] animate-pulse rounded-md" style={{ backgroundColor: "var(--bg-elevated)", borderColor: "var(--border-subtle)", borderStyle: "solid", borderWidth: "1px" }} />
  ))}</div>;
}

function EmptyActionState() {
  return <div className="flex h-full flex-col items-center justify-center gap-2 p-8 text-center" data-testid="attention-empty-state">
    <CheckCircle2 className="h-7 w-7" style={{ color: "var(--status-success)" }} />
    <p className="font-medium" style={{ color: "var(--text-primary)" }}>All clear</p>
    <p className="text-sm" style={{ color: "var(--text-muted)" }}>Nothing needs your attention.</p>
  </div>;
}

function HistoryPlaceholder() {
  return <div className="flex h-full flex-col items-center justify-center gap-2 p-8 text-center" data-testid="notification-history-empty-state">
    <Inbox className="h-7 w-7" style={{ color: "var(--text-muted)" }} />
    <p className="font-medium" style={{ color: "var(--text-primary)" }}>No notifications yet</p>
    <p className="text-sm" style={{ color: "var(--text-muted)" }}>Alerts and completions will show up here.</p>
  </div>;
}

export interface NotificationCenterPanelProps {
  projectId?: string;
  isOpen: boolean;
  onClose: () => void;
  onOpenAutomationDetail?: (automationId: string) => void;
}

export function NotificationCenterPanel({ projectId, isOpen, onClose, onOpenAutomationDetail }: NotificationCenterPanelProps) {
  const [activeTab, setActiveTab] = useState<"action" | "history">("action");
  const [selectedReviewTaskId, setSelectedReviewTaskId] = useState<string | null>(null);
  const contentMounted = useDeferredDrawerContent(isOpen);
  const { data: items = [], isLoading } = useAttentionItems(projectId, { enabled: contentMounted });
  const tasks = useTaskStore((state) => state.tasks);
  const queryClient = useQueryClient();

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => { if (event.key === "Escape" && isOpen) onClose(); };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [isOpen, onClose]);

  const groups = useMemo(() => GROUP_ORDER.map((group) => ({ group, items: items.filter((item) => ATTENTION_CATEGORY_MAPPING[item.category].group === group) })).filter(({ items: groupedItems }) => groupedItems.length > 0), [items]);

  const openItem = useCallback((item: AttentionItem) => {
    if (item.category === "permission_request") {
      window.dispatchEvent(new CustomEvent("ralphx:open-permission-dialog", { detail: { requestId: permissionRequestId(item) } }));
      onClose();
      return;
    }
    if (item.target.kind === "task" && item.target.taskId) useUiStore.getState().navigateToTask(item.target.taskId);
    if (item.target.kind === "agent_conversation") {
      const conversationId = item.target.conversationId ?? item.target.setupConversationId;
      if (conversationId) navigateToIdeationSession(conversationId);
    }
    if (item.target.kind === "automation_run" && item.target.projectId && item.target.automationId && item.target.runId && item.target.conversationId) {
      void requestAutomationRunOpen(queryClient, {
        projectId: item.target.projectId,
        automationId: item.target.automationId,
        runId: item.target.runId,
        conversationId: item.target.conversationId,
        ...(item.target.setupConversationId && { setupConversationId: item.target.setupConversationId }),
      }, {
        ...(onOpenAutomationDetail && { onOpenAutomationDetail }),
      });
    } else if (item.target.kind === "automation_run" && item.target.automationId) {
      onOpenAutomationDetail?.(item.target.automationId);
    }
    if (item.target.kind === "project" && item.target.projectId) {
      useProjectStore.getState().selectProject(item.target.projectId);
      useUiStore.getState().setCurrentView("kanban");
    }
    if (item.target.kind !== "none") onClose();
  }, [onClose, onOpenAutomationDetail, queryClient]);

  return <>
    <section data-testid="notifications-panel" role="complementary" aria-label="Notifications" className={cn("flex h-full flex-col", !isOpen && "invisible pointer-events-none")} style={{ backgroundColor: "var(--bg-surface)" }}>
      <div className="flex items-center justify-between px-4 py-3" style={{ borderBottomColor: "var(--border-subtle)", borderBottomStyle: "solid", borderBottomWidth: "1px" }}>
        <h2 className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>Notifications</h2>
        <Tooltip><TooltipTrigger asChild><button type="button" data-testid="notifications-panel-close" aria-label="Close notifications" onClick={onClose} className="grid h-7 w-7 place-items-center rounded outline-none hover:bg-[var(--bg-hover)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"><X className="h-4 w-4" /></button></TooltipTrigger><TooltipContent>Close notifications</TooltipContent></Tooltip>
      </div>
      <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as "action" | "history")} className="px-4 pt-3">
        <TabsList className="grid h-9 w-full grid-cols-2" style={{ backgroundColor: "var(--bg-surface)" }}>
          <TabsTrigger value="action">Needs action ({items.length})</TabsTrigger><TabsTrigger value="history">History</TabsTrigger>
        </TabsList>
      </Tabs>
      <ScrollArea className="min-h-0 flex-1">
        {activeTab === "history" ? <HistoryPlaceholder /> : !contentMounted || isLoading ? <SkeletonRows /> : groups.length === 0 ? <EmptyActionState /> : <div className="space-y-4 p-4">{groups.map(({ group, items: groupedItems }) => <div key={group} className="space-y-2"><p className="text-[11px] font-semibold uppercase tracking-[0.08em]" style={{ color: "color-mix(in srgb, var(--text-secondary) 60%, transparent)" }}>{group} · {groupedItems.length}</p><div className="space-y-2">{groupedItems.map((item) => {
          const taskId = item.target.taskId;
          const review = (item.category === "review_needed" || item.category === "review_escalated") && taskId ? tasks[taskId] : undefined;
          return review ? <TaskReviewCard key={item.id} task={review} onReview={setSelectedReviewTaskId} /> : <AttentionItemRow key={item.id} item={item} onOpen={openItem} />;
        })}</div></div>)}</div>}
      </ScrollArea>
    </section>
    {selectedReviewTaskId && <ReviewDetailModal taskId={selectedReviewTaskId} onClose={() => setSelectedReviewTaskId(null)} />}
  </>;
}
