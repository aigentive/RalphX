import { z } from "zod";

import { backendFetch } from "@/api/backend";

const AgentTaskStateSchema = z.enum(["open", "active", "done", "dropped"]);

const AgentTaskSummaryResponseSchema = z.object({
  task_id: z.string(),
  task_number: z.number(),
  title: z.string(),
  state: AgentTaskStateSchema,
  owner_agent: z.string().nullable().optional(),
  blocked_by: z.array(z.string()),
  blocks: z.array(z.string()),
  availability: z.string(),
  updated_at: z.string(),
});

const AgentTaskListResponseSchema = z.object({
  success: z.boolean(),
  tasks: z.array(AgentTaskSummaryResponseSchema),
  error: z.string().nullable().optional(),
});

const AgentTaskListSummaryResponseSchema = z.object({
  list_id: z.string(),
  list_sequence: z.number(),
  task_count: z.number(),
  open_count: z.number(),
  active_count: z.number(),
  done_count: z.number(),
  dropped_count: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

const AgentTaskListsResponseSchema = z.object({
  success: z.boolean(),
  lists: z.array(AgentTaskListSummaryResponseSchema),
  error: z.string().nullable().optional(),
});

type RawAgentTaskSummary = z.infer<typeof AgentTaskSummaryResponseSchema>;
type RawAgentTaskListSummary = z.infer<typeof AgentTaskListSummaryResponseSchema>;

export type AgentTaskState = z.infer<typeof AgentTaskStateSchema>;

export interface AgentTaskSummary {
  taskId: string;
  taskNumber: number;
  title: string;
  state: AgentTaskState;
  ownerAgent: string | null;
  blockedBy: string[];
  blocks: string[];
  availability: string;
  updatedAt: string;
}

export interface AgentTaskListSummary {
  listId: string;
  listSequence: number;
  taskCount: number;
  openCount: number;
  activeCount: number;
  doneCount: number;
  droppedCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface ListAgentTasksInput {
  contextType: string;
  contextId: string;
  projectId?: string | null | undefined;
  includeDone?: boolean | undefined;
}

export interface ListAgentTasksForListInput extends ListAgentTasksInput {
  listId: string;
}

function transformAgentTaskSummary(raw: RawAgentTaskSummary): AgentTaskSummary {
  return {
    taskId: raw.task_id,
    taskNumber: raw.task_number,
    title: raw.title,
    state: raw.state,
    ownerAgent: raw.owner_agent ?? null,
    blockedBy: raw.blocked_by,
    blocks: raw.blocks,
    availability: raw.availability,
    updatedAt: raw.updated_at,
  };
}

function transformAgentTaskListSummary(
  raw: RawAgentTaskListSummary,
): AgentTaskListSummary {
  return {
    listId: raw.list_id,
    listSequence: raw.list_sequence,
    taskCount: raw.task_count,
    openCount: raw.open_count,
    activeCount: raw.active_count,
    doneCount: raw.done_count,
    droppedCount: raw.dropped_count,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

async function postJson<T>(endpoint: string, body: Record<string, unknown>): Promise<T> {
  const response = await backendFetch(endpoint, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Error(`Agent task request failed: ${response.status} ${response.statusText}`);
  }
  return (await response.json()) as T;
}

export const agentTaskApi = {
  async listAgentTasks(input: ListAgentTasksInput): Promise<AgentTaskSummary[]> {
    const raw = await postJson<unknown>("agent_tasks/list", {
      context_type: input.contextType,
      context_id: input.contextId,
      ...(input.projectId ? { project_id: input.projectId } : {}),
      include_done: input.includeDone ?? false,
    });
    const parsed = AgentTaskListResponseSchema.parse(raw);
    if (!parsed.success) {
      throw new Error(parsed.error ?? "Agent task list request failed");
    }
    return parsed.tasks.map(transformAgentTaskSummary);
  },

  async listAgentTaskLists(input: ListAgentTasksInput): Promise<AgentTaskListSummary[]> {
    const raw = await postJson<unknown>("agent_tasks/lists", {
      context_type: input.contextType,
      context_id: input.contextId,
      ...(input.projectId ? { project_id: input.projectId } : {}),
    });
    const parsed = AgentTaskListsResponseSchema.parse(raw);
    if (!parsed.success) {
      throw new Error(parsed.error ?? "Agent task list history request failed");
    }
    return parsed.lists.map(transformAgentTaskListSummary);
  },

  async listAgentTasksForList(
    input: ListAgentTasksForListInput,
  ): Promise<AgentTaskSummary[]> {
    const raw = await postJson<unknown>("agent_tasks/list_for_list", {
      context_type: input.contextType,
      context_id: input.contextId,
      ...(input.projectId ? { project_id: input.projectId } : {}),
      list_id: input.listId,
      include_done: input.includeDone ?? false,
    });
    const parsed = AgentTaskListResponseSchema.parse(raw);
    if (!parsed.success) {
      throw new Error(parsed.error ?? "Agent task list request failed");
    }
    return parsed.tasks.map(transformAgentTaskSummary);
  },

  listConversationTasks(args: {
    conversationId: string;
    projectId?: string | null | undefined;
    includeDone?: boolean | undefined;
  }): Promise<AgentTaskSummary[]> {
    return agentTaskApi.listAgentTasks({
      contextType: "conversation",
      contextId: args.conversationId,
      projectId: args.projectId,
      includeDone: args.includeDone,
    });
  },

  listConversationTaskLists(args: {
    conversationId: string;
    projectId?: string | null | undefined;
  }): Promise<AgentTaskListSummary[]> {
    return agentTaskApi.listAgentTaskLists({
      contextType: "conversation",
      contextId: args.conversationId,
      projectId: args.projectId,
    });
  },

  listConversationTaskListTasks(args: {
    conversationId: string;
    taskListId: string;
    projectId?: string | null | undefined;
    includeDone?: boolean | undefined;
  }): Promise<AgentTaskSummary[]> {
    return agentTaskApi.listAgentTasksForList({
      contextType: "conversation",
      contextId: args.conversationId,
      projectId: args.projectId,
      listId: args.taskListId,
      includeDone: args.includeDone,
    });
  },
} as const;
