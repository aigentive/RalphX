import { describe, expect, it } from "vitest";
import { hydrateRalphxRuntimeEnvFromCli, parseCliOptionFromArgs, } from "../runtime-context.js";
describe("parseCliOptionFromArgs", () => {
    it("supports inline and pair-style CLI options", () => {
        expect(parseCliOptionFromArgs(["node", "index.js", "--context-type=ideation"], "context-type")).toBe("ideation");
        expect(parseCliOptionFromArgs(["node", "index.js", "--context-id", "session-123"], "context-id")).toBe("session-123");
    });
});
describe("hydrateRalphxRuntimeEnvFromCli", () => {
    it("hydrates process-style env values from RalphX CLI args", () => {
        const env = {};
        const runtimeContext = hydrateRalphxRuntimeEnvFromCli([
            "node",
            "index.js",
            "--agent-type",
            "ralphx-plan-verifier",
            "--agent-profile",
            "plan",
            "--context-type",
            "ideation",
            "--context-id",
            "session-123",
            "--conversation-id",
            "conversation-current",
            "--parent-conversation-id",
            "conversation-789",
            "--agent-run-id",
            "run-current",
            "--project-id",
            "project-456",
            "--working-directory",
            "/tmp/workspace",
            "--filesystem-read-root",
            "/tmp/project",
            "--filesystem-read-root=/tmp/shared-artifacts",
            "--tauri-api-url",
            "http://127.0.0.1:3857",
            "--trace-dir",
            "/tmp/ralphx-logs/mcp-proxy",
        ], env);
        expect(runtimeContext.agentType).toBe("ralphx-plan-verifier");
        expect(runtimeContext.agentProfile).toBe("plan");
        expect(runtimeContext.contextType).toBe("ideation");
        expect(runtimeContext.contextId).toBe("session-123");
        expect(runtimeContext.conversationId).toBe("conversation-current");
        expect(runtimeContext.parentConversationId).toBe("conversation-789");
        expect(runtimeContext.agentRunId).toBe("run-current");
        expect(runtimeContext.projectId).toBe("project-456");
        expect(runtimeContext.workingDirectory).toBe("/tmp/workspace");
        expect(runtimeContext.filesystemReadRoots).toBe(JSON.stringify(["/tmp/project", "/tmp/shared-artifacts"]));
        expect(runtimeContext.tauriApiUrl).toBe("http://127.0.0.1:3857");
        expect(runtimeContext.traceDir).toBe("/tmp/ralphx-logs/mcp-proxy");
        expect(env.RALPHX_AGENT_TYPE).toBe("ralphx-plan-verifier");
        expect(env.RALPHX_AGENT_PROFILE).toBe("plan");
        expect(env.RALPHX_CONTEXT_TYPE).toBe("ideation");
        expect(env.RALPHX_CONTEXT_ID).toBe("session-123");
        expect(env.RALPHX_CONVERSATION_ID).toBe("conversation-current");
        expect(env.RALPHX_PARENT_CONVERSATION_ID).toBe("conversation-789");
        expect(env.RALPHX_AGENT_RUN_ID).toBe("run-current");
        expect(env.RALPHX_PROJECT_ID).toBe("project-456");
        expect(env.RALPHX_WORKING_DIRECTORY).toBe("/tmp/workspace");
        expect(env.RALPHX_FILESYSTEM_READ_ROOTS).toBe(JSON.stringify(["/tmp/project", "/tmp/shared-artifacts"]));
        expect(env.TAURI_API_URL).toBe("http://127.0.0.1:3857");
        expect(env.RALPHX_MCP_TRACE_DIR).toBe("/tmp/ralphx-logs/mcp-proxy");
    });
});
//# sourceMappingURL=runtime-context.test.js.map