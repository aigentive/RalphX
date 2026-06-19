/**
 * MCP tool definitions for plan artifact management
 * Used by ralphx-ideation agent to create and manage implementation plans
 */
/**
 * Plan artifact tools for ralphx-ideation agent
 * All tools are proxies that forward to Tauri backend via HTTP
 */
export const PLAN_TOOLS = [
    {
        name: "create_plan_artifact",
        description: "Create a new implementation plan artifact linked to the ideation session. Use this when the user describes a complex feature that needs architectural planning before breaking into tasks. The plan is stored as a Specification artifact and can be referenced by task proposals. " +
            "For child sessions that inherited a parent's plan: calling this creates a completely independent plan for the child session — it does NOT modify or copy from the parent's plan.",
        inputSchema: {
            type: "object",
            properties: {
                session_id: {
                    type: "string",
                    description: "The ideation session ID (provided in context)",
                },
                title: {
                    type: "string",
                    description: "Plan title (e.g., 'Real-time Collaboration Implementation Plan')",
                },
                content: {
                    type: "string",
                    description: "Plan content in markdown format. Should include architecture decisions, data flow, key implementation details, and considerations.",
                },
            },
            required: ["session_id", "title", "content"],
        },
    },
    {
        name: "update_plan_artifact",
        description: "Update an existing implementation plan's content. Creates a NEW version with a new artifact ID (immutable version chain). Stale artifact IDs are auto-resolved: you can pass any previous version's ID and it will resolve to the latest before updating. Linked proposals are automatically re-linked to the new version (plan_version_at_creation is preserved). The response includes `previous_artifact_id` and `session_id` for reference. You do NOT need to call get_session_plan between updates to refresh the ID. Caller-session routing for verification freeze bypass is derived automatically from live app context; do not pass it manually.",
        inputSchema: {
            type: "object",
            properties: {
                artifact_id: {
                    type: "string",
                    description: "The artifact ID of the plan to update. Can be any version ID — stale IDs are auto-resolved to the latest version.",
                },
                content: {
                    type: "string",
                    description: "Updated plan content in markdown format. This will create a new version of the artifact with a new ID.",
                },
            },
            required: ["artifact_id", "content"],
        },
    },
    {
        name: "link_proposals_to_plan",
        description: "Link multiple task proposals to an implementation plan. Use after creating proposals to establish the connection between the plan and its derived tasks. Stale artifact IDs are auto-resolved: you can pass any previous version's ID and it will resolve to the latest before linking. This enables traceability and allows the system to suggest updates when the plan changes.",
        inputSchema: {
            type: "object",
            properties: {
                proposal_ids: {
                    type: "array",
                    items: { type: "string" },
                    description: "Array of proposal IDs to link to the plan",
                },
                artifact_id: {
                    type: "string",
                    description: "The plan artifact ID to link proposals to. Can be any version ID — stale IDs are auto-resolved to the latest version.",
                },
            },
            required: ["proposal_ids", "artifact_id"],
        },
    },
    {
        name: "get_session_plan",
        description: "Get the implementation plan artifact for the current ideation session, if one exists. Use to check if a plan has already been created before suggesting a new one. " +
            "Response includes an `is_inherited` boolean: if true, the plan was inherited from a parent session and is read-only — call create_plan_artifact to create an independent plan for this session.",
        inputSchema: {
            type: "object",
            properties: {
                session_id: {
                    type: "string",
                    description: "The ideation session ID",
                },
            },
            required: ["session_id"],
        },
    },
    {
        name: "submit_plan_complexity_assessment",
        description: "Persist a complexity assessment for the current approved Plan-mode artifact version. " +
            "Only the ralphx-utility-plan-complexity helper should call this after grading an approved plan.",
        inputSchema: {
            type: "object",
            properties: {
                session_id: {
                    type: "string",
                    description: "Planning session ID for the approved plan",
                },
                artifact_id: {
                    type: "string",
                    description: "Approved plan artifact ID",
                },
                artifact_version: {
                    type: "integer",
                    description: "Approved plan artifact version",
                },
                level: {
                    type: "string",
                    enum: ["trivial", "simple", "moderate", "complex", "very_complex"],
                    description: "Overall plan complexity tier",
                },
                score: {
                    type: "integer",
                    minimum: 0,
                    maximum: 100,
                    description: "0-100 score where higher means proposal decomposition is more appropriate",
                },
                recommended_action: {
                    type: "string",
                    enum: ["implement_directly", "create_proposals"],
                    description: "Recommended primary CTA for the approved plan",
                },
                confidence: {
                    type: "number",
                    minimum: 0,
                    maximum: 1,
                    description: "Confidence in the recommendation",
                },
                reason_summary: {
                    type: "string",
                    description: "Concise user-facing reason for the recommendation",
                },
                signals: {
                    type: "object",
                    description: "Compact signals used to grade the plan, such as dependent_steps, affected_areas, migration_risk, or ambiguity",
                    additionalProperties: true,
                },
            },
            required: [
                "session_id",
                "artifact_id",
                "artifact_version",
                "level",
                "score",
                "recommended_action",
                "confidence",
                "reason_summary",
            ],
        },
    },
    {
        name: "report_verification_round",
        description: "Verifier-friendly helper for reporting an in-progress verification round on the PARENT ideation session. " +
            "The parent session remains canonical and is derived automatically from the active verification child context. Do not pass session_id. " +
            "Use this after each round once the merged gap list is ready. The response is authoritative for next-step control flow: it returns the backend verification state after convergence checks, so the verifier should use returned status/in_progress/convergence_reason instead of re-implementing zero-blocking, jaccard, or max-round rules in the prompt. " +
            "If the response says needs_revision, treat that as actionable plan feedback unless the convergence_reason is a terminal non-actionable runtime/user stop reason. If generation is stale, call get_plan_verification again instead of guessing.",
        inputSchema: {
            type: "object",
            examples: [
                {
                    round: 1,
                    generation: 3,
                },
            ],
            properties: {
                round: {
                    type: "integer",
                    description: "Current round number (1-based).",
                },
                generation: {
                    type: "integer",
                    description: "Generation counter for zombie protection. Pass on every verifier call. The backend reads the current-round merged gaps from the last run_verification_round result instead of trusting prompt-assembled round state.",
                },
            },
            required: ["round", "generation"],
        },
    },
    {
        name: "run_verification_enrichment",
        description: "Backend-owned one-time verification enrichment helper for the PARENT ideation session. " +
            "The parent session is derived automatically from the active verification child context; do not pass session_id. " +
            "The verifier chooses which enrichment specialists to run. The backend dispatches those specialists once, waits a bounded amount for typed finding publication or terminal delegate state, and returns the latest findings plus delegate snapshots.",
        inputSchema: {
            type: "object",
            examples: [
                {
                    selected_specialists: ["intent", "code-quality"],
                },
            ],
            properties: {
                selected_specialists: {
                    type: "array",
                    items: { type: "string" },
                    description: "Explicit enrichment specialists to run. Allowed values: ['intent', 'code-quality'].",
                },
            },
            required: [],
        },
    },
    {
        name: "run_verification_round",
        description: "Backend-owned verification round driver for the PARENT ideation session. " +
            "The parent session is derived automatically from the active verification child context; do not pass session_id. " +
            "The verifier chooses which optional specialists to run. The backend dispatches those specialists, runs the required completeness + feasibility critics, waits for bounded settlement, and returns structured required critic findings plus backend-owned merged gaps.",
        inputSchema: {
            type: "object",
            examples: [
                {
                    round: 2,
                    selected_specialists: ["ux", "pipeline-safety"],
                },
            ],
            properties: {
                round: {
                    type: "integer",
                    description: "Current verification round number (1-based).",
                },
                selected_specialists: {
                    type: "array",
                    items: { type: "string" },
                    description: "Explicit optional specialists to run. Allowed values: ['ux', 'prompt-quality', 'pipeline-safety', 'state-machine'].",
                },
            },
            required: ["round"],
        },
    },
    {
        name: "complete_plan_verification",
        description: "Verifier-friendly helper for terminal verification updates on the PARENT ideation session. " +
            "The parent session remains canonical and is derived automatically from the active verification child context. Do not pass session_id. " +
            "For verifier-owned runs, the helper uses the backend-owned current round state created by run_verification_round instead of trusting prompt-supplied settlement fields. " +
            "If the required delegate set settles as infrastructure/runtime failure, the backend resets the parent to unverified instead of recording a bogus content verdict. " +
            "Call this only for true terminal outcomes: verified, exhausted revision, explicit escalation, or runtime/user stop. Do not call it immediately after an actionable needs_revision round report. " +
            "Use verified or needs_revision for normal terminal outcomes; skipped remains available only where skip is actually allowed by the backend. Do NOT pass reviewing here. " +
            "If generation is stale, call get_plan_verification again instead of guessing.",
        inputSchema: {
            type: "object",
            examples: [
                {
                    status: "verified",
                    round: 1,
                    convergence_reason: "zero_blocking",
                    generation: 3,
                },
            ],
            properties: {
                status: {
                    type: "string",
                    enum: ["needs_revision", "verified", "skipped"],
                    description: "Terminal verification status. Use verified or needs_revision for normal completion, and skipped only for flows where skip is actually allowed.",
                },
                round: {
                    type: "integer",
                    description: "Current round number (1-based). Include when it helps downstream summaries.",
                },
                convergence_reason: {
                    type: "string",
                    enum: [
                        "zero_blocking",
                        "jaccard_converged",
                        "max_rounds",
                        "critic_parse_failure",
                        "agent_error",
                        "user_stopped",
                        "user_skipped",
                        "user_reverted",
                        "escalated_to_parent",
                    ],
                    description: "Why verification converged or exited. Recommended on every terminal call and required for the standard verifier cleanup flow.",
                },
                generation: {
                    type: "integer",
                    description: "Generation counter for zombie protection. Pass on every verifier call. If mismatched, the server rejects the request.",
                },
            },
            required: ["status", "generation"],
        },
    },
    {
        name: "get_plan_verification",
        description: "Get the current verification status for an ideation plan. Parent ideation agents must pass session_id. Verifier child agents omit session_id because the canonical parent session is derived automatically from the active verification child context. Use this before and during verification to confirm the generation, in_progress flag, and current round before calling report_verification_round or complete_plan_verification. " +
            "If a verification update call is rejected, call this again on the parent session and copy the returned generation/in_progress values instead of guessing.",
        inputSchema: {
            type: "object",
            examples: [
                {
                    session_id: "ideation-session-id",
                },
                {},
            ],
            properties: {
                session_id: {
                    type: "string",
                    description: "Parent ideation session ID. Required for parent ideation agents; verifier child agents omit it.",
                },
            },
            required: [],
        },
    },
    {
        name: "revert_and_skip",
        description: "Atomically revert the plan to a previous version and skip verification. " +
            "Use when the user wants to discard recent plan changes and proceed without re-running adversarial review. " +
            "Restores the exact content of the specified plan version, creates a new artifact entry for auditability, " +
            "and sets verification status to skipped in a single atomic operation.",
        inputSchema: {
            type: "object",
            properties: {
                session_id: {
                    type: "string",
                    description: "The ideation session ID",
                },
                plan_version_to_restore: {
                    type: "string",
                    description: "The artifact ID of the plan version to restore content from. " +
                        "Typically a previous version's artifact ID (e.g., from before edits were made).",
                },
            },
            required: ["session_id", "plan_version_to_restore"],
        },
    },
    {
        name: "stop_verification",
        description: "Stop a running verification loop and skip to completion. Kills the verification child agent immediately, sets status to skipped with convergence_reason user_stopped, and unfreezes the plan artifact. Idempotent: returns success if no verification is in progress.",
        inputSchema: {
            type: "object",
            properties: {
                session_id: {
                    type: "string",
                    description: "The ideation session ID",
                },
            },
            required: ["session_id"],
        },
    },
    {
        name: "edit_plan_artifact",
        description: "Apply anchor-based edit operations to an existing implementation plan. More token-efficient than update_plan_artifact for targeted changes — only send the text to find and replace, not the entire plan content. Each edit finds the first occurrence of old_text and replaces it with new_text. Stale artifact IDs are auto-resolved to the latest version. Edits are applied sequentially; if any edit fails (old_text not found or ambiguous), the entire operation is rejected with details of which edit failed. Caller-session routing for verification freeze bypass is derived automatically from live app context; do not pass it manually.",
        inputSchema: {
            type: "object",
            properties: {
                artifact_id: {
                    type: "string",
                    description: "The artifact ID of the plan to edit. Can be any version ID — stale IDs are auto-resolved to the latest version.",
                },
                edits: {
                    type: "array",
                    minItems: 1,
                    maxItems: 20,
                    items: {
                        type: "object",
                        properties: {
                            old_text: {
                                type: "string",
                                minLength: 1,
                                description: "The exact text to find in the plan. Must be unique within the plan content to avoid ambiguous replacements.",
                            },
                            new_text: {
                                type: "string",
                                description: "The replacement text. Can be empty string to delete the matched text.",
                            },
                        },
                        required: ["old_text", "new_text"],
                    },
                    description: "List of edit operations to apply sequentially. Each operation finds old_text and replaces with new_text.",
                },
            },
            required: ["artifact_id", "edits"],
        },
    },
];
//# sourceMappingURL=plan-tools.js.map