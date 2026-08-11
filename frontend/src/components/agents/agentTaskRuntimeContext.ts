export type AgentTaskRuntimeContextType =
  | "task_execution"
  | "review"
  | "merge"
  | "branch_update";

export function isTaskRuntimeContextType(
  contextType: string,
): contextType is AgentTaskRuntimeContextType {
  return (
    contextType === "task_execution" ||
    contextType === "review" ||
    contextType === "merge" ||
    contextType === "branch_update"
  );
}
