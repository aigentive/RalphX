import { useState } from "react";

import { Button } from "@/components/ui/button";

interface NotificationActionToastProps {
  actionLabel: string;
  body?: string;
  onAction: () => Promise<void>;
  onDismiss: () => void;
  title: string;
}

export function NotificationActionToast({
  actionLabel,
  body,
  onAction,
  onDismiss,
  title,
}: NotificationActionToastProps) {
  const [actionPending, setActionPending] = useState(false);

  const handleAction = async () => {
    if (actionPending) return;
    setActionPending(true);
    try {
      await onAction();
    } finally {
      setActionPending(false);
    }
  };

  return (
    <div className="flex min-w-0 flex-col gap-3">
      <div className="min-w-0">
        <p className="text-sm font-medium text-[var(--text-primary)]">{title}</p>
        {body && (
          <p className="mt-1 break-words text-xs text-[var(--text-secondary)]">{body}</p>
        )}
      </div>
      <div className="flex items-center justify-end gap-1">
        <Button type="button" variant="ghost" size="sm" onClick={onDismiss}>
          Dismiss
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="text-[var(--accent-primary)]"
          disabled={actionPending}
          onClick={() => void handleAction()}
        >
          {actionPending ? `${actionLabel}…` : actionLabel}
        </Button>
      </div>
    </div>
  );
}
