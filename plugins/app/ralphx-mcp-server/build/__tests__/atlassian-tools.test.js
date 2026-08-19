import { Ajv as AjvValidator } from 'ajv/dist/ajv.js';
import { describe, expect, it, vi } from "vitest";
import { ATLASSIAN_TOOLS, callAtlassianTool, isAtlassianToolName, } from "../atlassian-tools.js";
import { getAllTools } from "../tools.js";
const READ_TOOLS = [
    "jira_search_issues",
    "jira_get_issue",
    "jira_list_projects",
    "jira_list_transitions",
    "jira_list_boards",
    "jira_list_sprints",
    "jira_get_sprint_issues",
    "jira_list_comments",
    "jira_search_users",
    "confluence_search_pages",
    "confluence_list_spaces",
    "confluence_get_page",
    "atlassian_api_request",
];
const WRITE_TOOLS = [
    "jira_create_issue",
    "jira_update_issue",
    "jira_add_comment",
    "jira_transition_issue",
    "jira_assign_issue",
    "confluence_create_page",
    "confluence_update_page",
];
const ALL_ATLASSIAN_TOOLS = [...READ_TOOLS, ...WRITE_TOOLS];
describe("Atlassian MCP tool schemas", () => {
    it("registers every Atlassian tool in the shared registry", () => {
        const registered = new Set(getAllTools().map((tool) => tool.name));
        for (const name of ALL_ATLASSIAN_TOOLS) {
            expect(registered.has(name), `${name} registered`).toBe(true);
        }
        expect(ATLASSIAN_TOOLS).toHaveLength(ALL_ATLASSIAN_TOOLS.length);
    });
    it("uses bare snake_case names that the runtime allowlist accepts", () => {
        for (const tool of ATLASSIAN_TOOLS) {
            expect(tool.name).toMatch(/^[a-z][a-z0-9_]*$/);
        }
    });
    it("validates every schema with ajv", () => {
        const ajv = new AjvValidator({ strict: false });
        for (const tool of ATLASSIAN_TOOLS) {
            expect(() => ajv.compile(tool.inputSchema), `${tool.name} schema compiles`).not.toThrow();
        }
    });
    it("never accepts run, conversation, or orchestration identity in a payload", () => {
        const forbidden = [
            "agentrunid",
            "agent_run_id",
            "conversationid",
            "conversation_id",
            "runid",
            "run_id",
            "sessionid",
            "session_id",
            "projectid",
            "project_id",
            "taskid",
            "task_id",
        ];
        for (const tool of ATLASSIAN_TOOLS) {
            const properties = Object.keys(tool.inputSchema.properties ?? {}).map((key) => key.toLowerCase());
            for (const term of forbidden) {
                expect(properties, `${tool.name} must not accept ${term}; identity is transport-owned`).not.toContain(term);
            }
        }
    });
    it("never names a credential, token, or site in any schema", () => {
        const forbidden = [
            "token",
            "credential",
            "authorization",
            "password",
            "secret",
            "apikey",
            "api_key",
            "siteurl",
            "site_url",
            "cloudid",
            "cloud_id",
            "email",
        ];
        for (const tool of ATLASSIAN_TOOLS) {
            const schemaText = JSON.stringify(tool).toLowerCase();
            for (const term of forbidden) {
                expect(schemaText, `${tool.name} schema must not mention ${term}`).not.toContain(term);
            }
        }
    });
    it("requires the fields the backend requires", () => {
        const required = (name) => ATLASSIAN_TOOLS.find((tool) => tool.name === name).inputSchema.required;
        expect(required("jira_search_issues")).toEqual(["query"]);
        expect(required("jira_get_issue")).toEqual(["issueKey"]);
        expect(required("jira_create_issue")).toEqual([
            "projectKey",
            "issueType",
            "summary",
        ]);
        expect(required("jira_transition_issue")).toEqual(["issueKey", "transitionId"]);
        expect(required("jira_list_boards")).toBeUndefined();
        expect(required("jira_list_sprints")).toEqual(["boardId"]);
        expect(required("jira_get_sprint_issues")).toEqual(["sprintId"]);
        expect(required("jira_list_comments")).toEqual(["issueKey"]);
        expect(required("jira_search_users")).toEqual(["query"]);
        expect(required("confluence_list_spaces")).toBeUndefined();
        // bodyStorage is no longer required: exactly one of bodyStorage/bodyMarkdown
        // is required, enforced by the backend, not the schema's `required` array.
        expect(required("confluence_create_page")).toEqual(["spaceId", "title"]);
        expect(required("confluence_update_page")).toEqual(["pageId"]);
        expect(required("atlassian_api_request")).toEqual(["method", "product", "path"]);
    });
    it("offers bodyMarkdown alongside bodyStorage on the Confluence write tools", () => {
        const createPage = ATLASSIAN_TOOLS.find((tool) => tool.name === "confluence_create_page");
        const updatePage = ATLASSIAN_TOOLS.find((tool) => tool.name === "confluence_update_page");
        const createProperties = createPage.inputSchema.properties;
        const updateProperties = updatePage.inputSchema.properties;
        expect(createProperties.bodyStorage?.type).toBe("string");
        expect(createProperties.bodyMarkdown?.type).toBe("string");
        expect(updateProperties.bodyStorage?.type).toBe("string");
        expect(updateProperties.bodyMarkdown?.type).toBe("string");
    });
    it("states that markdown formatting is rendered on the write-path text fields", () => {
        const byName = (name) => ATLASSIAN_TOOLS.find((tool) => tool.name === name);
        const properties = (name) => byName(name).inputSchema.properties;
        expect(properties("jira_create_issue").description?.description?.toLowerCase()).toContain("markdown");
        expect(properties("jira_update_issue").description?.description?.toLowerCase()).toContain("markdown");
        expect(properties("jira_add_comment").body?.description?.toLowerCase()).toContain("markdown");
        expect(properties("confluence_create_page").bodyMarkdown?.description?.toLowerCase()).toContain("markdown");
        expect(properties("confluence_update_page").bodyMarkdown?.description?.toLowerCase()).toContain("markdown");
    });
    it("exposes an opt-in raw jql/cql pass-through flag on the search tools", () => {
        const jiraSearch = ATLASSIAN_TOOLS.find((tool) => tool.name === "jira_search_issues");
        const confluenceSearch = ATLASSIAN_TOOLS.find((tool) => tool.name === "confluence_search_pages");
        const jiraProperties = jiraSearch.inputSchema.properties;
        const confluenceProperties = confluenceSearch.inputSchema.properties;
        expect(jiraProperties.jql?.type).toBe("boolean");
        expect(confluenceProperties.cql?.type).toBe("boolean");
        // jql/cql are opt-in, not required: default mode stays free-text search.
        expect(jiraSearch.inputSchema.required).toEqual(["query"]);
        expect(confluenceSearch.inputSchema.required).toEqual(["query"]);
    });
    it("does not claim the default (non-raw) search mode uses JQL/CQL", () => {
        const jiraSearch = ATLASSIAN_TOOLS.find((tool) => tool.name === "jira_search_issues");
        const confluenceSearch = ATLASSIAN_TOOLS.find((tool) => tool.name === "confluence_search_pages");
        const jiraProperties = jiraSearch.inputSchema.properties;
        const confluenceProperties = confluenceSearch.inputSchema.properties;
        // The old descriptions claimed every search used JQL/CQL. That claim must
        // be gone; JQL/CQL is now opt-in via the jql/cql flag only.
        expect(jiraSearch.description).not.toContain("Search Jira issues with JQL");
        expect(confluenceSearch.description).not.toContain("Search Confluence pages with CQL");
        expect(jiraSearch.description?.toLowerCase()).toContain("free text");
        expect(confluenceSearch.description?.toLowerCase()).toContain("free text");
        expect(jiraProperties.jql?.description?.toLowerCase()).toContain("jql");
        expect(confluenceProperties.cql?.description?.toLowerCase()).toContain("cql");
    });
    it("exposes accountId on jira_assign_issue, taking precedence over assignToMe", () => {
        const tool = ATLASSIAN_TOOLS.find((candidate) => candidate.name === "jira_assign_issue");
        const properties = tool.inputSchema.properties;
        expect(properties.accountId?.type).toBe("string");
        expect(properties.accountId?.description?.toLowerCase()).toContain("precedence");
        expect(properties.assignToMe?.type).toBe("boolean");
    });
    it("exposes parentKey, assigneeAccountId, and components on jira_create_issue", () => {
        const tool = ATLASSIAN_TOOLS.find((candidate) => candidate.name === "jira_create_issue");
        const properties = tool.inputSchema.properties;
        expect(properties.parentKey?.type).toBe("string");
        expect(properties.assigneeAccountId?.type).toBe("string");
        expect(properties.components?.type).toBe("array");
        expect(properties.components?.items?.type).toBe("string");
    });
    it("points agents from confluence_list_spaces at confluence_create_page's spaceId", () => {
        const listSpaces = ATLASSIAN_TOOLS.find((candidate) => candidate.name === "confluence_list_spaces");
        expect(listSpaces.description?.toLowerCase()).toContain("spaceid");
    });
    it("constrains the escape hatch to known methods and products", () => {
        const tool = ATLASSIAN_TOOLS.find((candidate) => candidate.name === "atlassian_api_request");
        const properties = tool.inputSchema.properties;
        expect(properties.method.enum).toEqual([
            "GET",
            "HEAD",
            "POST",
            "PUT",
            "PATCH",
            "DELETE",
        ]);
        expect(properties.product.enum).toEqual(["jira", "confluence"]);
    });
});
describe("Atlassian MCP tool dispatch", () => {
    it("recognizes exactly the Atlassian tool names", () => {
        for (const name of ALL_ATLASSIAN_TOOLS) {
            expect(isAtlassianToolName(name)).toBe(true);
        }
        expect(isAtlassianToolName("get_task_context")).toBe(false);
        expect(isAtlassianToolName("jira_delete_everything")).toBe(false);
    });
    it("fails closed on an unknown tool name", async () => {
        const callTauri = vi.fn();
        await expect(callAtlassianTool("jira_not_a_tool", callTauri, {})).rejects.toThrow(/Unknown Atlassian tool/);
        expect(callTauri).not.toHaveBeenCalled();
    });
    it.each([
        ["jira_search_issues", "atlassian-mcp/jira/search"],
        ["jira_get_issue", "atlassian-mcp/jira/issue"],
        ["jira_list_projects", "atlassian-mcp/jira/projects"],
        ["jira_list_transitions", "atlassian-mcp/jira/transitions"],
        ["jira_list_boards", "atlassian-mcp/jira/agile/boards"],
        ["jira_list_sprints", "atlassian-mcp/jira/agile/sprints"],
        ["jira_get_sprint_issues", "atlassian-mcp/jira/agile/sprint/issues"],
        ["jira_list_comments", "atlassian-mcp/jira/issue/comments"],
        ["jira_search_users", "atlassian-mcp/jira/users/search"],
        ["jira_create_issue", "atlassian-mcp/jira/issue/create"],
        ["jira_update_issue", "atlassian-mcp/jira/issue/update"],
        ["jira_add_comment", "atlassian-mcp/jira/issue/comment"],
        ["jira_transition_issue", "atlassian-mcp/jira/issue/transition"],
        ["jira_assign_issue", "atlassian-mcp/jira/issue/assign"],
        ["confluence_search_pages", "atlassian-mcp/confluence/search"],
        ["confluence_list_spaces", "atlassian-mcp/confluence/spaces"],
        ["confluence_get_page", "atlassian-mcp/confluence/page"],
        ["confluence_create_page", "atlassian-mcp/confluence/page/create"],
        ["confluence_update_page", "atlassian-mcp/confluence/page/update"],
        ["atlassian_api_request", "atlassian-mcp/request"],
    ])("dispatches %s to %s", async (name, endpoint) => {
        const callTauri = vi.fn().mockResolvedValue({ ok: true });
        await callAtlassianTool(name, callTauri, { issueKey: "ENG-1" });
        expect(callTauri).toHaveBeenCalledTimes(1);
        expect(callTauri.mock.calls[0][0]).toBe(endpoint);
        expect(callTauri.mock.calls[0][1]).toEqual({ issueKey: "ENG-1" });
    });
    it("forwards the payload unchanged and adds transport identity headers", async () => {
        const callTauri = vi.fn().mockResolvedValue({ ok: true });
        await callAtlassianTool("jira_create_issue", callTauri, {
            projectKey: "ENG",
            issueType: "Task",
            summary: "Wire the gate",
            labels: ["ralphx"],
        }, { conversationId: "conv-1", agentRunId: "run-1" });
        const [endpoint, payload, options] = callTauri.mock.calls[0];
        expect(endpoint).toBe("atlassian-mcp/jira/issue/create");
        expect(payload).toEqual({
            projectKey: "ENG",
            issueType: "Task",
            summary: "Wire the gate",
            labels: ["ralphx"],
        });
        expect(options.headers).toEqual({
            "x-ralphx-agent-run-id": "run-1",
            "x-ralphx-conversation-id": "conv-1",
        });
        // Identity must never leak into the model-visible payload.
        expect(payload).not.toHaveProperty("agentRunId");
        expect(payload).not.toHaveProperty("conversationId");
    });
    it("omits identity headers when the runtime context is incomplete", async () => {
        const callTauri = vi.fn().mockResolvedValue({ ok: true });
        await callAtlassianTool("jira_search_issues", callTauri, { query: "x" }, {
            conversationId: "conv-1",
        });
        expect(callTauri.mock.calls[0][2]).toBeUndefined();
    });
    it("normalizes non-object arguments to an empty payload", async () => {
        const callTauri = vi.fn().mockResolvedValue({ ok: true });
        await callAtlassianTool("jira_list_projects", callTauri, undefined);
        expect(callTauri.mock.calls[0][1]).toEqual({});
    });
});
//# sourceMappingURL=atlassian-tools.test.js.map