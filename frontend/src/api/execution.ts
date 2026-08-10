// Tauri invoke wrappers for execution control with type safety using Zod schemas

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";
import { DEFAULT_PROJECT_SETTINGS } from "@/types/settings";
import {
  ExecutionStatusResponseSchema,
  ExecutionCommandResponseSchema,
  ExecutionSettingsResponseSchema,
  GlobalExecutionSettingsResponseSchema,
} from "./execution.schemas";
import {
  transformExecutionStatus,
  transformExecutionCommand,
  transformExecutionSettings,
  transformExecutionSettingsInput,
  transformGlobalExecutionSettings,
  transformGlobalExecutionSettingsInput,
} from "./execution.transforms";
import type {
  ExecutionStatusResponse,
  ExecutionCommandResponse,
  ExecutionSettingsResponse,
  UpdateExecutionSettingsInput,
  GlobalExecutionSettingsResponse,
  UpdateGlobalExecutionSettingsInput,
} from "./execution.types";

// Re-export types for convenience

/**
 * True while the active environment is remote, so execution operations with spawn-free
 * facade twins use those twins instead of local-only commands.
 */
function remoteExecutionFacadeEnabled(): boolean {
  return isRemoteEnvironmentId(getTransportEnvironmentId());
}

export type {
  ExecutionHaltMode,
  ExecutionStatusResponse,
  ExecutionCommandResponse,
  ExecutionSettingsResponse,
  UpdateExecutionSettingsInput,
  GlobalExecutionSettingsResponse,
  UpdateGlobalExecutionSettingsInput,
} from "./execution.types";

// Re-export schemas for consumers that need validation
export {
  ExecutionStatusResponseSchema,
  ExecutionCommandResponseSchema,
  ExecutionSettingsResponseSchema,
  UpdateExecutionSettingsInputSchema,
  GlobalExecutionSettingsResponseSchema,
  UpdateGlobalExecutionSettingsInputSchema,
} from "./execution.schemas";

// Re-export transforms for consumers that need manual transformation
export {
  transformExecutionStatus,
  transformExecutionCommand,
  transformExecutionSettings,
  transformExecutionSettingsInput,
  transformGlobalExecutionSettings,
  transformGlobalExecutionSettingsInput,
} from "./execution.transforms";

// ============================================================================
// Typed Invoke Helper
// ============================================================================

async function typedInvokeWithTransform<TRaw, TResult>(
  cmd: string,
  args: Record<string, unknown>,
  schema: z.ZodType<TRaw>,
  transform: (raw: TRaw) => TResult
): Promise<TResult> {
  const result = await invoke(cmd, args);
  const validated = schema.parse(result);
  return transform(validated);
}

const RemoteResumeIntentSchema = z.object({
  requestId: z.string(),
  status: z.enum(["pending", "starting", "completed", "failed", "failedStale"]),
  errorCode: z.string().nullable().optional(),
  result: z.unknown().nullable().optional(),
});

