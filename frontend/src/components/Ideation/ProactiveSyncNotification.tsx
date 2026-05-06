/**
 * ProactiveSyncNotification - Notification banner for plan sync updates
 */

import { AlertCircle, Eye, Undo2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ProactiveSyncNotification } from "@/stores/ideationStore";

interface ProactiveSyncNotificationProps {
  notification: ProactiveSyncNotification;
  onDismiss: () => void;
  onReview: () => void;
  onUndo: () => void;
}

export function ProactiveSyncNotificationBanner({
  notification,
  onDismiss,
  onReview,
  onUndo,
}: ProactiveSyncNotificationProps) {
  const affectedCount = notification.proposalIds.length;

  return (
    <div
      data-testid="proactive-sync-notification"
      className="mb-3 p-3 rounded-lg bg-gradient-to-br from-[var(--accent-primary)]/10 to-[var(--accent-primary)]/5 border border-[var(--accent-primary)]/30"
    >
      <div className="flex items-start gap-2">
        <div className="w-7 h-7 rounded-md bg-[var(--accent-primary)]/20 flex items-center justify-center flex-shrink-0">
          <AlertCircle className="w-3.5 h-3.5 text-[var(--accent-primary)]" />
        </div>
        <div className="flex-1 min-w-0">
          <p className="text-xs font-medium text-[var(--text-primary)] mb-0.5">Plan updated</p>
          <p className="text-[0.6875rem] text-[var(--text-secondary)]">
            {affectedCount} proposal{affectedCount !== 1 ? "s" : ""} may need revision.
          </p>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="sm"
            onClick={onReview}
            className="h-6 px-2 text-[0.6875rem] text-[var(--accent-primary)] hover:bg-[var(--accent-primary)]/10"
          >
            <Eye className="w-3 h-3 mr-1" /> Review
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={onUndo}
            className="h-6 px-2 text-[0.6875rem] hover:bg-white/[0.06]"
          >
            <Undo2 className="w-3 h-3 mr-1" /> Undo
          </Button>
          <Button
            variant="ghost"
            size="icon"
            onClick={onDismiss}
            className="h-6 w-6 hover:bg-white/[0.06]"
          >
            ×
          </Button>
        </div>
      </div>
    </div>
  );
}
