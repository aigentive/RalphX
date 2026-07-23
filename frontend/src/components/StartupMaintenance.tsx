import { lazy, Suspense, useEffect, useState } from "react";

export const STARTUP_MAINTENANCE_IDLE_GRACE_MS = 1_000;

const LazyUpdateChecker = lazy(async () => {
  const { UpdateChecker } = await import("./UpdateChecker");
  return { default: UpdateChecker };
});

const LazyProviderCliUpdateChecker = lazy(async () => {
  const { ProviderCliUpdateChecker } = await import("./ProviderCliUpdateChecker");
  return { default: ProviderCliUpdateChecker };
});

interface StartupMaintenanceProps {
  backgroundSettled: boolean;
}

/** Defers optional update and release-note work until startup recovery is settled. */
export function StartupMaintenance({ backgroundSettled }: StartupMaintenanceProps) {
  const [enabled, setEnabled] = useState(false);

  useEffect(() => {
    if (!backgroundSettled) {
      setEnabled(false);
      return undefined;
    }

    const timeoutId = window.setTimeout(
      () => setEnabled(true),
      STARTUP_MAINTENANCE_IDLE_GRACE_MS,
    );
    return () => window.clearTimeout(timeoutId);
  }, [backgroundSettled]);

  if (!enabled) {
    return null;
  }

  return (
    <Suspense fallback={null}>
      <LazyUpdateChecker />
      <LazyProviderCliUpdateChecker />
    </Suspense>
  );
}