async function remoteExecutionResume(projectId?: string): Promise<ExecutionCommandResponse> {
  const requested = RemoteResumeIntentSchema.parse(
    await invoke("request_remote_execution_resume", {
      input: { projectId: projectId ?? null },
    })
  );
  for (let attempt = 0; attempt < 1800; attempt += 1) {
    const request = attempt === 0 && requested.status !== "pending"
      ? requested
      : RemoteResumeIntentSchema.parse(
          await invoke("get_remote_execution_resume_request", {
            requestId: requested.requestId,
          })
        );
    if (request.status === "completed") {
      return transformExecutionCommand(ExecutionCommandResponseSchema.parse(request.result));
    }
    if (request.status === "failed" || request.status === "failedStale") {
      throw new Error(request.errorCode ?? "REMOTE_RESUME_HOST_FAILED");
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("Timed out waiting for the host to resume execution");
}

// ============================================================================
// API Object
// ============================================================================

/**
 * Execution control API wrappers for Tauri commands
 * Phase 82: All commands now accept optional projectId for per-project scoping
 */
export const executionApi = {
  /**
   * Get current execution status
   * Phase 82: Optional projectId scopes to specific project; if omitted, uses active project
   * @param projectId - Optional project ID to scope status to
   * @returns Execution status with pause state, running count, queued count
   */
  getStatus: (projectId?: string): Promise<ExecutionStatusResponse> => {
    const args = { projectId: projectId ?? null };
    return remoteExecutionFacadeEnabled()
      ? typedInvokeWithTransform(
          "get_remote_execution_status",
          args,
          ExecutionStatusResponseSchema,
          transformExecutionStatus
        )
      : typedInvokeWithTransform(
          "get_execution_status",
          args,
          ExecutionStatusResponseSchema,
          transformExecutionStatus
        );
  },

  /**
   * Pause execution (stops picking up new tasks)
   * Phase 82: Optional projectId scopes pause to specific project
   * @param projectId - Optional project ID to pause
   * @returns Command response with success and current status
   */
  pause: (projectId?: string): Promise<ExecutionCommandResponse> =>
    typedInvokeWithTransform(
      "pause_execution",
      { projectId: projectId ?? null },
      ExecutionCommandResponseSchema,
      transformExecutionCommand
    ),

  /**
   * Resume or start execution (relaunches paused work and reopens stopped queues)
   * Phase 82: Optional projectId scopes resume to specific project
   * @param projectId - Optional project ID to resume
   * @returns Command response with success and current status
   */
  resume: (projectId?: string): Promise<ExecutionCommandResponse> =>
    remoteExecutionFacadeEnabled()
      ? remoteExecutionResume(projectId)
      : typedInvokeWithTransform(
          "resume_execution",
          { projectId: projectId ?? null },
          ExecutionCommandResponseSchema,
          transformExecutionCommand
        ),

  /**
   * Stop execution (halts active work and requires manual restart)
   * Phase 82: Optional projectId scopes stop to specific project
   * @param projectId - Optional project ID to stop
   * @returns Command response with success and current status
   */
  stop: (projectId?: string): Promise<ExecutionCommandResponse> =>
    typedInvokeWithTransform(
      "stop_execution",
      { projectId: projectId ?? null },
      ExecutionCommandResponseSchema,
      transformExecutionCommand
    ),

  /**
   * Get execution settings from database
   * Phase 82: Optional projectId gets per-project settings; if omitted, uses global defaults
   * @param projectId - Optional project ID for per-project settings
   * @returns Execution settings with max concurrent tasks, auto-commit, pause on failure
   */
  getSettings: async (projectId?: string): Promise<ExecutionSettingsResponse> => {
    const result = await invoke("get_execution_settings", { projectId: projectId ?? null });

    if (!result) {
      const defaults = DEFAULT_PROJECT_SETTINGS.execution;
      return transformExecutionSettings({
        max_concurrent_tasks: defaults.max_concurrent_tasks,
        project_ideation_max: defaults.project_ideation_max,
        auto_commit: defaults.auto_commit,
        pause_on_failure: defaults.pause_on_failure,
        agent_workspace_pr_autofix_default: defaults.agent_workspace_pr_autofix_default,
        agent_workspace_pr_auto_merge_default:
          defaults.agent_workspace_pr_auto_merge_default,
      });
    }

    const validated = ExecutionSettingsResponseSchema.parse(result);
    return transformExecutionSettings(validated);
  },

  /**
   * Update execution settings in database
   * Phase 82: Optional projectId updates per-project settings; if omitted, updates global defaults
   * Also syncs ExecutionState when max_concurrent_tasks changes
   * @param input - Settings to update
   * @param projectId - Optional project ID for per-project settings
   * @returns Updated execution settings
   */
  updateSettings: (
    input: UpdateExecutionSettingsInput,
    projectId?: string
  ): Promise<ExecutionSettingsResponse> => {
    const args = {
      input: transformExecutionSettingsInput(input),
      projectId: projectId ?? null,
    };
    // `get_execution_settings` is registered, so under a remote environment the pane already
    // shows the HOST's live values — but `update_execution_settings` is Elevated (it kicks
    // the scheduler and drains pending ideation sessions, both spawn sinks) and unregistered,
    // so saving them failed silently. The spawn-free twin persists and syncs the caps without
    // arming queued work. Two literal call sites, because P-11 requires literal names.
    return remoteExecutionFacadeEnabled()
      ? typedInvokeWithTransform(
          "update_remote_execution_settings",
          args,
          ExecutionSettingsResponseSchema,
          transformExecutionSettings
        )
      : typedInvokeWithTransform(
          "update_execution_settings",
          args,
          ExecutionSettingsResponseSchema,
          transformExecutionSettings
        );
  },

  /**
   * Set the active project for scoped execution operations
   * Phase 82: Backend tracks active project for commands that don't specify projectId
   * @param projectId - Project ID to set as active, or undefined to clear
   */
  setActiveProject: async (projectId?: string): Promise<void> => {
    await invoke("set_active_project", { projectId: projectId ?? null });
  },

  /**
   * Get global execution settings (cross-project cap)
   * Phase 82: Returns global_max_concurrent setting
   * @returns Global execution settings with globalMaxConcurrent
   */
  getGlobalSettings: (): Promise<GlobalExecutionSettingsResponse> =>
    typedInvokeWithTransform(
      "get_global_execution_settings",
      {},
      GlobalExecutionSettingsResponseSchema,
      transformGlobalExecutionSettings
    ),

  /**
   * Update global execution settings (cross-project cap)
   * Phase 82: Updates global_max_concurrent setting
   * @param input - Global settings to update
   * @returns Updated global execution settings
   */
  updateGlobalSettings: (
    input: UpdateGlobalExecutionSettingsInput
  ): Promise<GlobalExecutionSettingsResponse> =>
    typedInvokeWithTransform(
      "update_global_execution_settings",
      { input: transformGlobalExecutionSettingsInput(input) },
      GlobalExecutionSettingsResponseSchema,
      transformGlobalExecutionSettings
    ),
} as const;
