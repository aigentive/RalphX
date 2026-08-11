import { useEffect, useState } from "react";

import type { StartupStage, StartupStatus } from "@/api/startup";
import { Button } from "@/components/ui/button";

const SLOW_START_THRESHOLD_MS = 3_000;

const STAGE_COPY: Record<StartupStage, { heading: string; description: string }> = {
  creating_window: { heading: "Preparing your workspace", description: "Starting the RalphX window." },
  opening_database: { heading: "Preparing workspace data", description: "Opening local workspace data." },
  compacting_database: { heading: "Reclaiming disk space", description: "Compacting local workspace data. This can take several minutes on a large database." },
  migrating: { heading: "Upgrading workspace data", description: "Applying safe workspace updates." },
  loading_settings: { heading: "Loading your settings", description: "Restoring local preferences." },
  startup_cleanup: { heading: "Restoring local services", description: "Cleaning up the previous session." },
  registering_state: { heading: "Preparing RalphX", description: "Connecting workspace services." },
  app_state_ready: { heading: "Preparing the app shell", description: "Loading the responsive workspace shell." },
  binding_local_runtime: { heading: "Restoring local services", description: "Starting local workspace services." },
  safety_recovery: { heading: "Checking interrupted work", description: "Safely restoring previous workspace activity." },
  runtime_ready: { heading: "Opening your workspace", description: "Finishing the responsive handoff." },
  background_recovery: { heading: "Restoring background work", description: "Your workspace is opening while recovery finishes safely." },
  ready: { heading: "RalphX is ready", description: "Your workspace is ready to use." },
  degraded: { heading: "RalphX is ready with attention needed", description: "Some background restoration needs review." },
  failed: { heading: "RalphX could not finish starting", description: "Your projects are safe. Review the startup details and try again." },
};

function useElapsedSeconds(startedAt: string | undefined): number | null {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!startedAt) return undefined;
    const intervalId = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(intervalId);
  }, [startedAt]);

  if (!startedAt) return null;
  const startedAtMs = Date.parse(startedAt);
  return Number.isNaN(startedAtMs) ? null : Math.max(0, Math.floor((now - startedAtMs) / 1_000));
}

function formatElapsed(seconds: number): string {
  return seconds < 60
    ? `${seconds} seconds`
    : `${Math.floor(seconds / 60)} minute${seconds >= 120 ? "s" : ""}`;
}

interface StartupScreenProps {
  status: StartupStatus | undefined;
  updateVersion?: string;
  statusError?: unknown;
  retryError?: unknown;
  onRetry?: () => void;
  retryLabel?: string;
  retryAvailable?: boolean;
  onOpenLogs?: () => Promise<void>;
  onCopyDiagnostics?: () => Promise<void>;
  recoveryMessage?: string;
  isRetrying?: boolean;
}

