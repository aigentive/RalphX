import { api } from "@/lib/tauri";

export async function resumeExecutionIfStopped(projectId: string): Promise<boolean> {
  const executionStatus = await api.execution.getStatus(projectId).catch((error: unknown) => {
    const detail = error instanceof Error ? error.message : String(error);
    throw new Error(`Unable to read execution status: ${detail}`);
  });
  if (executionStatus.haltMode !== "stopped") {
    return false;
  }

  await api.execution.resume(projectId);
  return true;
}
