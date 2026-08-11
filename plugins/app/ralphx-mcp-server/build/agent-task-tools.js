const metadataSchema = {
    type: "object",
    additionalProperties: true,
    description: "Optional JSON metadata to merge into the task.",
};
const taskRefProperty = {
    type: "string",
    description: "Agent task number or task_id returned by create_agent_task or list_agent_tasks.",
};
const dependencyListProperty = {
    type: "array",
    items: { type: "string" },
    description: "Agent task numbers or task_id values.",
};
export const AGENT_TASK_TOOLS = [
    {
        name: "create_agent_task",
        description: "Create a lightweight agent task in the current RalphX conversation/run ledger. Use it for multi-step todo items, delegated work, and dependencies that agents need to coordinate. Do not create a task for genuinely single-step work; non-trivial work should be decomposed into multiple concrete tasks before claiming.",
        inputSchema: {
            type: "object",
            properties: {
                title: {
                    type: "string",
                    description: "Short task title.",
                },
                details: {
                    type: "string",
                    description: "Concrete requirements, context, or acceptance criteria.",
                },
                active_label: {
                    type: "string",
                    description: "Optional present-tense status label shown while the task is active.",
                },
                owner_agent: {
                    type: "string",
                    description: "Optional agent name that owns the task.",
                },
                metadata: metadataSchema,
                blocked_by: dependencyListProperty,
                blocks: dependencyListProperty,
            },
            required: ["title", "details"],
            additionalProperties: false,
        },
    },
    {
        name: "get_agent_task",
        description: "Read one agent task with full details, including all declared dependencies and unresolved blockers.",
        inputSchema: {
            type: "object",
            properties: {
                task_ref: taskRefProperty,
            },
            required: ["task_ref"],
            additionalProperties: false,
        },
    },
    {
        name: "list_agent_tasks",
        description: "List current agent tasks for this RalphX context. By default resolved tasks are hidden and blocked_by only includes unresolved blockers.",
        inputSchema: {
            type: "object",
            properties: {
                include_done: {
                    type: "boolean",
                    description: "Include done and dropped tasks in the list.",
                },
            },
            additionalProperties: false,
        },
    },
    {
        name: "update_agent_task",
        description: "Update an agent task's fields, owner, state, metadata, or dependencies. Metadata is merged; keys with null values are removed. Use state=dropped to clean up an accidental single-task ledger before continuing without the ledger.",
        inputSchema: {
            type: "object",
            properties: {
                task_ref: taskRefProperty,
                title: { type: "string" },
                details: { type: "string" },
                active_label: {
                    type: ["string", "null"],
                    description: "Replacement active label, or null to clear it.",
                },
                owner_agent: {
                    type: ["string", "null"],
                    description: "Replacement owner agent, or null to clear ownership.",
                },
                state: {
                    type: "string",
                    enum: ["open", "active", "done", "dropped"],
                },
                metadata: metadataSchema,
                add_blocked_by: dependencyListProperty,
                add_blocks: dependencyListProperty,
                remove_blocked_by: dependencyListProperty,
                remove_blocks: dependencyListProperty,
            },
            required: ["task_ref"],
            additionalProperties: false,
        },
    },
    {
        name: "claim_agent_task",
        description: "Claim a ready agent task for an agent and move it to active. The backend rejects claims while unresolved blockers remain or when the ledger has only one meaningful task; decompose the ledger or mark the lone task dropped before continuing.",
        inputSchema: {
            type: "object",
            properties: {
                task_ref: taskRefProperty,
                owner_agent: {
                    type: "string",
                    description: "Optional owner override. Defaults to the current caller agent.",
                },
            },
            required: ["task_ref"],
            additionalProperties: false,
        },
    },
    {
        name: "complete_agent_task",
        description: "Mark an agent task done and optionally merge completion metadata into the task. The backend rejects completion of an accidental single-task ledger; decompose it or mark the lone task dropped instead.",
        inputSchema: {
            type: "object",
            properties: {
                task_ref: taskRefProperty,
                metadata: metadataSchema,
            },
            required: ["task_ref"],
            additionalProperties: false,
        },
    },
    {
        name: "get_delegate_assignment",
        description: "Inspect the exact caller task bound to the current delegated run. This does not enumerate the caller ledger and accepts no orchestration identity.",
        inputSchema: {
            type: "object",
            properties: {},
            additionalProperties: false,
        },
    },
    {
        name: "complete_delegate_assignment",
        description: "Request completion of the exact task bound to the current delegated run. All meaningful delegate-local tasks must already be resolved; the caller task remains active until this exact run exits successfully.",
        inputSchema: {
            type: "object",
            properties: {
                metadata: {
                    ...metadataSchema,
                    description: "Optional completion metadata to merge after successful terminal settlement.",
                },
            },
            additionalProperties: false,
        },
    },
    {
        name: "release_delegate_assignment",
        description: "Request release of the exact task bound to the current delegated run when the work cannot be completed. The task remains unavailable until this exact run terminates.",
        inputSchema: {
            type: "object",
            properties: {
                reason: {
                    type: "string",
                    description: "Concise reason the assigned work must return to its caller.",
                },
            },
            required: ["reason"],
            additionalProperties: false,
        },
    },
];
export const AGENT_TASK_TOOL_NAMES = AGENT_TASK_TOOLS.map((tool) => tool.name);
//# sourceMappingURL=agent-task-tools.js.map