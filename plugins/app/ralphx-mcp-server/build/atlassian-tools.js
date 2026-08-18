/**
 * Atlassian (Jira + Confluence) MCP tool definitions.
 *
 * These proxy to the RalphX backend, which reuses the already-configured
 * Atlassian integration credentials. Access is tiered per agent routing role,
 * so which of these tools an agent can see is decided at spawn time and
 * re-checked by the backend on every call.
 *
 * Schemas never accept run, conversation, or orchestration ids: caller identity
 * is transport-owned and injected as headers. No schema names a credential,
 * token, site URL, or cloud id.
 */
import { buildRuntimeIdentityTransportHeaders } from "./runtime-context.js";
const MAX_SEARCH_RESULTS = 50;
export const ATLASSIAN_TOOLS = [
    // ========================================================================
    // READ TIER
    // ========================================================================
    {
        name: "jira_search_issues",
        description: "Search Jira issues using the workspace's configured Atlassian integration. " +
            "By default, query is treated as free text (an exact issue key like 'ENG-123' matches " +
            "directly, otherwise it becomes a phrase search). Set jql: true to pass a raw JQL query " +
            "through unmodified, for example to filter by project, status, assignee, or label.",
        inputSchema: {
            type: "object",
            properties: {
                query: {
                    type: "string",
                    description: "Free text or issue key by default. With jql: true, a raw JQL query, for example " +
                        "\"project = ENG AND status = 'In Progress'\".",
                },
                jql: {
                    type: "boolean",
                    description: "When true, query is submitted to Jira as raw JQL, unmodified. Default false (free text).",
                },
                maxResults: {
                    type: "number",
                    description: `Maximum issues to return (1-${MAX_SEARCH_RESULTS}, default 25).`,
                },
            },
            required: ["query"],
        },
    },
    {
        name: "jira_get_issue",
        description: "Read a single Jira issue including summary, description, status, and assignee.",
        inputSchema: {
            type: "object",
            properties: {
                issueKey: {
                    type: "string",
                    description: "Jira issue key, for example 'ENG-123'.",
                },
            },
            required: ["issueKey"],
        },
    },
    {
        name: "jira_list_projects",
        description: "List Jira projects available to the configured Atlassian integration.",
        inputSchema: {
            type: "object",
            properties: {
                limit: {
                    type: "number",
                    description: "Maximum projects to return (1-200, default 50).",
                },
            },
        },
    },
    {
        name: "jira_list_transitions",
        description: "List the workflow transitions currently available for a Jira issue. " +
            "Call this before jira_transition_issue to get a valid transition id.",
        inputSchema: {
            type: "object",
            properties: {
                issueKey: {
                    type: "string",
                    description: "Jira issue key, for example 'ENG-123'.",
                },
            },
            required: ["issueKey"],
        },
    },
    {
        name: "jira_list_boards",
        description: "List Jira Software boards available to the configured Atlassian integration. " +
            "Omit projectKey to list every visible board.",
        inputSchema: {
            type: "object",
            properties: {
                projectKey: {
                    type: "string",
                    description: "Optional Jira project key filter, for example 'ENG'.",
                },
            },
        },
    },
    {
        name: "jira_list_sprints",
        description: "List sprints for a Jira Software board. Currently returns only active sprints.",
        inputSchema: {
            type: "object",
            properties: {
                boardId: {
                    type: "string",
                    description: "Jira Software board id, from jira_list_boards.",
                },
                state: {
                    type: "string",
                    description: "Sprint state filter. Only 'active' (the default) is supported.",
                },
            },
            required: ["boardId"],
        },
    },
    {
        name: "jira_get_sprint_issues",
        description: "List issues in a Jira Software sprint, including status, issue type, assignee, " +
            `and last-updated timestamp (up to ${MAX_SEARCH_RESULTS} issues).`,
        inputSchema: {
            type: "object",
            properties: {
                sprintId: {
                    type: "string",
                    description: "Jira Software sprint id, from jira_list_sprints.",
                },
            },
            required: ["sprintId"],
        },
    },
    {
        name: "jira_list_comments",
        description: "List comments on a Jira issue with the provider's true total comment count. " +
            "Use this to page through comments beyond the handful shown inline by jira_get_issue.",
        inputSchema: {
            type: "object",
            properties: {
                issueKey: {
                    type: "string",
                    description: "Jira issue key, for example 'ENG-123'.",
                },
                startAt: {
                    type: "number",
                    description: "Zero-based offset into the comment list (default 0).",
                },
                maxResults: {
                    type: "number",
                    description: "Maximum comments to return per page (1-100, default 20).",
                },
            },
            required: ["issueKey"],
        },
    },
    {
        name: "jira_search_users",
        description: "Search for Jira users by name or address, returning accountId and displayName. " +
            "Use the returned accountId with jira_assign_issue or jira_create_issue's assigneeAccountId.",
        inputSchema: {
            type: "object",
            properties: {
                query: {
                    type: "string",
                    description: "Name or address fragment to search for.",
                },
                maxResults: {
                    type: "number",
                    description: "Maximum users to return (1-20, default 20).",
                },
            },
            required: ["query"],
        },
    },
    {
        name: "confluence_list_spaces",
        description: "List Confluence spaces visible to the configured Atlassian integration, including " +
            "each space's id, key, and name. Use the returned id as confluence_create_page's spaceId.",
        inputSchema: {
            type: "object",
            properties: {
                limit: {
                    type: "number",
                    description: "Maximum spaces to return (1-250, default 50).",
                },
            },
        },
    },
    {
        name: "confluence_search_pages",
        description: "Search Confluence pages using the configured Atlassian integration. " +
            "By default, query is treated as free text matched against page titles and content " +
            "(a numeric page id matches directly). Set cql: true to pass a raw CQL query through " +
            "unmodified.",
        inputSchema: {
            type: "object",
            properties: {
                query: {
                    type: "string",
                    description: "Free text or page id by default. With cql: true, a raw CQL query, for example " +
                        "'type = page AND text ~ \"runbook\"'.",
                },
                cql: {
                    type: "boolean",
                    description: "When true, query is submitted to Confluence as raw CQL, unmodified. Default false (free text).",
                },
                maxResults: {
                    type: "number",
                    description: `Maximum pages to return (1-${MAX_SEARCH_RESULTS}, default 25).`,
                },
            },
            required: ["query"],
        },
    },
    {
        name: "confluence_get_page",
        description: "Read a Confluence page including its full storage-format body. " +
            "Use confluence_search_pages first when you only have a title.",
        inputSchema: {
            type: "object",
            properties: {
                pageId: {
                    type: "string",
                    description: "Confluence page id.",
                },
            },
            required: ["pageId"],
        },
    },
    {
        name: "atlassian_api_request",
        description: "Escape hatch for Atlassian REST endpoints with no dedicated tool. " +
            "Paths must be relative and start with /rest/api/, /rest/agile/, /wiki/rest/api/, or /wiki/api/v2/. " +
            "GET and HEAD are available at read access; mutating methods require write access.",
        inputSchema: {
            type: "object",
            properties: {
                method: {
                    type: "string",
                    enum: ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"],
                    description: "HTTP method. Mutating methods require write access.",
                },
                product: {
                    type: "string",
                    enum: ["jira", "confluence"],
                    description: "Which Atlassian product the path belongs to.",
                },
                path: {
                    type: "string",
                    description: "Relative API path, for example '/rest/agile/1.0/board/5/sprint'. Absolute URLs are rejected.",
                },
                body: {
                    type: "object",
                    description: "Optional JSON request body. Ignored for GET and HEAD.",
                },
            },
            required: ["method", "product", "path"],
        },
    },
    // ========================================================================
    // WRITE TIER
    // ========================================================================
    {
        name: "jira_create_issue",
        description: "Create a Jira issue in the configured Atlassian integration.",
        inputSchema: {
            type: "object",
            properties: {
                projectKey: {
                    type: "string",
                    description: "Jira project key, for example 'ENG'.",
                },
                issueType: {
                    type: "string",
                    description: "Issue type name, for example 'Task' or 'Bug'.",
                },
                summary: { type: "string", description: "Issue summary line." },
                description: {
                    type: "string",
                    description: "Issue description. Markdown is converted to Jira rich text " +
                        "(headings, lists, code blocks, links, bold/italic/inline code).",
                },
                labels: {
                    type: "array",
                    items: { type: "string" },
                    description: "Optional labels to apply.",
                },
                priority: { type: "string", description: "Optional priority name." },
                parentKey: {
                    type: "string",
                    description: "Optional parent/epic issue key to link this issue under, for example 'ENG-100'.",
                },
                assigneeAccountId: {
                    type: "string",
                    description: "Optional assignee accountId, from jira_search_users.",
                },
                components: {
                    type: "array",
                    items: { type: "string" },
                    description: "Optional component names to apply.",
                },
            },
            required: ["projectKey", "issueType", "summary"],
        },
    },
    {
        name: "jira_update_issue",
        description: "Update summary, description, labels, or priority on a Jira issue. " +
            "Omitted fields are left unchanged.",
        inputSchema: {
            type: "object",
            properties: {
                issueKey: { type: "string", description: "Jira issue key." },
                summary: { type: "string", description: "Replacement summary line." },
                description: {
                    type: "string",
                    description: "Replacement description. Markdown is converted to Jira rich text " +
                        "(headings, lists, code blocks, links, bold/italic/inline code).",
                },
                labels: {
                    type: "array",
                    items: { type: "string" },
                    description: "Replacement label set.",
                },
                priority: { type: "string", description: "Replacement priority name." },
            },
            required: ["issueKey"],
        },
    },
    {
        name: "jira_add_comment",
        description: "Add a comment to a Jira issue.",
        inputSchema: {
            type: "object",
            properties: {
                issueKey: { type: "string", description: "Jira issue key." },
                body: {
                    type: "string",
                    description: "Comment body. Markdown is converted to Jira rich text " +
                        "(headings, lists, code blocks, links, bold/italic/inline code).",
                },
            },
            required: ["issueKey", "body"],
        },
    },
    {
        name: "jira_transition_issue",
        description: "Move a Jira issue through a workflow transition. " +
            "Call jira_list_transitions first to get a valid transition id.",
        inputSchema: {
            type: "object",
            properties: {
                issueKey: { type: "string", description: "Jira issue key." },
                transitionId: {
                    type: "string",
                    description: "Transition id from jira_list_transitions.",
                },
            },
            required: ["issueKey", "transitionId"],
        },
    },
    {
        name: "jira_assign_issue",
        description: "Assign a Jira issue to a specific user, the integration's account, or clear its assignee. " +
            "Precedence when multiple fields are set: accountId, then assignToMe, then clear.",
        inputSchema: {
            type: "object",
            properties: {
                issueKey: { type: "string", description: "Jira issue key." },
                accountId: {
                    type: "string",
                    description: "Assign to this specific user's accountId, from jira_search_users. Takes " +
                        "precedence over assignToMe.",
                },
                assignToMe: {
                    type: "boolean",
                    description: "True assigns the issue to the integration account; false clears the assignee. " +
                        "Ignored when accountId is set.",
                },
            },
            required: ["issueKey"],
        },
    },
    {
        name: "confluence_create_page",
        description: "Create a Confluence page from storage-format or markdown content. " +
            "Supply exactly one of bodyStorage or bodyMarkdown.",
        inputSchema: {
            type: "object",
            properties: {
                spaceId: { type: "string", description: "Confluence space id." },
                title: { type: "string", description: "Page title." },
                bodyStorage: {
                    type: "string",
                    description: "Page body in Confluence storage format (XHTML-like). Exactly one " +
                        "of bodyStorage/bodyMarkdown is required.",
                },
                bodyMarkdown: {
                    type: "string",
                    description: "Page body in markdown, converted to Confluence storage format " +
                        "(headings, lists, code blocks, links, bold/italic/inline code). " +
                        "Exactly one of bodyStorage/bodyMarkdown is required.",
                },
                parentId: {
                    type: "string",
                    description: "Optional parent page id.",
                },
            },
            required: ["spaceId", "title"],
        },
    },
    {
        name: "confluence_update_page",
        description: "Update a Confluence page's title or body. The current version is read and " +
            "incremented automatically, so no version number is required. Supply at most " +
            "one of bodyStorage or bodyMarkdown; omitting both leaves the body unchanged.",
        inputSchema: {
            type: "object",
            properties: {
                pageId: { type: "string", description: "Confluence page id." },
                title: { type: "string", description: "Replacement page title." },
                bodyStorage: {
                    type: "string",
                    description: "Replacement body in Confluence storage format.",
                },
                bodyMarkdown: {
                    type: "string",
                    description: "Replacement body in markdown, converted to Confluence storage format " +
                        "(headings, lists, code blocks, links, bold/italic/inline code).",
                },
            },
            required: ["pageId"],
        },
    },
];
const ATLASSIAN_TOOL_NAMES = new Set(ATLASSIAN_TOOLS.map((tool) => tool.name));
/** Backend endpoint for each Atlassian tool. */
const ATLASSIAN_TOOL_ENDPOINTS = {
    jira_search_issues: "atlassian-mcp/jira/search",
    jira_get_issue: "atlassian-mcp/jira/issue",
    jira_list_projects: "atlassian-mcp/jira/projects",
    jira_list_transitions: "atlassian-mcp/jira/transitions",
    jira_list_boards: "atlassian-mcp/jira/agile/boards",
    jira_list_sprints: "atlassian-mcp/jira/agile/sprints",
    jira_get_sprint_issues: "atlassian-mcp/jira/agile/sprint/issues",
    jira_list_comments: "atlassian-mcp/jira/issue/comments",
    jira_search_users: "atlassian-mcp/jira/users/search",
    jira_create_issue: "atlassian-mcp/jira/issue/create",
    jira_update_issue: "atlassian-mcp/jira/issue/update",
    jira_add_comment: "atlassian-mcp/jira/issue/comment",
    jira_transition_issue: "atlassian-mcp/jira/issue/transition",
    jira_assign_issue: "atlassian-mcp/jira/issue/assign",
    confluence_search_pages: "atlassian-mcp/confluence/search",
    confluence_list_spaces: "atlassian-mcp/confluence/spaces",
    confluence_get_page: "atlassian-mcp/confluence/page",
    confluence_create_page: "atlassian-mcp/confluence/page/create",
    confluence_update_page: "atlassian-mcp/confluence/page/update",
    atlassian_api_request: "atlassian-mcp/request",
};
export function isAtlassianToolName(name) {
    return ATLASSIAN_TOOL_NAMES.has(name);
}
/**
 * Dispatch an Atlassian tool call to its backend endpoint.
 *
 * The payload is forwarded as-is; the backend owns validation, tier
 * enforcement, and credential resolution. Caller identity travels in headers,
 * never in the payload.
 */
export async function callAtlassianTool(name, callTauri, args, runtimeContext) {
    const endpoint = ATLASSIAN_TOOL_ENDPOINTS[name];
    if (!endpoint) {
        throw new Error(`Unknown Atlassian tool: ${name}`);
    }
    const payload = args && typeof args === "object" && !Array.isArray(args)
        ? args
        : {};
    const headers = runtimeContext
        ? buildRuntimeIdentityTransportHeaders(runtimeContext)
        : undefined;
    return callTauri(endpoint, payload, headers ? { headers } : undefined);
}
//# sourceMappingURL=atlassian-tools.js.map