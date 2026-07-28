import { QueryClientProvider } from "@tanstack/react-query";
import { useEffect, useState, type ReactNode } from "react";

import { getQueryClient } from "@/lib/queryClient";
import { useEnvironmentStore } from "@/stores/environmentStore";
import { EventProvider } from "./EventProvider";

interface EnvironmentScopedProvidersProps {
  children: ReactNode;
}

/**
 * Environment switch = a full subtree swap, so it is the single most expensive click
 * in the app: 24 store resets, every query cache handed over, and a complete
 * unmount+remount of the app tree.
 *
 * Rule 24 forbids doing all of that in the click's own commit. The mounted
 * environment id therefore LAGS the store's active id by one paint: the first commit
 * only tears the old subtree down and paints a shell, and the remount under the new
 * environment's client happens after `requestAnimationFrame` + a macrotask.
 *
 * P-13 isolation is strengthened rather than weakened by the lag. During the switching
 * window NO application subtree is mounted, so there is no component left to issue a
 * query against the OLD environment's `QueryClient` while the transport already points
 * at the new environment — the failure mode a same-commit swap only avoids by luck.
 */
export function EnvironmentScopedProviders({
  children,
}: EnvironmentScopedProvidersProps) {
  const activeEnvironmentId = useEnvironmentStore(
    (state) => state.activeEnvironmentId,
  );
  const [mountedEnvironmentId, setMountedEnvironmentId] =
    useState(activeEnvironmentId);
  const switching = mountedEnvironmentId !== activeEnvironmentId;
  const queryClient = getQueryClient(mountedEnvironmentId);

  useEffect(() => {
    if (!switching) {
      return;
    }
    let cancelled = false;
    const commit = () => {
      window.setTimeout(() => {
        if (!cancelled) {
          setMountedEnvironmentId(activeEnvironmentId);
        }
      }, 0);
    };
    // `rAF` gets us past the paint that removed the old tree; the macrotask inside it
    // gets us past the browser's own commit work for that paint.
    const frameId = window.requestAnimationFrame?.(commit);
    if (frameId === undefined) {
      commit();
    }
    return () => {
      cancelled = true;
      if (frameId !== undefined) {
        window.cancelAnimationFrame?.(frameId);
      }
    };
  }, [switching, activeEnvironmentId]);

  useEffect(() => {
    // Single writer after initial client creation: Playwright always observes
    // the client belonging to the provider subtree that is currently mounted.
    if (typeof window !== "undefined" && !window.__TAURI_INTERNALS__) {
      window.__queryClient = queryClient;
    }
  }, [queryClient]);

  return (
    <QueryClientProvider key={mountedEnvironmentId} client={queryClient}>
      <EventProvider environmentId={mountedEnvironmentId}>
        {switching ? <EnvironmentSwitchShell /> : children}
      </EventProvider>
    </QueryClientProvider>
  );
}

/**
 * The lightweight frame that occupies the switching paint. Deliberately styleless
 * beyond layout: it exists to make the transition visible immediately, not to be a
 * loading experience with its own chrome to mount.
 */
function EnvironmentSwitchShell() {
  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="environment-switch-shell"
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        height: "100vh",
        fontSize: "0.8125rem",
        color: "var(--text-muted)",
      }}
    >
      Switching environment…
    </div>
  );
}
