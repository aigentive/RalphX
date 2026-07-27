import { useEffect } from "react";
import { toast } from "sonner";

import type { StartupStatus } from "@/api/startup";
import { useUiStore } from "@/stores/uiStore";

export const STARTUP_BACKGROUND_OPERATION_TOAST_ID = "startup-background-operation";

interface StartupBackgroundStatusProps {
  active: boolean;
  status: StartupStatus | undefined;
}

/**
 * Reports non-blocking recovery after the App shell has painted. Sonner's
 * toast layer has no backdrop, so workspace controls remain available.
 */
export function StartupBackgroundStatus({
  active,
  status,
}: StartupBackgroundStatusProps) {
  useEffect(() => {
    if (!active || !status) {
      return;
    }

    if (status.stage === "ready") {
      toast.dismiss(STARTUP_BACKGROUND_OPERATION_TOAST_ID);
      return;
    }

    if (status.stage === "degraded") {
      toast.warning("Background restoration needs review.", {
        id: STARTUP_BACKGROUND_OPERATION_TOAST_ID,
        duration: Infinity,
        action: {
          label: "Review activity",
          onClick: () => useUiStore.getState().setCurrentView("activity"),
        },
      });
      return;
    }

    if (!status.backgroundComplete) {
      toast.loading("Restoring background work…", {
        id: STARTUP_BACKGROUND_OPERATION_TOAST_ID,
        duration: Infinity,
      });
    }
  }, [active, status]);

  return null;
}
