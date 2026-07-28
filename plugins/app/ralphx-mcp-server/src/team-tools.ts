import { Tool } from "@modelcontextprotocol/sdk/types.js";
import type { TauriCallOptions } from "./tauri-client.js";

type TauriPost = (
  path: string,
  body: Record<string, unknown>,
  options?: TauriCallOptions
) => Promise<unknown>;

type TauriGet = (path: string, options?: TauriCallOptions) => Promise<unknown>;

export type TeamToolRuntimeContext = {
  conversationId?: string;
  agentRunId?: string;
};

const memberName = {
  type: "string",
  description: "Unique normalized Team member name. Never pass an id.",
};

export const TEAM_TOOLS: Tool[] = [
  {
    name: "team_add_member",
    description:
      "Add a lazy standing Team member. This creates durable member identity only; it does not spawn a provider process.",
    inputSchema: {
      type: "object",
      properties: {
        name: memberName,
        canonical_agent_name: {
          type: "string",
          description: "Canonical RalphX agent type for the member.",
        },
        role_summary: { type: "string", description: "Short assignment role." },
        harness: { type: "string", enum: ["claude", "codex"] },
        logical_model: { type: "string" },
        logical_effort: { type: "string" },
      },
      required: ["name", "canonical_agent_name", "role_summary"],
      additionalProperties: false,
    },
  },
  {
    name: "team_assign",
    description:
      "Assign one caller-led task to an idle Team member by name. Write work requires declared reservation surfaces; the backend resolves Team and run authority from this coordinator context.",
    inputSchema: {
      type: "object",
      properties: {
        member_name: memberName,
        task_ref: { type: "string", description: "Caller ledger task number or task id." },
        work_classification: {
          type: "string",
          enum: ["read_only", "write", "validator"],
        },
        writable_paths: { type: "array", items: { type: "string" } },
        generated_outputs: { type: "array", items: { type: "string" } },
        resource_locks: { type: "array", items: { type: "string" } },
      },
      required: ["member_name", "task_ref", "work_classification"],
      additionalProperties: false,
    },
  },
  {
    name: "team_list",
    description:
      "List currently idle Team members for the calling coordinator. The backend resolves the active Team from trusted runtime context.",
    inputSchema: { type: "object", properties: {}, required: [], additionalProperties: false },
  },
  {
    name: "team_stop_member",
    description:
      "Stop one Team member by normalized name. The backend resolves the Team and member generation from the caller context.",
    inputSchema: {
      type: "object",
      properties: { member_name: memberName },
      required: ["member_name"],
      additionalProperties: false,
    },
  },
];

const TEAM_TOOL_NAMES = new Set(TEAM_TOOLS.map((tool) => tool.name));

export function isTeamToolName(name: string): boolean {
  return TEAM_TOOL_NAMES.has(name);
}

export async function callTeamTool(
  name: string,
  callTauri: TauriPost,
  callTauriGet: TauriGet,
  args: unknown,
  runtimeContext?: TeamToolRuntimeContext
): Promise<unknown> {
  const headers = teamHeaders(name, runtimeContext);
  const body = args && typeof args === "object" ? args as Record<string, unknown> : {};
  switch (name) {
    case "team_add_member":
      return callTauri("managed_team/member", body, { headers });
    case "team_assign":
      return callTauri("managed_team/member/assign", body, { headers });
    case "team_list":
      return callTauriGet("managed_team/members/idle", { headers });
    case "team_stop_member":
      return callTauri("managed_team/member/stop", body, { headers });
    default:
      throw new Error(`Unsupported Team tool: ${name}`);
  }
}

function teamHeaders(
  toolName: string,
  runtimeContext?: TeamToolRuntimeContext
): Record<string, string> {
  const conversationId = runtimeContext?.conversationId?.trim() ?? "";
  const agentRunId = runtimeContext?.agentRunId?.trim() ?? "";
  if (!conversationId || !agentRunId) {
    throw new Error(`${toolName} requires trusted coordinator conversation and run context.`);
  }
  return {
    "x-ralphx-conversation-id": conversationId,
    "x-ralphx-agent-run-id": agentRunId,
  };
}
