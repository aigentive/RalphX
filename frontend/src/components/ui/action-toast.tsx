import { useState, type ReactNode } from "react";

import { Button } from "@/components/ui/button";

export interface ActionToastAction {
  label: string;
  onClick: () => void | Promise<void>;
  accent?: boolean;
  pendingLabel?: string;
}

export interface ActionToastProps {
  title: string;
  description?: string | undefined;
  leading?: ReactNode;
  meta?: ReactNode;
  actions?: ActionToastAction[];
  dismissLabel?: string;
  onDismiss?: (() => void) | undefined;
}

function ActionToastActionButton({ action }: { action: ActionToastAction }) {
  const [pending, setPending] = useState(false);

  const handleClick = async () => {
    if (pending) return;
    setPending(true);
    try {
      await action.onClick();
    } finally {
      setPending(false);
    }
  };

  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      className={action.accent ? "text-[var(--accent-primary)]" : undefined}
      disabled={pending}
      onClick={() => void handleClick()}
    >
      {pending ? action.pendingLabel ?? `${action.label}…` : action.label}
    </Button>
  );
}

export function ActionToast({
  title,
  description,
  leading,
  meta,
  actions,
  dismissLabel = "Dismiss",
  onDismiss,
}: ActionToastProps) {
  return (
    <div className="flex min-w-0 flex-col gap-3">
      <div className="min-w-0">
        <p className="text-sm font-medium text-[var(--text-primary)]">
          {leading ? (
            <span className="inline-flex items-center gap-1.5">
              {leading}
              {title}
            </span>
          ) : (
            title
          )}
        </p>
        {description && (
          <p className="mt-1 break-words text-xs text-[var(--text-secondary)]">{description}</p>
        )}
        {meta && <p className="mt-1 text-xs text-[var(--text-muted)]">{meta}</p>}
      </div>
      <div className="flex items-center justify-end gap-1">
        {onDismiss && (
          <Button type="button" variant="ghost" size="sm" onClick={onDismiss}>
            {dismissLabel}
          </Button>
        )}
        {actions?.map((action) => (
          <ActionToastActionButton key={action.label} action={action} />
        ))}
      </div>
    </div>
  );
}