export function StartupScreen({
  status,
  updateVersion,
  statusError,
  retryError,
  onRetry,
  retryLabel = "Retry startup",
  retryAvailable,
  onOpenLogs,
  onCopyDiagnostics,
  recoveryMessage,
  isRetrying = false,
}: StartupScreenProps) {
  const elapsedSeconds = useElapsedSeconds(status?.startedAt);
  const isFailed = status?.stage === "failed" || Boolean(statusError) || Boolean(retryError);
  const copy = isFailed
    ? STAGE_COPY.failed
    : status ? STAGE_COPY[status.stage] : STAGE_COPY.creating_window;
  const progress = status?.progress ?? null;
  const hasProgress = progress !== null && progress.totalUnits > 0;
  const progressValue = hasProgress ? Math.min(progress.completedUnits, progress.totalUnits) : null;
  const diagnosticSummary = isFailed
    ? status?.diagnosticSummary ?? "RalphX could not prepare the current startup attempt."
    : null;
  const canRetry = Boolean(onRetry)
    && (retryAvailable ?? (status === undefined || status.retryAllowed));

  return (
    <main
      className="fixed inset-0 z-[100] flex min-h-screen flex-col"
      data-testid="startup-screen"
      style={{ backgroundColor: "var(--app-content-bg)", color: "var(--text-primary)" }}
    >
      <div
        className="h-12 flex-shrink-0 border-b"
        data-tauri-drag-region
        style={{
          backgroundColor: "var(--app-header-bg)",
          borderBottomColor: "var(--app-header-border)",
          borderBottomStyle: "solid",
          borderBottomWidth: "1px",
        }}
      />
      <div className="flex flex-1 items-center justify-center px-8 py-12">
        <div
          aria-live="polite"
          className="flex w-full max-w-[420px] flex-col items-center gap-4 rounded-lg border px-8 py-9 text-center"
          role="status"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-default)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          {!isFailed && (
            <div
              aria-hidden="true"
              className="h-8 w-8 animate-spin rounded-full border-2 border-transparent"
              style={{ borderRightColor: "var(--accent-primary)", borderTopColor: "var(--accent-primary)" }}
            />
          )}
          <div className="space-y-2">
            <p className="text-xs font-medium uppercase tracking-[0.14em]" style={{ color: "var(--text-muted)" }}>
              {updateVersion ? "Finishing the RalphX update" : "Starting RalphX"}
            </p>
            <h1 className="text-lg font-semibold">{copy.heading}</h1>
            <p className="text-sm" style={{ color: "var(--text-secondary)" }}>{copy.description}</p>
          </div>
          {hasProgress && progressValue !== null && (
            <div className="w-full space-y-2">
              <div
                aria-valuemax={progress.totalUnits}
                aria-valuemin={0}
                aria-valuenow={progressValue}
                className="h-2 w-full overflow-hidden rounded-full"
                role="progressbar"
                style={{ backgroundColor: "var(--bg-elevated)" }}
              >
                <div
                  className="h-full rounded-full"
                  style={{ backgroundColor: "var(--accent-primary)", width: `${(progressValue / progress.totalUnits) * 100}%` }}
                />
              </div>
              <p className="text-xs" style={{ color: "var(--text-muted)" }}>
                {progressValue} of {progress.totalUnits} complete
              </p>
            </div>
          )}
          {elapsedSeconds !== null && elapsedSeconds * 1_000 >= SLOW_START_THRESHOLD_MS && !isFailed && (
            <p className="text-xs" style={{ color: "var(--text-muted)" }}>
              Still working after {formatElapsed(elapsedSeconds)}. Your projects and work are safe.
            </p>
          )}
          {diagnosticSummary && (
            <p className="text-sm" style={{ color: "var(--status-error)" }}>{diagnosticSummary}</p>
          )}
          {isFailed && (
            <div className="flex w-full flex-col items-center gap-3">
              {canRetry && onRetry && (
                <Button disabled={isRetrying} onClick={onRetry} type="button">
                  {isRetrying ? "Retrying startup…" : retryLabel}
                </Button>
              )}
              {(onOpenLogs || onCopyDiagnostics) && (
                <div className="flex flex-wrap justify-center gap-2">
                  {onOpenLogs && (
                    <Button onClick={() => void onOpenLogs()} type="button" variant="outline">
                      Open Logs
                    </Button>
                  )}
                  {onCopyDiagnostics && (
                    <Button onClick={() => void onCopyDiagnostics()} type="button" variant="outline">
                      Copy Diagnostics
                    </Button>
                  )}
                </div>
              )}
              {recoveryMessage && (
                <p aria-live="polite" className="text-xs" style={{ color: "var(--text-muted)" }}>
                  {recoveryMessage}
                </p>
              )}
              {!canRetry && (
                <p className="text-xs" style={{ color: "var(--text-muted)" }}>
                  Quit RalphX completely, then reopen it to start a fresh session.
                </p>
              )}
            </div>
          )}
        </div>
      </div>
    </main>
  );
}
