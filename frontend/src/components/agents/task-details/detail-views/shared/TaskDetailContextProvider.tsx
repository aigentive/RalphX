import type { ReactNode } from "react";
import type { Task } from "@/types/task";
import {
  TaskDetailContext,
  useTaskDetailContextData,
  type TaskDetailViewMode,
} from "./TaskDetailContext";

export function TaskDetailContextProvider({
  task,
  viewMode,
  children,
}: {
  task: Task;
  viewMode: TaskDetailViewMode;
  children: ReactNode;
}) {
  const model = useTaskDetailContextData(task, viewMode);
  return (
    <TaskDetailContext.Provider value={model}>
      {children}
    </TaskDetailContext.Provider>
  );
}
