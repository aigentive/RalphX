import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CheckCircle2, Ellipsis, RefreshCw, TriangleAlert, X } from "lucide-react";
import { useQueries, useQueryClient } from "@tanstack/react-query";

import { TaskReviewCard } from "@/components/reviews/TaskReviewCard";
import { ReviewDetailModal } from "@/components/reviews/ReviewDetailModal";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useAttentionItems } from "@/hooks/useAttentionItems";
import { useNotificationReadActions } from "@/hooks/useNotificationHistory";
import { useTasksAwaitingReview } from "@/hooks/useReviews";
import { taskKeys } from "@/hooks/useTasks";
import { api } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useProjectStore } from "@/stores/projectStore";
import { useTaskStore } from "@/stores/taskStore";
import { useUiStore } from "@/stores/uiStore";
import type { AttentionItem, Notification } from "@/types/notifications";

import { ATTENTION_CATEGORY_MAPPING, type AttentionGroup } from "./categoryMapping";
import { NotificationHistoryTab } from "./NotificationHistoryTab";
import {
  navigateNotification,
  performNotificationPrimaryAction,
} from "./notificationNavigation";

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

function relativeTime(createdAt: string | undefined, now: number): string | null {
  if (!createdAt) return null;
  const milliseconds = now - new Date(createdAt).getTime();
  if (!Number.isFinite(milliseconds)) return null;
  const minutes = Math.max(0, Math.floor(milliseconds / 60_000));
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return hours < 24 ? `${hours}h` : `${Math.floor(hours / 24)}d`;
}

function useNotificationNow(isOpen: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  const wasOpen = useRef(isOpen);

  useEffect(() => {
    if (!isOpen) {
      wasOpen.current = false;
      return undefined;
    }
    if (!wasOpen.current) {
      setNow(Date.now());
    }
    wasOpen.current = true;
    const intervalId = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(intervalId);
  }, [isOpen]);

  return now;
}

function permissionExpiry(createdAt: string | undefined, now: number): string | null {
  if (!createdAt) return null;
  const remaining = Math.max(0, 5 * 60_000 - (now - new Date(createdAt).getTime()));
  return remaining === 0 ? "expired" : `expires in ${Math.ceil(remaining / 60_000)}m`;
}

function AttentionItemRow({
  item,
  onAction,
  onOpen,
  projectName,
  now,
}: {
  item: AttentionItem;
  onAction: (item: AttentionItem) => void;
  onOpen: (item: AttentionItem) => void;
  projectName: string | undefined;
  now: number;
}) {
  const presentation = ATTENTION_CATEGORY_MAPPING[item.category];
  const Icon = presentation.icon;
  const time = item.category === "permission_request"
    ? permissionExpiry(item.createdAt, now)
    : relativeTime(item.createdAt, now);
  const expired = time === "expired";
  const metaTime = item.category === "permission_request" && time ? `⏳ ${expired ? "Expired" : time}` : time;
  const open = () => { if (!expired) onOpen(item); };

  return (
    <div
      role="button"
      tabIndex={0}
      aria-disabled={expired}
      data-testid={`attention-item-${item.id}`}
      onClick={open}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          open();
        }
      }}
      className="w-full max-w-full min-w-0 overflow-hidden rounded-md p-3 text-left outline-none hover:bg-[var(--bg-hover)]/30 focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"
      style={{
        backgroundColor: "var(--bg-elevated)", borderColor: "var(--border-subtle)",
        borderStyle: "solid", borderWidth: "1px",
      }}
    >
      <div className="flex min-w-0 gap-2">
        <Icon className="mt-0.5 h-4 w-4 shrink-0" style={{ color: presentation.iconColor }} />
        <div className="min-w-0 flex-1 overflow-hidden">
          <p className="min-w-0 truncate text-sm font-medium [overflow-wrap:anywhere]" style={{ color: "var(--text-primary)" }}>{item.title}</p>
          {item.detail && <p className="mt-1 line-clamp-2 break-words text-sm" style={{ color: "var(--text-muted)" }}>{item.detail}</p>}
          <div className="mt-2 flex min-w-0 flex-wrap items-center gap-2 text-xs" style={{ color: "var(--text-muted)" }}>
            <span className="min-w-0 flex-1 truncate">{[metaTime, projectName].filter(Boolean).join(" · ")}</span>
            {presentation.action && <Button variant="ghost" size="sm" disabled={expired} className="h-6 max-w-full shrink-0 px-2 text-xs text-[var(--accent-primary)]" onClick={(event) => {
              event.stopPropagation();
              onAction(item);
            }}>{expired ? "Expired" : presentation.action}</Button>}
          </div>
        </div>
      </div>
    </div>
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

