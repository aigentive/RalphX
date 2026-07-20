import { memo } from "react";

import { cn } from "@/lib/utils";
import type { Notification } from "@/types/notifications";

import { ATTENTION_CATEGORY_MAPPING } from "./categoryMapping";

function relativeTime(createdAt: string, now: number): string {
  const milliseconds = now - new Date(createdAt).getTime();
  if (!Number.isFinite(milliseconds)) return "";
  const minutes = Math.max(0, Math.floor(milliseconds / 60_000));
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return hours < 24 ? `${hours}h` : `${Math.floor(hours / 24)}d`;
}

interface NotificationHistoryRowProps {
  notification: Notification;
  now: number;
  onOpen: (notification: Notification) => void;
}

export const NotificationHistoryRow = memo(function NotificationHistoryRow({
  notification,
  now,
  onOpen,
}: NotificationHistoryRowProps) {
  const presentation = ATTENTION_CATEGORY_MAPPING[notification.category];
  const Icon = presentation.icon;
  const unread = notification.readAt === undefined;

  return (
    <button
      type="button"
      data-testid={`notification-history-row-${notification.id}`}
      onClick={() => onOpen(notification)}
      className={cn(
        "grid w-full grid-cols-[0.5rem_1rem_minmax(0,1fr)_max-content] items-start gap-x-2 overflow-hidden px-3 py-2 text-left outline-none hover:bg-[var(--bg-hover)]/35 focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]",
        unread && "bg-[var(--accent-muted)]",
      )}
      style={unread ? { backgroundColor: "var(--accent-muted)" } : undefined}
    >
      <span className="mt-2 flex w-2 justify-center" aria-hidden="true">
        {unread && <span className="h-1 w-1 rounded-full" style={{ backgroundColor: "var(--accent-primary)" }} />}
      </span>
      <Icon className="mt-0.5 h-4 w-4" style={{ color: presentation.iconColor }} aria-hidden="true" />
      <span className="min-w-0 text-sm font-medium leading-5 break-words [overflow-wrap:anywhere] line-clamp-2" style={{ color: "var(--text-primary)" }}>
        {notification.title}
      </span>
      <span className="whitespace-nowrap pt-0.5 pl-1 text-right text-xs" style={{ color: "var(--text-muted)" }}>
        {relativeTime(notification.createdAt, now)}
      </span>
      {notification.body && <span className="col-start-3 col-end-4 min-w-0 pt-0.5 text-xs leading-snug break-words [overflow-wrap:anywhere] line-clamp-2" style={{ color: "var(--text-muted)" }}>
        {notification.body}
      </span>}
    </button>
  );
});
