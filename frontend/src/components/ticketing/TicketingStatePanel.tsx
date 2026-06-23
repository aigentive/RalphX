import { AlertTriangle, Inbox, Loader2, PlugZap, ShieldAlert, WifiOff } from "lucide-react";

import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";

interface TicketingStatePanelProps {
  state: "loading" | "empty" | "disconnected" | "permission" | "stale" | "error";
  title: string;
  description?: string;
  actionLabel?: string;
  onAction?: () => void;
}

const STATE_ICON = {
  loading: Loader2,
  empty: Inbox,
  disconnected: WifiOff,
  permission: ShieldAlert,
  stale: PlugZap,
  error: AlertTriangle,
};

const STATE_VARIANT = {
  loading: "neutral",
  empty: "neutral",
  disconnected: "warning",
  permission: "warning",
  stale: "warning",
  error: "error",
} as const;

export function TicketingStatePanel({
  state,
  title,
  description,
  actionLabel,
  onAction,
}: TicketingStatePanelProps) {
  const Icon = STATE_ICON[state];
  return (
    <EmptyState
      data-testid={`ticketing-state-${state}`}
      variant={STATE_VARIANT[state]}
      className="h-full min-h-[320px] w-full"
      icon={<Icon className={state === "loading" ? "animate-spin" : undefined} />}
      title={title}
      {...(description !== undefined && { description })}
      action={
        actionLabel && onAction ? (
          <Button type="button" size="sm" variant="outline" onClick={onAction}>
            {actionLabel}
          </Button>
        ) : undefined
      }
    />
  );
}
