import { useEffect, useState } from "react";

import type { StartupStatus } from "@/api/startup";
import { StartupBackgroundStatus } from "@/components/StartupBackgroundStatus";
import { StartupScreen } from "@/components/StartupScreen";
import { Toaster } from "@/components/ui/sonner";

type StartupVisualScenario =
  | "long-running"
  | "app-state-ready"
  | "background-restoring"
  | "failed";

const SCENARIOS: Record<StartupVisualScenario, StartupStatus> = {
  "long-running": {
    bootId: "startup-visual-boot",
    attemptId: 1,
    stage: "migrating",
    startedAt: new Date(Date.now() - 65_000).toISOString(),
    stageStartedAt: new Date(Date.now() - 2_000).toISOString(),
    completedAt: null,
    appStateReady: false,
    runtimeReady: false,
    backgroundComplete: false,
    retryAllowed: false,
    progress: { completedUnits: 2, totalUnits: 4 },
    messageCode: "migrating_workspace_data",
    failureCode: null,
    diagnosticSummary: null,
  },
  "app-state-ready": {
    bootId: "startup-visual-boot",
    attemptId: 1,
    stage: "app_state_ready",
    startedAt: new Date(Date.now() - 8_000).toISOString(),
    stageStartedAt: new Date(Date.now() - 2_000).toISOString(),
    completedAt: null,
    appStateReady: true,
    runtimeReady: false,
    backgroundComplete: false,
    retryAllowed: false,
    progress: null,
    messageCode: "startup_preparing_app",
    failureCode: null,
    diagnosticSummary: null,
  },
  failed: {
    bootId: "startup-visual-boot",
    attemptId: 1,
    stage: "failed",
    startedAt: new Date(Date.now() - 6_000).toISOString(),
    stageStartedAt: new Date(Date.now() - 2_000).toISOString(),
    completedAt: null,
    appStateReady: false,
    runtimeReady: false,
    backgroundComplete: false,
    retryAllowed: true,
    progress: null,
    messageCode: "startup_failed",
    failureCode: "database_open_failed",
    diagnosticSummary: "RalphX could not prepare local workspace data.",
  },
  "background-restoring": {
    bootId: "startup-visual-boot",
    attemptId: 1,
    stage: "background_recovery",
    startedAt: new Date(Date.now() - 6_000).toISOString(),
    stageStartedAt: new Date(Date.now() - 2_000).toISOString(),
    completedAt: null,
    appStateReady: true,
    runtimeReady: true,
    backgroundComplete: false,
    retryAllowed: false,
    progress: null,
    messageCode: "startup_restoring_interrupted_work",
    failureCode: null,
    diagnosticSummary: null,
  },
};

function resolveScenario(value: string | null): StartupVisualScenario {
  return value === "app-state-ready" || value === "background-restoring" || value === "failed"
    ? value
    : "long-running";
}

export function StartupVisualTestPage({ scenario }: { scenario: string | null }) {
  const resolvedScenario = resolveScenario(scenario);
  const theme = new URLSearchParams(window.location.search).get("theme") ?? "dark";
  const [workspaceOpened, setWorkspaceOpened] = useState(false);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);

  if (resolvedScenario === "background-restoring") {
    return (
      <>
        <main
          className="min-h-screen p-6"
          style={{
            backgroundColor: "var(--app-content-bg)",
            color: "var(--text-primary)",
          }}
        >
          <section
            className="rounded-lg border p-5"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-default)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            <h1 className="text-lg font-semibold">Projects</h1>
            <p className="mt-2 text-sm" style={{ color: "var(--text-secondary)" }}>
              Your workspace is interactive while interrupted work is restored.
            </p>
            <button
              className="mt-4 rounded-md px-3 py-2 text-sm font-medium"
              data-testid="safe-shell-action"
              onClick={() => setWorkspaceOpened(true)}
              style={{
                backgroundColor: "var(--accent-primary)",
                color: "var(--text-on-accent)",
              }}
              type="button"
            >
              {workspaceOpened ? "Workspace opened" : "Open workspace"}
            </button>
          </section>
        </main>
        <Toaster />
        <StartupBackgroundStatus
          active
          status={SCENARIOS["background-restoring"]}
        />
      </>
    );
  }

  const recoveryProps = resolvedScenario === "failed"
    ? {
        onCopyDiagnostics: async () => undefined,
        onOpenLogs: async () => undefined,
        onRetry: () => undefined,
      }
    : {};
  return <StartupScreen {...recoveryProps} status={SCENARIOS[resolvedScenario]} updateVersion="0.12.3" />;
}
