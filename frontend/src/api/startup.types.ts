export const STARTUP_STAGES = [
  "creating_window",
  "opening_database",
  "migrating",
  "loading_settings",
  "startup_cleanup",
  "registering_state",
  "app_state_ready",
  "binding_local_runtime",
  "safety_recovery",
  "runtime_ready",
  "background_recovery",
  "ready",
  "degraded",
  "failed",
] as const;

export type StartupStage = (typeof STARTUP_STAGES)[number];

export interface StartupProgress {
  completedUnits: number;
  totalUnits: number;
}

export interface StartupStatus {
  bootId: string;
  attemptId: number;
  stage: StartupStage;
  startedAt: string;
  stageStartedAt: string;
  completedAt: string | null;
  appStateReady: boolean;
  runtimeReady: boolean;
  backgroundComplete: boolean;
  retryAllowed: boolean;
  progress: StartupProgress | null;
  messageCode: string;
  failureCode: string | null;
  diagnosticSummary: string | null;
}

export interface StartupDiagnostics {
  attemptId: number;
  stage: StartupStage;
  messageCode: string;
  failureCode: string | null;
  canRetry: boolean;
}

export type StartupFrontendMilestone = "shell_painted";

export interface ReportStartupFrontendMilestoneInput {
  bootId: string;
  attemptId: number;
  milestone: StartupFrontendMilestone;
}