function AttentionLoadError({ onRetry }: { onRetry: () => void }) {
  return <div className="flex h-full flex-col items-center justify-center gap-2 p-8 text-center" data-testid="attention-load-error">
    <TriangleAlert className="h-7 w-7" style={{ color: "var(--status-warning)" }} />
    <p className="font-medium" style={{ color: "var(--text-primary)" }}>Couldn&apos;t load notifications</p>
    <Button variant="outline" size="sm" onClick={onRetry}><RefreshCw className="h-3.5 w-3.5" />Retry</Button>
  </div>;
}

export interface NotificationCenterPanelProps {
  isOpen: boolean;
  onClose: () => void;
  onOpenAutomationDetail?: (automationId: string) => void;
  hasUnreadHistory?: boolean;
}

export function NotificationCenterPanel({ isOpen, onClose, onOpenAutomationDetail, hasUnreadHistory = false }: NotificationCenterPanelProps) {
  const [activeTab, setActiveTab] = useState<"action" | "history">("action");
  const [selectedReviewTaskId, setSelectedReviewTaskId] = useState<string | null>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const contentMounted = useDeferredDrawerContent(isOpen);
  const now = useNotificationNow(isOpen);
  const { data: items = [], isLoading, isError, refetch } = useAttentionItems(undefined, { enabled: contentMounted });
  const { markAllRead } = useNotificationReadActions();
  const openModal = useUiStore((state) => state.openModal);
  const projects = useProjectStore((state) => state.projects);
  const activeProjectId = useProjectStore((state) => state.activeProjectId);
  const tasks = useTaskStore((state) => state.tasks);
  const { allTasks: awaitingReviewTasks } = useTasksAwaitingReview(activeProjectId ?? "", { enabled: contentMounted });
  const reviewTaskIds = useMemo(() => [...new Set(items.flatMap((item) => (
    (item.category === "review_needed" || item.category === "review_escalated") && item.target.taskId
      ? [item.target.taskId]
      : []
  )))], [items]);
  const reviewTaskQueries = useQueries({
    queries: reviewTaskIds.map((taskId) => ({
      queryKey: taskKeys.detail(taskId),
      queryFn: () => api.tasks.get(taskId),
      enabled: contentMounted,
    })),
  });
  const queryClient = useQueryClient();

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => { if (event.key === "Escape" && isOpen && !selectedReviewTaskId) onClose(); };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [isOpen, onClose, selectedReviewTaskId]);

  const wasOpen = useRef(isOpen);
  useEffect(() => {
    if (isOpen) closeButtonRef.current?.focus();
    else if (wasOpen.current) document.getElementById("notifications-toggle")?.focus();
    wasOpen.current = isOpen;
  }, [isOpen]);

  const groups = useMemo(() => GROUP_ORDER.map((group) => ({ group, items: items.filter((item) => ATTENTION_CATEGORY_MAPPING[item.category].group === group) })).filter(({ items: groupedItems }) => groupedItems.length > 0), [items]);
  const awaitingReviewTasksById = useMemo(() => Object.fromEntries(awaitingReviewTasks.map((task) => [task.id, task])), [awaitingReviewTasks]);
  const reviewTasksById = useMemo(() => Object.fromEntries(reviewTaskQueries.flatMap((query) => query.data ? [[query.data.id, query.data]] : [])), [reviewTaskQueries]);

  const openItem = useCallback((item: AttentionItem) => {
    void navigateNotification(item, queryClient, {
      onClose,
      ...(onOpenAutomationDetail && { onOpenAutomationDetail }),
    });
  }, [onClose, onOpenAutomationDetail, queryClient]);

  const actOnItem = useCallback((item: AttentionItem) => {
    void performNotificationPrimaryAction(item, queryClient, {
      onClose,
      ...(onOpenAutomationDetail && { onOpenAutomationDetail }),
    });
  }, [onClose, onOpenAutomationDetail, queryClient]);

  const openHistoryNotification = useCallback((notification: Notification) => {
    void navigateNotification(notification, queryClient, {
      onClose,
      ...(onOpenAutomationDetail && { onOpenAutomationDetail }),
    });
  }, [onClose, onOpenAutomationDetail, queryClient]);

  return <>
    <section data-testid="notifications-panel" role="complementary" aria-label="Notifications" className={cn("flex h-full flex-col", !isOpen && "invisible pointer-events-none")} style={{ backgroundColor: "var(--bg-surface)" }}>
      <div className="flex items-center justify-between px-4 py-3" style={{ borderBottomColor: "var(--border-subtle)", borderBottomStyle: "solid", borderBottomWidth: "1px" }}>
        <h2 className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>Notifications</h2>
        <div className="flex items-center gap-1">
          <DropdownMenu>
            <Tooltip><TooltipTrigger asChild><DropdownMenuTrigger asChild><Button variant="ghost" size="icon-sm" aria-label="Notification actions"><Ellipsis className="h-4 w-4" /></Button></DropdownMenuTrigger></TooltipTrigger><TooltipContent>Notification actions</TooltipContent></Tooltip>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onSelect={() => void markAllRead()}>Mark all read</DropdownMenuItem>
              <DropdownMenuItem onSelect={() => {
                onClose();
                openModal("settings", { section: "notifications" });
              }}>Notification settings</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <Tooltip><TooltipTrigger asChild><button ref={closeButtonRef} type="button" data-testid="notifications-panel-close" aria-label="Close notifications" onClick={onClose} className="grid h-7 w-7 place-items-center rounded outline-none hover:bg-[var(--bg-hover)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"><X className="h-4 w-4" /></button></TooltipTrigger><TooltipContent>Close notifications</TooltipContent></Tooltip>
        </div>
      </div>
      <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as "action" | "history")} className="px-4 pt-3">
        <TabsList className="grid h-9 w-full grid-cols-2" style={{ backgroundColor: "var(--bg-surface)" }}>
          <TabsTrigger value="action">Needs action ({items.length})</TabsTrigger><TabsTrigger value="history">History{hasUnreadHistory && <span aria-label="Unread notification history" className="ml-1 h-1 w-1 rounded-full" style={{ backgroundColor: "var(--accent-primary)" }} />}</TabsTrigger>
        </TabsList>
      </Tabs>
      <ScrollArea className="min-h-0 flex-1 [&_[data-radix-scroll-area-viewport]>div]:!block [&_[data-radix-scroll-area-viewport]>div]:!min-w-0 [&_[data-radix-scroll-area-viewport]>div]:!w-full">
        {activeTab === "history" ? <NotificationHistoryTab active={contentMounted} now={now} onOpen={openHistoryNotification} /> : !contentMounted || isLoading ? <SkeletonRows /> : isError && items.length === 0 ? <AttentionLoadError onRetry={() => void refetch()} /> : <>{isError && <p className="px-4 pt-3 text-xs" data-testid="attention-stale-indicator" style={{ color: "var(--text-muted)" }}>Showing saved notifications</p>}{groups.length === 0 ? <EmptyActionState /> : <div className="space-y-4 p-4">{groups.map(({ group, items: groupedItems }) => <div key={group} className="space-y-2"><p className="text-[11px] font-semibold uppercase tracking-[0.08em]" style={{ color: "color-mix(in srgb, var(--text-secondary) 60%, transparent)" }}>{group} · {groupedItems.length}</p><div className="space-y-2">{groupedItems.map((item) => {
          const taskId = item.target.taskId;
          const review = (item.category === "review_needed" || item.category === "review_escalated") && taskId
            ? reviewTasksById[taskId] ?? awaitingReviewTasksById[taskId] ?? tasks[taskId]
            : undefined;
          return review ? <TaskReviewCard key={item.id} task={review} onReview={setSelectedReviewTaskId} presentation="panel" /> : <AttentionItemRow key={item.id} item={item} now={now} onAction={actOnItem} onOpen={openItem} projectName={item.projectId ? projects[item.projectId]?.name : undefined} />;
        })}</div></div>)}</div>}</>}
      </ScrollArea>
    </section>
    {selectedReviewTaskId && <ReviewDetailModal taskId={selectedReviewTaskId} onClose={() => setSelectedReviewTaskId(null)} />}
  </>;
}
