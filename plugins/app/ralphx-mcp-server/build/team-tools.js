const memberName = {
    type: "string",
    description: "Unique normalized Team member name. Never pass an id.",
};
export const TEAM_TOOLS = [
    {
        name: "team_add_member",
        description: "Add a lazy standing Team member. This creates durable member identity only; it does not spawn a provider process.",
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
        description: "Assign one caller-led task to an idle Team member by name. Write work requires declared reservation surfaces; the backend resolves Team and run authority from this coordinator context.",
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
        description: "List currently idle Team members for the calling coordinator. The backend resolves the active Team from trusted runtime context.",
        inputSchema: { type: "object", properties: {}, required: [], additionalProperties: false },
    },
    {
        name: "team_stop_member",
        description: "Stop one Team member by normalized name. The backend resolves the Team and member generation from the caller context.",
        inputSchema: {
            type: "object",
            properties: { member_name: memberName },
            required: ["member_name"],
            additionalProperties: false,
        },
    },
    {
        name: "team_send_message",
        description: "Send a durable Team message. Coordinators may target one member or broadcast; members may target the coordinator or broadcast. The backend derives sender identity and idempotency from trusted runtime context.",
        inputSchema: {
            type: "object",
            properties: {
                target: { type: "string", enum: ["coordinator", "member", "broadcast"] },
                member_name: memberName,
                kind: {
                    type: "string",
                    enum: ["instruction", "result", "question", "status", "control", "approval"],
                },
                content: { type: "string", description: "Bounded Team message content." },
            },
            required: ["target", "content"],
            additionalProperties: false,
        },
    },
    {
        name: "team_roster",
        description: "Read the bounded name, role, and status roster for the caller's current Team. The backend resolves Team membership from trusted runtime context.",
        inputSchema: { type: "object", properties: {}, required: [], additionalProperties: false },
    },
    {
        name: "team_status",
        description: "Read per-member liveness (running state, last activity, latest run) joined to the roster for the caller's current Team. Coordinator-only. The backend resolves Team membership from trusted runtime context.",
        inputSchema: { type: "object", properties: {}, required: [], additionalProperties: false },
    },
];
const TEAM_TOOL_NAMES = new Set(TEAM_TOOLS.map((tool) => tool.name));
export function isTeamToolName(name) {
    return TEAM_TOOL_NAMES.has(name);
}
export async function callTeamTool(name, callTauri, callTauriGet, args, runtimeContext) {
    const headers = teamHeaders(name, runtimeContext);
    const body = args && typeof args === "object" ? args : {};
    switch (name) {
        case "team_add_member":
            return callTauri("managed_team/member", body, { headers });
        case "team_assign":
            return callTauri("managed_team/member/assign", body, { headers });
        case "team_list":
            return callTauriGet("managed_team/members/idle", { headers });
        case "team_stop_member":
            return callTauri("managed_team/member/stop", body, { headers });
        case "team_send_message":
            return callTauri("managed_team/message", body, { headers });
        case "team_roster":
            return callTauriGet("managed_team/member/roster", { headers });
        case "team_status":
            return callTauriGet("managed_team/members/status", { headers });
        default:
            throw new Error(`Unsupported Team tool: ${name}`);
    }
}
function teamHeaders(toolName, runtimeContext) {
    const conversationId = runtimeContext?.conversationId?.trim() ?? "";
    const agentRunId = runtimeContext?.agentRunId?.trim() ?? "";
    if (!conversationId || !agentRunId) {
        throw new Error(`${toolName} requires trusted Team conversation and run context.`);
    }
    return {
        "x-ralphx-conversation-id": conversationId,
        "x-ralphx-agent-run-id": agentRunId,
    };
}
//# sourceMappingURL=team-tools.js.map