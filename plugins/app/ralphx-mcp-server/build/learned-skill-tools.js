export const LEARNED_SKILL_TOOLS = [
    {
        name: "list_project_skills",
        description: "List approved or staged learned project skills for the active project. " +
            "Read-only; use to inspect repository-backed learned procedural guidance before applying it.",
        inputSchema: {
            type: "object",
            properties: {
                project_id: {
                    type: "string",
                    description: "The active project ID from RALPHX_PROJECT_ID.",
                },
                status: {
                    type: "string",
                    enum: ["staged", "approved", "rejected", "archived", "retired"],
                    description: "Optional lifecycle status filter. Use approved for normal runtime guidance.",
                },
                include_archived: {
                    type: "boolean",
                    description: "Include archived or retired skills. Defaults to false.",
                },
                stage: {
                    type: "string",
                    description: "Optional stage filter such as planning, verification, review, execution, or merge.",
                },
                bucket: {
                    type: "string",
                    description: "Optional bucket filter such as planning, verification, review, execution, or merge.",
                },
                scope_path: {
                    type: "string",
                    description: "Optional active path for backend scope filtering.",
                },
            },
            required: ["project_id"],
        },
    },
    {
        name: "get_project_skill",
        description: "Fetch one learned project skill by stable project_skill_id. Read-only; returns full guidance and provenance for an already visible skill.",
        inputSchema: {
            type: "object",
            properties: {
                project_skill_id: {
                    type: "string",
                    description: "Stable project skill ID returned by list_project_skills.",
                },
            },
            required: ["project_skill_id"],
        },
    },
];
export const LEARNED_SKILL_TOOL_NAMES = LEARNED_SKILL_TOOLS.map((tool) => tool.name);
//# sourceMappingURL=learned-skill-tools.js.map