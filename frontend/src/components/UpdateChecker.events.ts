import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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
  useEffect(() => {
    if (!enabled) {
      return undefined;
    }

    const unlisteners: UnlistenFn[] = [];
    let isMounted = true;

    void listen(UPDATE_CHECK_EVENT, () => {
      void checkForUpdates({ manual: true, force: true });
    }).then((unlisten) => {
      if (isMounted) {
        unlisteners.push(unlisten);
      } else {
        unlisten();
      }
    });

    void listen(RELEASE_NOTES_EVENT, () => {
      void openCurrentReleaseNotes();
    }).then((unlisten) => {
      if (isMounted) {
        unlisteners.push(unlisten);
      } else {
        unlisten();
      }
    });

    return () => {
      isMounted = false;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [checkForUpdates, enabled, openCurrentReleaseNotes]);
}
