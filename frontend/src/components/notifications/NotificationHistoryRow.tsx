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
  observe: (id: string, readAt: string | undefined) => (element: HTMLButtonElement | null) => void;
}

export const NotificationHistoryRow = memo(function NotificationHistoryRow({
  notification,
  now,
  onOpen,
  observe,
}: NotificationHistoryRowProps) {
  const presentation = ATTENTION_CATEGORY_MAPPING[notification.category];
  const Icon = presentation.icon;
  const unread = notification.readAt === undefined;

  return (
    <button
      ref={observe(notification.id, notification.readAt)}
      type="button"
      data-testid={`notification-history-row-${notification.id}`}
      onClick={() => onOpen(notification)}
      className={cn(
        "flex w-full items-center gap-2 px-3 py-2 text-left outline-none hover:bg-[var(--bg-hover)]/35 focus-visible:ring-1 focus-visible:ring-[var(--accent-primary)]",
        unread && "bg-[var(--accent-muted)]",
      )}
      style={{ backgroundColor: unread ? "var(--accent-muted)" : "transparent" }}
    >
      <span className="flex w-2 shrink-0 justify-center" aria-hidden="true">
        {unread && <span className="h-1 w-1 rounded-full" style={{ backgroundColor: "var(--accent-primary)" }} />}
      </span>
      <Icon className="h-4 w-4 shrink-0" style={{ color: presentation.iconColor }} aria-hidden="true" />
      <span className="min-w-0 flex-1 truncate text-sm font-medium" style={{ color: "var(--text-primary)" }}>
        {notification.title}
      </span>
      <span className="shrink-0 text-xs" style={{ color: "var(--text-muted)" }}>
        {relativeTime(notification.createdAt, now)}
      </span>
    </button>
  );
});
