import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export type ValidationRunStatus =
  | "running"
  | "passed"
  | "failed"
  | "error"
  | "cancelled"
  | "skipped";

export type ValidationCommandStatus =
  | "running"
  | "passed"
  | "failed"
  | "error"
  | "timed_out"
  | "cancelled"
  | "skipped"
  | "cached";

export type ValidationCacheDecision =
  | "ran"
  | "cached"
  | "stale"
  | "forced"
  | "skipped";

export interface ValidationRunSummary {
  id: string;
  purpose: string;
  context_type: string;
  requested_by_agent: string | null;
  status: ValidationRunStatus;
  mode: string;
  policy_enabled: boolean;
  head_sha: string | null;
  head_short_sha: string | null;
  base_ref: string | null;
  started_at: string;
  completed_at: string | null;
  current_for_head?: boolean;
  current_for_execution_episode?: boolean;
  review_evidence_eligible?: boolean;
  ineligible_reason?: string | null;
}

export interface ValidationCommandSummary {
  id: string;
  command_source: string;
  command_ref: string | null;
  command: string;
  cwd: string;
  label: string | null;
  category: string;
  reason: string | null;
  related_files: string[];
  cache_decision: ValidationCacheDecision;
  status: ValidationCommandStatus;
  exit_code: number | null;
  duration_ms: number | null;
  stdout_snippet: string | null;
  stderr_snippet: string | null;
  stdout_log_path: string | null;
  stderr_log_path: string | null;
  created_at: string;
}

export interface LegacyValidationCacheSummary {
  validation_hint?: string | null;
  hint_message?: string | null;
  tests_ran?: boolean | null;
  tests_passed?: boolean | null;
  captured_at?: string | null;
}

export interface TaskValidationSummary {
  task_id: string;
  project_id: string;
  policy_enabled: boolean;
  latest_run: ValidationRunSummary | null;
  commands: ValidationCommandSummary[];
  legacy_validation_cache: LegacyValidationCacheSummary | null;
  disabled_reason: string | null;
}

export const taskValidationKeys = {
  all: ["task-validation"] as const,
  summary: (taskId: string) =>
    [...taskValidationKeys.all, "summary", taskId] as const,
};

export function useTaskValidationSummary(taskId: string) {
  return useQuery<TaskValidationSummary, Error>({
    queryKey: taskValidationKeys.summary(taskId),
    queryFn: () =>
      invoke<TaskValidationSummary>("get_task_validation_summary", { taskId }),
    enabled: Boolean(taskId),
    staleTime: 15_000,
  });
}
