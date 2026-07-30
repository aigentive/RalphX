import { Suspense, useCallback, useEffect, useState } from "react";

import { lazyWithRetry } from "@/lib/lazy-with-retry";
import { useUpdateCheckerNativeEvents } from "./UpdateChecker.events";

const LazyUpdateChecker = lazyWithRetry(async () => {
  const { UpdateChecker } = await import("./UpdateChecker");
  return { default: UpdateChecker };
});

const LazyProviderCliUpdateChecker = lazyWithRetry(async () => {
  const { ProviderCliUpdateChecker } = await import("./ProviderCliUpdateChecker");
  return { default: ProviderCliUpdateChecker };
});

export const STARTUP_MAINTENANCE_IDLE_GRACE_MS = 1_000;

interface StartupMaintenanceProps {
  backgroundSettled: boolean;
}

/** Defers optional update and release-note work until startup recovery is settled. */
export function StartupMaintenance({ backgroundSettled }: StartupMaintenanceProps) {
  const [automaticMaintenanceEnabled, setAutomaticMaintenanceEnabled] =
    useState(false);
  const [checkForUpdatesRequest, setCheckForUpdatesRequest] = useState(0);
  const [openReleaseNotesRequest, setOpenReleaseNotesRequest] = useState(0);

  const requestUpdateCheck = useCallback(() => {
    setCheckForUpdatesRequest((current) => current + 1);
  }, []);
  const requestReleaseNotes = useCallback(() => {
    setOpenReleaseNotesRequest((current) => current + 1);
  }, []);

  useUpdateCheckerNativeEvents({
    checkForUpdates: requestUpdateCheck,
    openCurrentReleaseNotes: requestReleaseNotes,
  });

  useEffect(() => {
    if (!backgroundSettled) {
      setAutomaticMaintenanceEnabled(false);
      return undefined;
    }

    const timeoutId = window.setTimeout(
      () => setAutomaticMaintenanceEnabled(true),
      STARTUP_MAINTENANCE_IDLE_GRACE_MS,
    );
    return () => window.clearTimeout(timeoutId);
  }, [backgroundSettled]);

  const shouldMountUpdateChecker =
    automaticMaintenanceEnabled ||
    checkForUpdatesRequest > 0 ||
    openReleaseNotesRequest > 0;

  return (
    <>
      {shouldMountUpdateChecker && (
        <Suspense fallback={null}>
          <LazyUpdateChecker
            automaticMaintenanceEnabled={automaticMaintenanceEnabled}
            checkForUpdatesRequest={checkForUpdatesRequest}
            listenForNativeActions={false}
            openReleaseNotesRequest={openReleaseNotesRequest}
          />
        </Suspense>
      )}
      {automaticMaintenanceEnabled && (
        <Suspense fallback={null}>
          <LazyProviderCliUpdateChecker />
        </Suspense>
      )}
    </>
  );
}
