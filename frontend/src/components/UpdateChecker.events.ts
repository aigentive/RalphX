import { useEffect } from "react";
import { useEventBus } from "@/providers/EventProvider";

const UPDATE_CHECK_EVENT = "ralphx://check-for-updates";
const RELEASE_NOTES_EVENT = "ralphx://show-release-notes";

export function useUpdateCheckerNativeEvents({
  checkForUpdates,
  enabled = true,
  openCurrentReleaseNotes,
}: {
  checkForUpdates: (options?: { manual?: boolean; force?: boolean }) => void;
  enabled?: boolean;
  openCurrentReleaseNotes: () => void;
}) {
  const eventBus = useEventBus();

  useEffect(() => {
    if (!enabled) {
      return undefined;
    }

    const unsubscribeUpdateCheck = eventBus.subscribe(UPDATE_CHECK_EVENT, () => {
      void checkForUpdates({ manual: true, force: true });
    });

    const unsubscribeReleaseNotes = eventBus.subscribe(RELEASE_NOTES_EVENT, () => {
      void openCurrentReleaseNotes();
    });

    return () => {
      unsubscribeUpdateCheck();
      unsubscribeReleaseNotes();
    };
  }, [checkForUpdates, enabled, eventBus, openCurrentReleaseNotes]);
}
