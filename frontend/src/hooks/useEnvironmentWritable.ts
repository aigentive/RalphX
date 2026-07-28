/**
 * Read-only mode for a degraded remote connection (PR 2.7-a).
 *
 * A remote environment that is reconnecting, offline, suspended, or blocked can still
 * RENDER: every query result already in its cache stays on screen, because throwing the
 * board away on a dropped socket is strictly worse than showing the last known truth.
 * What it cannot do is accept mutations — the proxy will refuse them, and an offline
 * queue is an explicit v1 non-goal.
 *
 * FAIL CLOSED: an environment with no presentation reported yet reads as NOT writable.
 * A remote environment is writable only while its supervisor says `connected`; local is
 * always writable because it has no supervisor and no transport to lose.
 */

import { useMemo } from "react";

import { useEnvironmentStore } from "@/stores/environmentStore";
import { LOCAL_ENVIRONMENT_ID } from "@/lib/remote/active-environment";

export interface EnvironmentWritableState {
  readonly writable: boolean;
  /** Why not, ready for a toast/tooltip; `null` when writable. */
  readonly reason: string | null;
}

const WRITABLE: EnvironmentWritableState = { writable: true, reason: null };

export function environmentWritableReason(
  environmentName: string,
  presentation: string | undefined
): string {
  switch (presentation) {
    case "offline":
      return `You're offline — changes to "${environmentName}" can't be made right now.`;
    case "error":
      return `"${environmentName}" needs attention before changes can be made.`;
    case "connecting":
      return `Still connecting to "${environmentName}" — changes can't be made yet.`;
    case "suspended":
      return `"${environmentName}" is paused in the background — changes can't be made right now.`;
    default:
      // Covers `reconnecting` AND the never-reported case, which is the same thing to a
      // user: there is no confirmed connection to write through.
      return `"${environmentName}" is reconnecting — changes can't be made right now.`;
  }
}

export function useEnvironmentWritable(): EnvironmentWritableState {
  // Primitive selectors only. A selector returning a fresh object would fail zustand's
  // reference check on every store write and re-render every mutation surface in the
  // app on each heartbeat-driven state change.
  const isLocal = useEnvironmentStore((state) => {
    if (state.activeEnvironmentId === LOCAL_ENVIRONMENT_ID) return true;
    const entry = state.environments.find(
      (candidate) => candidate.id === state.activeEnvironmentId
    );
    return entry?.kind === "local";
  });
  const environmentName = useEnvironmentStore(
    (state) =>
      state.environments.find(
        (candidate) => candidate.id === state.activeEnvironmentId
      )?.name ?? state.activeEnvironmentId
  );
  const presentation = useEnvironmentStore(
    (state) =>
      state.connectionPresentations[state.activeEnvironmentId]?.presentation
  );

  return useMemo(() => {
    if (isLocal || presentation === "connected") {
      return WRITABLE;
    }
    return {
      writable: false,
      reason: environmentWritableReason(environmentName, presentation),
    };
  }, [environmentName, isLocal, presentation]);
}
