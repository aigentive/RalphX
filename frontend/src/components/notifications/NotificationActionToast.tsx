import { ActionToast } from "@/components/ui/action-toast";

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
  return (
    <ActionToast
      title={title}
      {...(body !== undefined && { description: body })}
      onDismiss={onDismiss}
      actions={[{ label: actionLabel, onClick: onAction, accent: true }]}
    />
  );
}
