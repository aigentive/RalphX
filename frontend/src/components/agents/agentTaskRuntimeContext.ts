export type AgentTaskRuntimeContextType = "task_execution" | "review" | "merge";

export function isTaskRuntimeContextType(
  contextType: string,
): contextType is AgentTaskRuntimeContextType {
  return (
    contextType === "task_execution" ||
    contextType === "review" ||
    contextType === "merge"
  );
}
