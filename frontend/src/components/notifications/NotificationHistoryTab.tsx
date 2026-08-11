import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronDown, Inbox, RefreshCw, TriangleAlert } from "lucide-react";

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  flattenNotificationPages,
  useNotificationHistory,
  useNotificationReadActions,
} from "@/hooks/useNotificationHistory";
import type { Notification } from "@/types/notifications";

import { NotificationHistoryRow } from "./NotificationHistoryRow";

function useDeferredHistoryContent(active: boolean): boolean {
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    let timeoutId: ReturnType<typeof setTimeout> | null = null;
    const frameId = window.requestAnimationFrame(() => {
      timeoutId = setTimeout(() => setMounted(active), 0);
    });
    return () => {
      window.cancelAnimationFrame(frameId);
      if (timeoutId !== null) clearTimeout(timeoutId);
    };
  }, [active]);

  return mounted;
}

function dayLabel(createdAt: string): string {
  const date = new Date(createdAt);
  const today = new Date();
  const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  const startOfDate = new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const dayDelta = Math.round((startOfToday - startOfDate) / 86_400_000);
  if (dayDelta === 0) return "TODAY";
  if (dayDelta === 1) return "YESTERDAY";
  return new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", year: "numeric" }).format(date).toUpperCase();
}

function groupByDay(notifications: Notification[]) {
  return notifications.reduce<Array<{ label: string; notifications: Notification[] }>>((groups, notification) => {
    const label = dayLabel(notification.createdAt);
    const current = groups[groups.length - 1];
    if (current?.label === label) {
      current.notifications.push(notification);
    } else {
      groups.push({ label, notifications: [notification] });
    }
    return groups;
  }, []);
}

function HistorySkeleton() {
  return <div className="space-y-2 p-4" data-testid="notification-history-skeletons">
    {[0, 1, 2].map((index) => <div key={index} className="h-9 animate-pulse rounded" style={{ backgroundColor: "var(--bg-elevated)" }} />)}
  </div>;
}

function HistoryEmptyState() {
  return <div className="flex h-full flex-col items-center justify-center gap-2 p-8 text-center" data-testid="notification-history-empty-state">
    <Inbox className="h-7 w-7" style={{ color: "var(--text-muted)" }} />
    <p className="font-medium" style={{ color: "var(--text-primary)" }}>No notifications yet</p>
    <p className="text-sm" style={{ color: "var(--text-muted)" }}>Alerts and completions will show up here.</p>
  </div>;
}

function HistoryLoadError({ onRetry }: { onRetry: () => void }) {
  return <div className="flex h-full flex-col items-center justify-center gap-2 p-8 text-center" data-testid="notification-history-load-error">
    <TriangleAlert className="h-7 w-7" style={{ color: "var(--status-warning)" }} />
    <p className="font-medium" style={{ color: "var(--text-primary)" }}>Couldn&apos;t load notifications</p>
    <button type="button" onClick={onRetry} className="rounded px-3 py-1.5 text-sm font-medium hover:bg-[var(--bg-hover)]" style={{ color: "var(--accent-primary)" }}>Retry</button>
  </div>;
}

function HistoryActions({ markAllRead, refetch, showMarkAllRead }: { markAllRead: () => void; refetch: () => void; showMarkAllRead: boolean }) {
  return <div className="flex justify-end gap-1 px-3 py-2">
    {showMarkAllRead && <button type="button" onClick={markAllRead} className="rounded px-2 py-1 text-xs font-medium hover:bg-[var(--bg-hover)]" style={{ color: "var(--accent-primary)" }}>
      Mark all read
    </button>}
    <Tooltip><TooltipTrigger asChild><button type="button" aria-label="Refresh notification history" data-testid="refresh-notification-history" onClick={refetch} className="grid h-7 w-7 place-items-center rounded hover:bg-[var(--bg-hover)] focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]"><RefreshCw className="h-3.5 w-3.5" aria-hidden="true" /></button></TooltipTrigger><TooltipContent>Refresh notification history</TooltipContent></Tooltip>
  </div>;
}

interface NotificationHistoryTabProps {
  active: boolean;
  now: number;
  onOpen: (notification: Notification) => void;
}

export function NotificationHistoryTab({ active, now, onOpen }: NotificationHistoryTabProps) {
  const contentMounted = useDeferredHistoryContent(active);
  const history = useNotificationHistory(undefined, { enabled: contentMounted });
  const { markRead, markAllRead } = useNotificationReadActions();
  const notifications = flattenNotificationPages(history.data);
  const groups = useMemo(() => groupByDay(notifications), [notifications]);

  const openNotification = useCallback((notification: Notification) => {
    onOpen(notification);
    if (notification.readAt === undefined) markRead(notification.id);
  }, [markRead, onOpen]);

  if (!contentMounted || history.isLoading) return <HistorySkeleton />;
  if (history.isError && notifications.length === 0) return <><HistoryActions markAllRead={() => void markAllRead()} refetch={() => void history.refetch()} showMarkAllRead={false} /><HistoryLoadError onRetry={() => void history.refetch()} /></>;
  if (notifications.length === 0) return <HistoryEmptyState />;

  return <div className="pb-4" data-testid="notification-history-content">
    <HistoryActions markAllRead={() => void markAllRead()} refetch={() => void history.refetch()} showMarkAllRead />
    {history.isError && <p className="px-3 text-xs" data-testid="notification-history-stale-indicator" style={{ color: "var(--text-muted)" }}>Showing saved notifications</p>}
    {groups.map((group) => <section key={group.label} className="pt-2">
      <p className="px-3 pb-1 text-[11px] font-semibold tracking-[0.08em]" style={{ color: "var(--text-muted)" }}>{group.label}</p>
      {group.notifications.map((notification) => <NotificationHistoryRow key={notification.id} notification={notification} now={now} onOpen={openNotification} />)}
    </section>)}
    {history.hasNextPage && <button type="button" data-testid="load-older-notifications" onClick={() => void history.fetchNextPage()} disabled={history.isFetchingNextPage} className="mx-auto mt-3 flex items-center gap-1 rounded px-3 py-2 text-sm hover:bg-[var(--bg-hover)] disabled:opacity-60" style={{ color: "var(--text-secondary)" }}>
      {history.isFetchingNextPage ? "Loading…" : "Load older"}<ChevronDown className="h-4 w-4" aria-hidden="true" />
    </button>}
  </div>;
}
