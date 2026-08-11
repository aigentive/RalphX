import { z } from "zod";

export const ExecutionTaskAgentWorkspaceSchema = z.object({
  conversation_id: z.string(),
  project_id: z.string(),
  title: z.string(),
});

export type ExecutionTaskAgentWorkspaceRaw = z.infer<
  typeof ExecutionTaskAgentWorkspaceSchema
>;

export interface ExecutionTaskAgentWorkspace {
  conversationId: string;
  projectId: string;
  title: string;
}

export function transformExecutionTaskAgentWorkspace(
  raw: ExecutionTaskAgentWorkspaceRaw,
): ExecutionTaskAgentWorkspace {
  return {
    conversationId: raw.conversation_id,
    projectId: raw.project_id,
    title: raw.title,
  };
}
