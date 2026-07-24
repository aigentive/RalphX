import type { z } from "zod";

import {
  StartupDiagnosticsSchema,
  StartupStatusSchema,
} from "./startup.schemas";
import type {
  StartupDiagnostics,
  StartupProgress,
  StartupStatus,
} from "./startup.types";

function transformStartupProgress(
  raw: z.infer<typeof StartupStatusSchema>["progress"],
): StartupProgress | null {
  if (!raw) {
    return null;
  }

  return {
    completedUnits: raw.completed_units,
    totalUnits: raw.total_units,
  };
}

export function transformStartupStatus(
  raw: z.infer<typeof StartupStatusSchema>,
): StartupStatus {
  return {
    bootId: raw.boot_id,
    attemptId: raw.attempt_id,
    stage: raw.stage,
    startedAt: raw.started_at,
    stageStartedAt: raw.stage_started_at,
    completedAt: raw.completed_at ?? null,
    appStateReady: raw.app_state_ready,
    runtimeReady: raw.runtime_ready,
    backgroundComplete: raw.background_complete,
    retryAllowed: raw.retry_allowed,
    progress: transformStartupProgress(raw.progress),
    messageCode: raw.message_code,
    failureCode: raw.failure_code ?? null,
    diagnosticSummary: raw.diagnostic_summary ?? null,
  };
}

export function transformStartupDiagnostics(
  raw: z.infer<typeof StartupDiagnosticsSchema>,
): StartupDiagnostics {
  return {
    attemptId: raw.attempt_id,
    stage: raw.stage,
    messageCode: raw.message_code,
    failureCode: raw.failure_code ?? null,
    canRetry: raw.can_retry,
  };
}
