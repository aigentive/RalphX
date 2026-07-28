import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { QueryClientProvider } from "@tanstack/react-query";

import { startupApi } from "@/api/startup";
import { StartupBackgroundStatus } from "@/components/StartupBackgroundStatus";
import { StartupScreen } from "@/components/StartupScreen";
import { useStartupStatus } from "@/hooks/useStartupStatus";
import { getQueryClient } from "@/lib/queryClient";
import { LOCAL_ENVIRONMENT_ID } from "@/lib/remote/active-environment";
import { clearPostUpdatePreparing, readFreshPostUpdatePreparingMarker } from "@/lib/postUpdatePreparing";

function loadAppShell() {
  return import("./App");
}

const LazyApp = lazy(loadAppShell);

function StartupShellPaintReporter({ onPainted }: { onPainted: () => void }) {
  useEffect(() => {
    let cancelled = false;
    const complete = () => {
      window.setTimeout(() => {
        if (!cancelled) onPainted();
      }, 0);
    };
    const frameId = window.requestAnimationFrame?.(complete);
    if (frameId === undefined) complete();

    return () => {
      cancelled = true;
      if (frameId !== undefined) window.cancelAnimationFrame?.(frameId);
    };
  }, [onPainted]);

  return null;
}

function StartupRootContent() {
  const {
    status,
    canMountApp,
    isStatusError,
    isRetrying,
    retry,
    retryError,
    statusError,
    refetch,
  } = useStartupStatus();
  const updateMarker = useMemo(() => readFreshPostUpdatePreparingMarker(), []);
  const reportedMilestoneRef = useRef<string | null>(null);
  const currentMilestoneRef = useRef<string | null>(null);
  const [shouldMountApp, setShouldMountApp] = useState(false);
  const [handoffComplete, setHandoffComplete] = useState(false);
  const [handoffError, setHandoffError] = useState<unknown>(null);
  const [recoveryMessage, setRecoveryMessage] = useState<string | null>(null);
  const currentMilestone = status ? `${status.bootId}:${status.attemptId}` : null;

  useLayoutEffect(() => {
    currentMilestoneRef.current = currentMilestone;
  }, [currentMilestone]);

  useEffect(() => {
    if (status?.appStateReady) void loadAppShell();
  }, [status?.appStateReady]);

  useEffect(() => {
    if (canMountApp) setShouldMountApp(true);
  }, [canMountApp]);

  useEffect(() => {
    setHandoffError(null);
    setRecoveryMessage(null);
  }, [currentMilestone]);

  const reportShellPaint = useCallback(() => {
    if (!status) return;
    const milestoneKey = `${status.bootId}:${status.attemptId}`;
    if (reportedMilestoneRef.current === milestoneKey) return;
    reportedMilestoneRef.current = milestoneKey;

    void startupApi.reportFrontendMilestone({
      bootId: status.bootId,
      attemptId: status.attemptId,
      milestone: "shell_painted",
    }).then(() => {
      if (currentMilestoneRef.current !== milestoneKey) {
        if (reportedMilestoneRef.current === milestoneKey) {
          reportedMilestoneRef.current = null;
        }
        return;
      }
      clearPostUpdatePreparing();
      setHandoffError(null);
      setHandoffComplete(true);
    }).catch((error: unknown) => {
      if (reportedMilestoneRef.current === milestoneKey) {
        reportedMilestoneRef.current = null;
      }
      setHandoffError(error);
    });
  }, [status]);

  const retryStartup = useCallback(() => {
    if (handoffError) {
      reportShellPaint();
    } else if (status?.stage === "failed" && status.retryAllowed) {
      void retry();
    } else {
      void refetch();
    }
  }, [handoffError, refetch, reportShellPaint, retry, status]);

  const openStartupLogs = useCallback(async () => {
    try {
      await startupApi.openLogs();
      setRecoveryMessage("Startup logs opened.");
    } catch {
      setRecoveryMessage("RalphX could not open startup logs. Quit and reopen RalphX to try again.");
    }
  }, []);

  const copyStartupDiagnostics = useCallback(async () => {
    try {
      const diagnostics = await startupApi.getDiagnostics();
      if (!navigator.clipboard?.writeText) {
        throw new Error("Clipboard API is unavailable");
      }
      await navigator.clipboard.writeText(JSON.stringify(diagnostics, null, 2));
      setRecoveryMessage("Startup diagnostics copied.");
    } catch {
      setRecoveryMessage("RalphX could not copy diagnostics. Quit and reopen RalphX to try again.");
    }
  }, []);

  const screenFailure = handoffError ?? retryError ?? statusError;
  const retryProps = handoffError
    ? {
        onRetry: retryStartup,
        retryAvailable: true,
        retryLabel: "Try shell handoff again",
      }
    : status?.stage === "failed" && status.retryAllowed
      ? { onRetry: retryStartup }
      : isStatusError
        ? { onRetry: retryStartup, retryLabel: "Check startup status again" }
        : {};

  return (
    <>
      {shouldMountApp && (
        <Suspense fallback={null}>
          <LazyApp {...(status ? { startupStatus: status } : {})} />
          <StartupShellPaintReporter onPainted={reportShellPaint} />
        </Suspense>
      )}
      <StartupBackgroundStatus active={handoffComplete} status={status} />
      {!handoffComplete && (
        <StartupScreen
          isRetrying={isRetrying}
          retryError={screenFailure}
          status={status}
          statusError={screenFailure}
          onCopyDiagnostics={copyStartupDiagnostics}
          onOpenLogs={openStartupLogs}
          {...retryProps}
          {...(recoveryMessage ? { recoveryMessage } : {})}
          {...(updateMarker?.version ? { updateVersion: updateMarker.version } : {})}
        />
      )}
    </>
  );
}

/**
 * Startup is a LOCAL concern — backend boot status, update markers, log access — and
 * it renders above `EnvironmentScopedProviders`, which owns the env-scoped client.
 *
 * This provider is therefore pinned to the local environment rather than defaulting to
 * `getTransportEnvironmentId()`. Unpinned, a session restored with a remote environment
 * active gave the startup screen the REMOTE client while the same subtree's
 * `EnvironmentScopedProviders` gave the app tree a second, differently-keyed one: two
 * providers, two caches, and startup queries landing in whichever the transport happened
 * to name at module evaluation.
 */
export function StartupRoot() {
  return (
    <QueryClientProvider client={getQueryClient(LOCAL_ENVIRONMENT_ID)}>
      <StartupRootContent />
    </QueryClientProvider>
  );
}
