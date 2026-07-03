export const VALIDATION_TOOLS = [
    {
        name: "run_task_validation",
        description: "Run or reuse backend-owned validation commands for the assigned task. " +
            "Use this instead of untracked validation shell commands so results are cached and available to later review/re-execution agents. " +
            "Execution/re-execution agents choose targeted commands and explain why each command covers the task changes.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The assigned task ID.",
                },
                purpose: {
                    type: "string",
                    enum: ["baseline", "wave_gate", "final", "re_execution", "other"],
                    description: "Why validation is being requested.",
                },
                mode: {
                    type: "string",
                    enum: ["reuse_or_run", "force", "dry_run"],
                    description: "Whether to reuse exact fresh cached results, force rerun, or only record skipped dry-run intent.",
                },
                analysis_fingerprint: {
                    type: "string",
                    description: "Optional fingerprint/version of the project-analysis data used to select commands.",
                },
                commands: {
                    type: "array",
                    items: {
                        type: "object",
                        properties: {
                            command: {
                                type: "string",
                                description: "Shell command to run from cwd through RalphX's production shell/env resolver.",
                            },
                            cwd: {
                                type: "string",
                                description: "Optional cwd, relative to the task worktree or absolute inside it. Defaults to task worktree.",
                            },
                            label: {
                                type: "string",
                                description: "Short human-readable command label.",
                            },
                            category: {
                                type: "string",
                                enum: ["test", "lint", "typecheck", "build", "format", "other"],
                                description: "Command category for cache/review evidence.",
                            },
                            reason: {
                                type: "string",
                                description: "Why this command is relevant to the task changes.",
                            },
                            related_files: {
                                type: "array",
                                items: { type: "string" },
                                description: "Changed files this command is intended to cover.",
                            },
                            command_ref: {
                                type: "string",
                                description: "Optional stable project-analysis command reference.",
                            },
                            source: {
                                type: "string",
                                enum: ["agent_selected", "project_analysis_ref"],
                                description: "Whether the command was selected by the agent or derived from a project-analysis ref.",
                            },
                        },
                        required: ["command", "category", "reason"],
                    },
                },
            },
            required: ["task_id", "commands"],
        },
    },
    {
        name: "get_task_validation_summary",
        description: "Read persisted validation evidence for a task. Reviewers use this read-only tool instead of running validation.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID.",
                },
            },
            required: ["task_id"],
        },
    },
    {
        name: "get_task_diff_stat",
        description: "Read a bounded stat summary of task worktree changes against the task review base. Read-only reviewer replacement for git diff --stat.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID.",
                },
                base_ref: {
                    type: "string",
                    description: "Optional base ref override. Defaults to the task plan/base branch.",
                },
            },
            required: ["task_id"],
        },
    },
    {
        name: "get_task_diff",
        description: "Read bounded task worktree file diffs against the task review base. Read-only reviewer replacement for git diff.",
        inputSchema: {
            type: "object",
            properties: {
                task_id: {
                    type: "string",
                    description: "The task ID.",
                },
                base_ref: {
                    type: "string",
                    description: "Optional base ref override. Defaults to the task plan/base branch.",
                },
                file_paths: {
                    type: "array",
                    items: { type: "string" },
                    description: "Optional changed file paths to include. Defaults to the first changed files.",
                },
                max_files: {
                    type: "number",
                    description: "Maximum number of file diffs to return. Defaults to 20 and is capped by backend.",
                },
            },
            required: ["task_id"],
        },
    },
];
//# sourceMappingURL=validation-tools.js.map