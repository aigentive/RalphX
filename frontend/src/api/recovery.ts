// Tauri invoke wrappers for recovery actions

import { z } from "zod";
import { typedInvoke } from "@/lib/tauri";
import { invoke } from "@tauri-apps/api/core";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";

export type RecoveryAction = "restart" | "cancel";

export async function recoverTaskExecution(taskId: string): Promise<boolean> {
  return typedInvoke(
    "recover_task_execution",
    { taskId },
    z.boolean()
  );
}

export async function resolveRecoveryPrompt(
  taskId: string,
  action: RecoveryAction
): Promise<boolean> {
  if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
    const intentSchema = z.object({
      requestId: z.string(),
      status: z.enum(["pending", "starting", "completed", "failed", "failedStale"]),
      errorCode: z.string().nullable().optional(),
      result: z.unknown().nullable().optional(),
    });
    const resultSchema = z.object({ applied: z.boolean(), benign: z.boolean() });
    const requested = intentSchema.parse(await invoke("request_remote_recovery_prompt_resolution", {
      input: { taskId, action },
    }));
    for (let attempt = 0; attempt < 1800; attempt += 1) {
      const request = intentSchema.parse(await invoke("get_remote_task_action_request", {
        requestId: requested.requestId,
      }));
      if (request.status === "completed") {
        return resultSchema.parse(request.result).applied;
      }
      if (request.status === "failed" || request.status === "failedStale") {
        throw new Error(request.errorCode ?? "REMOTE_RESUME_HOST_FAILED");
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    throw new Error("Timed out waiting for the host to resolve recovery");
  }
  return typedInvoke(
    "resolve_recovery_prompt",
    { taskId, action },
    z.boolean()
  );
}
