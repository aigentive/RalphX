import { z } from "zod";

import { backendApiUrl } from "@/api/backend";

const workflowRunStatusSchema = z.enum([
  "awaiting_approval",
  "queued",
  "running",
  "pause_requested",
  "paused",
  "recovering",
  "completed",
  "failed",
  "cancelled",
  "disabled",
]);

const workflowStepStatusSchema = z.enum([
  "pending",
  "running",
  "completed",
  "failed",
  "cancelled",
  "skipped",
]);

const workflowRunSchema = z.object({
  id: z.string(),
  script_id: z.string(),
  conversation_id: z.string(),
  status: workflowRunStatusSchema,
  created_at: z.string(),
  updated_at: z.string(),
  completed_at: z.string().nullable(),
  error: z.string().nullable(),
});

const workflowPhaseSchema = z.object({
  id: z.string(),
  key: z.string(),
  name: z.string(),
  ordinal: z.number(),
  status: workflowStepStatusSchema,
  error: z.string().nullable(),
});

const workflowInvocationSchema = z.object({
  id: z.string(),
  logical_key: z.string(),
  agent_name: z.string(),
  status: workflowStepStatusSchema,
  child_conversation_id: z.string().nullable(),
  error: z.string().nullable(),
});

const workflowLogSchema = z.object({
  sequence: z.number(),
  level: z.string(),
  message: z.string(),
  created_at: z.string(),
});

const workflowProgressSchema = z.object({
  run: workflowRunSchema,
  phases: z.array(workflowPhaseSchema),
  invocations: z.array(workflowInvocationSchema),
  logs: z.array(workflowLogSchema),
  usage: z.object({
    input_tokens: z.number(),
    output_tokens: z.number(),
    cache_creation_tokens: z.number(),
    cache_read_tokens: z.number(),
    estimated_usd: z.number(),
  }),
});

type WorkflowRunStatus = z.infer<typeof workflowRunStatusSchema>;
type WorkflowStepStatus = z.infer<typeof workflowStepStatusSchema>;

export interface AgentWorkflowRun {
  id: string;
  scriptId: string;
  conversationId: string;
  status: WorkflowRunStatus;
  createdAt: string;
  updatedAt: string;
  completedAt: string | null;
  error: string | null;
}

export interface AgentWorkflowProgress {
  run: AgentWorkflowRun;
  phases: Array<{
    id: string;
    key: string;
    name: string;
    ordinal: number;
    status: WorkflowStepStatus;
    error: string | null;
  }>;
  invocations: Array<{
    id: string;
    logicalKey: string;
    agentName: string;
    status: WorkflowStepStatus;
    childConversationId: string | null;
    error: string | null;
  }>;
  logs: Array<{
    sequence: number;
    level: string;
    message: string;
    createdAt: string;
  }>;
  usage: {
    inputTokens: number;
    outputTokens: number;
    cacheCreationTokens: number;
    cacheReadTokens: number;
    estimatedUsd: number;
  };
}

function transformRun(raw: z.infer<typeof workflowRunSchema>): AgentWorkflowRun {
  return {
    id: raw.id,
    scriptId: raw.script_id,
    conversationId: raw.conversation_id,
    status: raw.status,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    completedAt: raw.completed_at,
    error: raw.error,
  };
}

async function postJson(endpoint: string, body: Record<string, unknown>): Promise<unknown> {
  const response = await fetch(backendApiUrl(endpoint), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    const payload = (await response.json().catch(() => null)) as {
      error?: unknown;
    } | null;
    const detail = typeof payload?.error === "string" ? payload.error : response.statusText;
    throw new Error(`Workflow request failed: ${detail || response.status}`);
  }
  return response.json();
}

export const agentWorkflowApi = {
  async approveAndStart(input: {
    scriptId: string;
    scriptHash: string;
    permissionHash: string;
    launchId: string;
  }): Promise<AgentWorkflowRun> {
    await postJson("agent_workflows/scripts/approve", {
      script_id: input.scriptId,
      script_hash: input.scriptHash,
      permission_hash: input.permissionHash,
    });
    const raw = await postJson("agent_workflows/runs/start", {
      script_id: input.scriptId,
      script_hash: input.scriptHash,
      permission_hash: input.permissionHash,
      launch_id: input.launchId,
      args: {},
    });
    return transformRun(workflowRunSchema.parse(raw));
  },

  async getLatestRun(scriptId: string): Promise<AgentWorkflowRun | null> {
    const raw = await postJson("agent_workflows/runs/latest", { script_id: scriptId });
    if (raw === null) return null;
    return transformRun(workflowRunSchema.parse(raw));
  },

  async getProgress(runId: string): Promise<AgentWorkflowProgress> {
    const parsed = workflowProgressSchema.parse(
      await postJson("agent_workflows/runs/get", { run_id: runId }),
    );
    return {
      run: transformRun(parsed.run),
      phases: parsed.phases.map((phase) => ({
        id: phase.id,
        key: phase.key,
        name: phase.name,
        ordinal: phase.ordinal,
        status: phase.status,
        error: phase.error,
      })),
      invocations: parsed.invocations.map((invocation) => ({
        id: invocation.id,
        logicalKey: invocation.logical_key,
        agentName: invocation.agent_name,
        status: invocation.status,
        childConversationId: invocation.child_conversation_id,
        error: invocation.error,
      })),
      logs: parsed.logs.map((entry) => ({
        sequence: entry.sequence,
        level: entry.level,
        message: entry.message,
        createdAt: entry.created_at,
      })),
      usage: {
        inputTokens: parsed.usage.input_tokens,
        outputTokens: parsed.usage.output_tokens,
        cacheCreationTokens: parsed.usage.cache_creation_tokens,
        cacheReadTokens: parsed.usage.cache_read_tokens,
        estimatedUsd: parsed.usage.estimated_usd,
      },
    };
  },

  async pause(runId: string): Promise<void> {
    await postJson("agent_workflows/runs/pause", { run_id: runId });
  },

  async resume(runId: string): Promise<void> {
    await postJson("agent_workflows/runs/resume", { run_id: runId });
  },

  async cancel(runId: string): Promise<void> {
    await postJson("agent_workflows/runs/cancel", { run_id: runId });
  },
};

export function isAgentWorkflowTerminal(status: WorkflowRunStatus): boolean {
  return status === "completed" || status === "failed" || status === "cancelled";
}
