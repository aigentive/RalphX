/**
 * Workflow and coordination MCP tool definitions
 */
export const WORKFLOW_TOOLS = [
    // ========================================================================
    // TEAM TOOLS (team lead agents)
    // ========================================================================
    {
        name: "request_team_plan",
        description: "Request approval for a team plan before spawning teammates. " +
            "The plan includes the process type, teammate roles/models/tools, and execution strategy. " +
            "User approval is required before teammates can be spawned.",
        inputSchema: {
            type: "object",
            properties: {
                process: {
                    type: "string",
                    description: "Process type: 'ideation-research', 'ideation-debate', 'worker-parallel'",
                },
                teammates: {
                    type: "array",
                    items: {
                        type: "object",
                        properties: {
                            role: {
                                type: "string",
                                description: "Teammate role name (e.g., 'frontend-researcher', 'coder-1')",
                            },
                            tools: {
                                type: "array",
                                items: { type: "string" },
                                description: "CLI tools for this teammate (e.g., ['Read', 'Grep', 'Glob'])",
                            },
                            mcp_tools: {
                                type: "array",
                                items: { type: "string" },
                                description: "MCP tools for this teammate (e.g., ['get_session_plan'])",
                            },
                            model: {
                                type: "string",
                                description: "Model to use: 'haiku', 'sonnet', or 'opus'",
                            },
                            preset: {
                                type: "string",
                                description: "Optional predefined agent template (for constrained mode)",
                            },
                            prompt_summary: {
                                type: "string",
                                description: "Brief summary of what this teammate will do",
                            },
                        },
                        required: ["role", "tools", "mcp_tools", "model", "prompt_summary"],
                    },
                    description: "Array of teammate configurations to spawn",
                },
                team_name: {
                    type: "string",
                    description: "Team name from the lead agent's TeamCreate call. Ensures teammates join the same team registry.",
                },
            },
            required: ["process", "teammates", "team_name"],
        },
    },
    {
        name: "request_teammate_spawn",
        description: "Request to spawn a single teammate. " +
            "The backend validates the request against team constraints, then spawns the teammate if approved.",
        inputSchema: {
            type: "object",
            properties: {
                role: {
                    type: "string",
                    description: "Teammate role name (e.g., 'frontend-researcher', 'coder-1')",
                },
                prompt: {
                    type: "string",
                    description: "Full prompt for the teammate describing their role and expected output",
                },
                model: {
                    type: "string",
                    enum: ["haiku", "sonnet", "opus"],
                    description: "Model to use (must be within model_ceiling constraint)",
                },
                tools: {
                    type: "array",
                    items: { type: "string" },
                    description: "Requested CLI tools (intersected with tool_ceiling)",
                },
                mcp_tools: {
                    type: "array",
                    items: { type: "string" },
                    description: "Requested MCP tools (intersected with mcp_tool_ceiling)",
                },
                preset: {
                    type: "string",
                    description: "Optional predefined agent template to use (for constrained mode)",
                },
            },
            required: ["role", "prompt", "model", "tools", "mcp_tools"],
        },
    },
    {
        name: "create_team_artifact",
        description: "Create a team artifact documenting research findings, analysis, or summary. " +
            "Automatically sets bucket_id='team-findings' and populates metadata with team info. " +
            "Use for documenting specialist findings, debate analyses, or lead-synthesized summaries. " +
            "Verification specialists should target the PARENT ideation session_id. If a verification child session_id is passed, the backend remaps it to the parent ideation session automatically. " +
            "If a caller is retrying after an incomplete run, reuse the same parent session_id and publish a partial artifact rather than omitting the artifact entirely. " +
            "This is the general team-artifact path, not the typed verification-finding path.",
        inputSchema: {
            type: "object",
            examples: [
                {
                    session_id: "parent-session-id",
                    title: "Completeness: Round 1 cold boot coverage",
                    content: "{\"status\":\"partial\",\"critic\":\"completeness\",\"round\":1,\"coverage\":\"affected_files\",\"summary\":\"Need one more pass on recovery edge cases\",\"gaps\":[]}",
                    artifact_type: "TeamResearch",
                },
            ],
            properties: {
                session_id: {
                    type: "string",
                    description: "The ideation or execution session ID. For verification critics/specialists the PARENT ideation session ID is canonical; verification child session IDs are auto-remapped to that parent.",
                },
                title: {
                    type: "string",
                    description: "Clear, concise title for the artifact.",
                },
                content: {
                    type: "string",
                    description: "Markdown or JSON-string content with research findings or analysis.",
                },
                artifact_type: {
                    type: "string",
                    enum: ["TeamResearch", "TeamAnalysis", "TeamSummary"],
                    description: "Type: TeamResearch (specialist findings), TeamAnalysis (comparison/debate), TeamSummary (lead synthesis)",
                },
                related_artifact_id: {
                    type: "string",
                    description: "Optional artifact ID to link to (e.g., master plan artifact)",
                },
            },
            required: ["session_id", "title", "content", "artifact_type"],
        },
    },
    {
        name: "publish_verification_finding",
        description: "Publish a typed verification finding for the current verification round. " +
            "Use this for verification-path specialists and required plan critics so the backend can aggregate gaps directly from structured metadata. " +
            "If session_id is omitted, the backend uses the current session context and remaps verification child sessions plus delegated verification specialists/critics to the parent ideation session automatically.",
        inputSchema: {
            type: "object",
            examples: [
                {
                    critic: "completeness",
                    round: 1,
                    status: "partial",
                    coverage: "affected_files",
                    summary: "Migration needs an explicit backfill step.",
                    gaps: [
                        {
                            severity: "high",
                            category: "completeness",
                            description: "Existing projects are not covered by a concrete backfill step.",
                            why_it_matters: "Persisted rows can retain the old mode.",
                        },
                    ],
                },
            ],
            properties: {
                session_id: {
                    type: "string",
                    description: "Optional parent ideation session ID. Omit this in normal verifier-critic flows and let the backend resolve the correct parent session automatically, including delegated verification specialists and critics.",
                },
                critic: {
                    type: "string",
                    description: "Critic identifier, for example 'completeness', 'feasibility', 'intent', 'code-quality', 'ux', 'prompt-quality', 'pipeline-safety', or 'state-machine'.",
                },
                round: {
                    type: "integer",
                    description: "Verification round number (1-based).",
                },
                status: {
                    type: "string",
                    enum: ["complete", "partial", "error"],
                    description: "Whether the critic finished fully, partially, or hit an infrastructure error.",
                },
                coverage: {
                    type: "string",
                    description: "Optional coverage scope such as 'plan_only', 'affected_files', or 'affected_files_plus_adjacent'.",
                },
                summary: {
                    type: "string",
                    description: "One-sentence synthesis of the critic result.",
                },
                gaps: {
                    type: "array",
                    description: "Structured verification gaps identified by the critic.",
                    items: {
                        type: "object",
                        properties: {
                            severity: {
                                type: "string",
                                enum: ["critical", "high", "medium", "low"],
                            },
                            category: { type: "string" },
                            description: { type: "string" },
                            why_it_matters: { type: "string" },
                            source: { type: "string" },
                            lens: { type: "string" },
                        },
                        required: ["severity", "category", "description"],
                    },
                },
                title_suffix: {
                    type: "string",
                    description: "Optional short human-readable suffix appended to the generated artifact title.",
                },
            },
            required: ["critic", "round", "status", "summary", "gaps"],
        },
    },
    {
        name: "get_team_artifacts",
        description: "Retrieve all team artifacts for a session. " +
            "Returns artifacts from the 'team-findings' bucket filtered by session ID. " +
            "Use the PARENT ideation session_id for verification flows; if a verification child session_id is passed, the backend remaps it to the parent ideation session automatically. " +
            "This is the raw artifact listing surface for cases where you genuinely need the full unfiltered team-artifact list.",
        inputSchema: {
            type: "object",
            examples: [{ session_id: "parent-session-id" }],
            properties: {
                session_id: {
                    type: "string",
                    description: "The ideation or execution session ID",
                },
            },
            required: ["session_id"],
        },
    },
    {
        name: "get_team_session_state",
        description: "Retrieve persisted team composition and phase progress for session recovery. " +
            "Returns team composition (teammate names/roles/prompts), current phase, and artifact IDs.",
        inputSchema: {
            type: "object",
            properties: {
                session_id: {
                    type: "string",
                    description: "The ideation or execution session ID",
                },
            },
            required: ["session_id"],
        },
    },
    {
        name: "save_team_session_state",
        description: "Persist current team composition to database for session recovery. " +
            "Called after spawning teammates to enable resume if session is interrupted.",
        inputSchema: {
            type: "object",
            properties: {
                session_id: {
                    type: "string",
                    description: "The ideation or execution session ID",
                },
                team_composition: {
                    type: "array",
                    items: {
                        type: "object",
                        properties: {
                            name: {
                                type: "string",
                                description: "Teammate name",
                            },
                            role: {
                                type: "string",
                                description: "Teammate role description",
                            },
                            prompt: {
                                type: "string",
                                description: "Full prompt used to spawn this teammate",
                            },
                            model: {
                                type: "string",
                                description: "Model used for this teammate",
                            },
                        },
                        required: ["name", "role", "prompt", "model"],
                    },
                    description: "Array of teammate configurations",
                },
                phase: {
                    type: "string",
                    description: "Current workflow phase (e.g., 'EXPLORE', 'PLAN', 'CONFIRM')",
                },
                artifact_ids: {
                    type: "array",
                    items: { type: "string" },
                    description: "IDs of team artifacts created so far",
                },
            },
            required: ["session_id", "team_composition", "phase"],
        },
    },
    // ========================================================================
    // TASK TOOLS (ralphx-chat-task agent)
    // ========================================================================
    {
        name: "update_task",
        description: "Update an existing task's details. Use when the user wants to modify task title, description, or priority. For status changes, use move_task or workflow commands.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID to update",
                },
                title: {
                    type: "string",
                    description: "Updated task title",
                },
                description: {
                    type: "string",
                    description: "Updated description",
                },
                priority: {
                    type: "string",
                    enum: ["critical", "high", "medium", "low"],
                    description: "Updated priority",
                },
            },
            required: ["task_id"],
        },
    },
    {
        name: "add_task_note",
        description: "Add a note or comment to a task. Use when the user wants to document progress, issues, or decisions.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID",
                },
                note: {
                    type: "string",
                    description: "The note content",
                },
            },
            required: ["task_id", "note"],
        },
    },
    {
        name: "get_task_details",
        description: "Get full details for a task including current status, notes, and history. Use when you need complete task information.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID",
                },
            },
            required: ["task_id"],
        },
    },
    // ========================================================================
    // PROJECT TOOLS (ralphx-chat-project agent)
    // ========================================================================
    {
        name: "suggest_task",
        description: "Suggest a new task based on project analysis. Use when you've identified something that should be done based on codebase exploration.",
        inputSchema: {
            type: "object",
            properties: {
                project_id: {
                    type: "string",
                    description: "The project ID (provided in context)",
                },
                title: {
                    type: "string",
                    description: "Suggested task title",
                },
                description: {
                    type: "string",
                    description: "Why this task should be done",
                },
                category: {
                    type: "string",
                    enum: ["setup", "feature", "fix", "refactor", "docs", "test", "performance", "security", "devops", "research", "design", "chore"],
                    description: "Task category: setup (project init/infra), feature (new functionality), fix (bug fix), refactor (code restructure), docs (documentation), test (testing), performance (optimization), security (security hardening), devops (CI/CD/tooling), research (investigation/spike), design (UX/UI design), chore (maintenance/cleanup)",
                },
                priority: {
                    type: "string",
                    enum: ["critical", "high", "medium", "low"],
                    description: "Suggested priority level",
                },
            },
            required: ["project_id", "title", "description", "category"],
        },
    },
    {
        name: "list_tasks",
        description: "List tasks in the project with optional filtering. Use to answer questions about what tasks exist, their status, or priorities.",
        inputSchema: {
            type: "object",
            properties: {
                project_id: {
                    type: "string",
                    description: "The project ID",
                },
                status: {
                    type: "string",
                    enum: [
                        "backlog",
                        "ready",
                        "blocked",
                        "executing",
                        "qa_refining",
                        "qa_testing",
                        "qa_passed",
                        "qa_failed",
                        "pending_review",
                        "reviewing",
                        "review_passed",
                        "escalated",
                        "revision_needed",
                        "re_executing",
                        "approved",
                        "failed",
                        "cancelled",
                    ],
                    description: "Filter by status (optional)",
                },
                category: {
                    type: "string",
                    enum: ["setup", "feature", "fix", "refactor", "docs", "test", "performance", "security", "devops", "research", "design", "chore"],
                    description: "Filter by category (optional): setup, feature, fix, refactor, docs, test, performance, security, devops, research, design, chore",
                },
            },
            required: ["project_id"],
        },
    },
    {
        name: "append_task_to_ideation_plan",
        description: "Append a one-off task to an already accepted ideation plan while its plan branch is still active, including when its PR is open and waiting. " +
            "Use this instead of starting a new ideation when the user asks for a small follow-up on an accepted, still-open plan. " +
            "The backend links the task to the existing session/execution plan, creates steps, and blocks the plan merge on the new task.",
        inputSchema: {
            type: "object",
            properties: {
                project_id: {
                    type: "string",
                    description: "The project ID (provided in context). Must match this agent's project scope.",
                },
                session_id: {
                    type: "string",
                    description: "Accepted ideation session ID to extend.",
                },
                title: {
                    type: "string",
                    description: "Short task title.",
                },
                description: {
                    type: "string",
                    description: "Task description and implementation intent.",
                },
                steps: {
                    type: "array",
                    items: { type: "string" },
                    description: "Concrete execution steps for the appended task.",
                },
                acceptance_criteria: {
                    type: "array",
                    items: { type: "string" },
                    description: "Acceptance criteria the task must satisfy.",
                },
                depends_on_task_ids: {
                    type: "array",
                    items: { type: "string" },
                    description: "Optional existing tasks that must complete before this one starts.",
                },
                priority: {
                    type: "number",
                    description: "Optional numeric priority. Defaults to the backend task default.",
                },
                source_conversation_id: {
                    type: "string",
                    description: "Optional source conversation ID for traceability.",
                },
                source_message_id: {
                    type: "string",
                    description: "Optional source message ID for traceability.",
                },
            },
            required: ["project_id", "session_id", "title", "steps", "acceptance_criteria"],
        },
    },
    {
        name: "search_memories",
        description: "Search project memories by optional text query and bucket filter. " +
            "Use this to retrieve relevant learned context before planning or answering questions.",
        inputSchema: {
            type: "object",
            properties: {
                project_id: {
                    type: "string",
                    description: "The project ID",
                },
                query: {
                    type: "string",
                    description: "Optional text query matched against title/summary/details",
                },
                bucket: {
                    type: "string",
                    enum: [
                        "architecture_patterns",
                        "implementation_discoveries",
                        "operational_playbooks",
                    ],
                    description: "Optional memory bucket filter",
                },
                limit: {
                    type: "number",
                    description: "Optional max number of results",
                },
            },
            required: ["project_id"],
        },
    },
    {
        name: "get_memory",
        description: "Get a single memory entry by ID. Use after search_memories when you need full details.",
        inputSchema: {
            type: "object",
            properties: {
                memory_id: {
                    type: "string",
                    description: "The memory entry ID",
                },
            },
            required: ["memory_id"],
        },
    },
    {
        name: "get_memories_for_paths",
        description: "Get memories relevant to one or more file paths using scope path matching. " +
            "Use this before editing specific files to load related historical context.",
        inputSchema: {
            type: "object",
            properties: {
                project_id: {
                    type: "string",
                    description: "The project ID",
                },
                paths: {
                    type: "array",
                    items: { type: "string" },
                    description: "File paths to match against memory scope paths",
                },
                limit: {
                    type: "number",
                    description: "Optional max number of results",
                },
            },
            required: ["project_id", "paths"],
        },
    },
    // ========================================================================
    // MERGE TOOLS (merger agent)
    // ========================================================================
    {
        name: "report_conflict",
        description: "Signal that merge conflicts could not be resolved automatically. Call this when conflicts are too complex (ambiguous intent, architectural incompatibility, or missing context). This transitions the task from Merging to MergeConflict state, keeping the branch/worktree for manual resolution.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID with unresolved conflicts",
                },
                conflict_files: {
                    type: "array",
                    items: { type: "string" },
                    description: "List of file paths that still have conflicts",
                },
                reason: {
                    type: "string",
                    description: "Explanation of why the conflicts couldn't be resolved",
                },
            },
            required: ["task_id", "conflict_files", "reason"],
        },
    },
    {
        name: "report_incomplete",
        description: "Report that merge cannot be completed due to non-conflict errors (e.g., git operation failures, missing configuration). " +
            "Use this instead of report_conflict when there are no actual merge conflicts but the merge still failed. " +
            "This transitions the task from Merging to MergeIncomplete state.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID where merge failed",
                },
                reason: {
                    type: "string",
                    description: "Detailed explanation of why the merge failed",
                },
                diagnostic_info: {
                    type: "string",
                    description: "Git status, logs, or other diagnostic output to help debug the issue",
                },
            },
            required: ["task_id", "reason"],
        },
    },
    {
        name: "complete_merge",
        description: "Signal that merge conflicts have been resolved and the merge is complete. Call this after successfully resolving all conflicts, staging changes, and completing the rebase/merge. Provide the commit SHA of the final merge commit (use `git rev-parse HEAD`). This transitions the task from Merging to Merged state.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID whose merge is complete",
                },
                commit_sha: {
                    type: "string",
                    description: "Full 40-character SHA of the merge/rebase commit (from `git rev-parse HEAD`)",
                },
            },
            required: ["task_id", "commit_sha"],
        },
    },
    {
        name: "get_merge_target",
        description: "Get the resolved merge target branches for a task. " +
            "Returns source_branch (task's branch) and target_branch (where to merge INTO). " +
            "IMPORTANT: Always call this BEFORE merging to know the correct target. " +
            "The target may be a plan feature branch instead of main.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: { type: "string", description: "The task ID" },
            },
            required: ["task_id"],
        },
    },
    // ========================================================================
    // REVIEW TOOLS (reviewer agent)
    // ========================================================================
    {
        name: "complete_review",
        description: "Submit a code review decision. Use after reviewing changes to approve, request changes, or escalate to supervisor.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task being reviewed",
                },
                decision: {
                    type: "string",
                    enum: ["approved", "needs_changes", "escalate", "approved_no_changes"],
                    description: "Review decision: approved (ship it), needs_changes (fixable issues), escalate (major concerns), approved_no_changes (use when task intentionally produced no code changes — research, docs, planning — skips merge pipeline)",
                },
                feedback: {
                    type: "string",
                    description: "Detailed feedback: what's good, what needs improvement, specific issues found",
                },
                issues: {
                    type: "array",
                    items: {
                        type: "object",
                        properties: {
                            title: {
                                type: "string",
                                description: "Short issue title",
                            },
                            severity: {
                                type: "string",
                                enum: ["critical", "major", "minor", "suggestion"],
                            },
                            step_id: {
                                type: "string",
                                description: "Task step ID when the issue maps to a specific execution step",
                            },
                            no_step_reason: {
                                type: "string",
                                description: "Required when step_id is absent; explains why the issue is not tied to a specific task step",
                            },
                            description: {
                                type: "string",
                                description: "Optional detailed explanation of the issue",
                            },
                            category: {
                                type: "string",
                                enum: ["bug", "missing", "quality", "design"],
                            },
                            file_path: { type: "string" },
                            line_number: { type: "number" },
                            code_snippet: { type: "string" },
                        },
                        required: ["title", "severity"],
                    },
                    description: "Specific issues found during review",
                },
                escalation_reason: {
                    type: "string",
                    description: "Required when decision is 'escalate': concise explanation of why human review is needed",
                },
                scope_drift_classification: {
                    type: "string",
                    enum: ["adjacent_scope_expansion", "plan_correction", "unrelated_drift"],
                    description: "Required when get_task_context reports scope_drift_status='scope_expansion'. Use adjacent_scope_expansion for nearby necessary files, plan_correction when the plan under-scoped the real implementation, or unrelated_drift for changes that do not belong in the task branch.",
                },
                scope_drift_notes: {
                    type: "string",
                    description: "Optional explanation for the scope drift classification, especially when the reviewer is sending the task back for revise.",
                },
            },
            required: ["task_id", "decision", "feedback"],
        },
    },
    {
        name: "get_review_notes",
        description: "Get all review feedback for a task. Call this before re-executing a task to understand what needs to be fixed.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID to get review notes for",
                },
            },
            required: ["task_id"],
        },
    },
    {
        name: "approve_task",
        description: "Approve a task after AI review. ONLY available when task is in 'review_passed' or 'escalated' status (awaiting human decision). " +
            "Use this when the user confirms they want to approve the task after discussing the review with you. " +
            "This will NOT work during active review - use complete_review for that.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID to approve",
                },
                comment: {
                    type: "string",
                    description: "Optional approval comment or notes",
                },
            },
            required: ["task_id"],
        },
    },
    {
        name: "request_task_changes",
        description: "Request changes on a task after AI review. ONLY available when task is in 'review_passed' or 'escalated' status (awaiting human decision). " +
            "Use this when the user wants to request changes after discussing the review with you. " +
            "This will NOT work during active review - use complete_review for that.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID to request changes on",
                },
                feedback: {
                    type: "string",
                    description: "Detailed feedback explaining what changes are needed",
                },
            },
            required: ["task_id", "feedback"],
        },
    },
];
//# sourceMappingURL=workflow-tools.js.map