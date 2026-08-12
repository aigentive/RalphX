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
        description: "Search Jira issues with JQL using the workspace's configured Atlassian integration. " +
            "Use this to find issues by project, status, assignee, label, or text.",
        inputSchema: {
            type: "object",
            properties: {
                query: {
                    type: "string",
                    description: "JQL query, for example \"project = ENG AND status = 'In Progress'\".",
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
        name: "confluence_search_pages",
        description: "Search Confluence pages with CQL using the configured Atlassian integration.",
        inputSchema: {
            type: "object",
            properties: {
                query: {
                    type: "string",
                    description: "CQL query or free text, for example 'type = page AND text ~ \"runbook\"'.",
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
                    description: "Plain-text description. Rich formatting is not supported.",
                },
                labels: {
                    type: "array",
                    items: { type: "string" },
                    description: "Optional labels to apply.",
                },
                priority: { type: "string", description: "Optional priority name." },
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
                    description: "Replacement plain-text description.",
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
                body: { type: "string", description: "Plain-text comment body." },
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
        description: "Assign a Jira issue to the integration's account, or clear its assignee.",
        inputSchema: {
            type: "object",
            properties: {
                issueKey: { type: "string", description: "Jira issue key." },
                assignToMe: {
                    type: "boolean",
                    description: "True assigns the issue to the integration account; false clears the assignee.",
                },
            },
            required: ["issueKey"],
        },
    },
    {
        name: "confluence_create_page",
        description: "Create a Confluence page from storage-format content.",
        inputSchema: {
            type: "object",
            properties: {
                spaceId: { type: "string", description: "Confluence space id." },
                title: { type: "string", description: "Page title." },
                bodyStorage: {
                    type: "string",
                    description: "Page body in Confluence storage format (XHTML-like).",
                },
                parentId: {
                    type: "string",
                    description: "Optional parent page id.",
                },
            },
            required: ["spaceId", "title", "bodyStorage"],
        },
    },
    {
        name: "confluence_update_page",
        description: "Update a Confluence page's title or body. The current version is read and " +
            "incremented automatically, so no version number is required.",
        inputSchema: {
            type: "object",
            properties: {
                pageId: { type: "string", description: "Confluence page id." },
                title: { type: "string", description: "Replacement page title." },
                bodyStorage: {
                    type: "string",
                    description: "Replacement body in Confluence storage format.",
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
    jira_create_issue: "atlassian-mcp/jira/issue/create",
    jira_update_issue: "atlassian-mcp/jira/issue/update",
    jira_add_comment: "atlassian-mcp/jira/issue/comment",
    jira_transition_issue: "atlassian-mcp/jira/issue/transition",
    jira_assign_issue: "atlassian-mcp/jira/issue/assign",
    confluence_search_pages: "atlassian-mcp/confluence/search",
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