import { describe, expect, it } from "vitest";
import { AUTOMATION_SETUP_TOOLS, callAutomationSetupTool, isAutomationSetupToolName, } from "../automation-tools.js";
function capturePost() {
    const calls = [];
    const callTauri = async (path, body, options) => {
        calls.push({ path, body, options });
        return { ok: true };
    };
    return { callTauri, calls };
}
describe("automation setup MCP tools", () => {
    const callerBoundActionTools = [
        "run_automation_now",
        "pause_automation",
        "resume_automation",
        "cancel_automation_run",
        "cancel_automation",
        "restart_automation",
        "retry_automation_judge",
        "retry_automation_plan_judge",
        "skip_automation_judge",
        "get_automation_publish_status",
        "check_automation_publish_readiness",
        "update_automation_from_base",
        "publish_automation_workspace",
    ];
    it("recognizes only automation setup tool names", () => {
        expect(isAutomationSetupToolName("get_automation")).toBe(true);
        expect(isAutomationSetupToolName("update_automation")).toBe(true);
        expect(isAutomationSetupToolName("verify_automation_decomposition")).toBe(true);
        expect(isAutomationSetupToolName("finalize_automation")).toBe(true);
        for (const name of callerBoundActionTools) {
            expect(isAutomationSetupToolName(name)).toBe(true);
        }
        expect(isAutomationSetupToolName("list_projects")).toBe(false);
    });
    it("exposes caller-bound actions without agent-selectable identities", () => {
        for (const name of callerBoundActionTools) {
            const tool = AUTOMATION_SETUP_TOOLS.find((candidate) => candidate.name === name);
            expect(tool, `${name} should be registered`).toBeDefined();
            expect(tool?.inputSchema).toEqual({
                type: "object",
                properties: {},
                required: [],
            });
        }
    });
    it("limits judge retry descriptions to persisted failed states", () => {
        for (const name of [
            "retry_automation_judge",
            "retry_automation_plan_judge",
        ]) {
            const tool = AUTOMATION_SETUP_TOOLS.find((candidate) => candidate.name === name);
            expect(tool?.description).toContain("persisted failed");
            expect(tool?.description).not.toContain("expired");
        }
    });
    it("forwards get_automation with the caller conversation header", async () => {
        const { callTauri, calls } = capturePost();
        await callAutomationSetupTool("get_automation", callTauri, {}, {
            conversationId: "conversation-1",
        });
        expect(calls).toEqual([
            {
                path: "get_automation",
                body: {},
                options: {
                    headers: {
                        "X-RalphX-Caller-Session-Id": "conversation-1",
                    },
                },
            },
        ]);
    });
    it("strips caller-supplied identity from update_automation payloads", async () => {
        const { callTauri, calls } = capturePost();
        await callAutomationSetupTool("update_automation", callTauri, {
            id: "automation-should-not-forward",
            conversation_id: "conversation-should-not-forward",
            name: "Spec automation",
            max_runs: 12,
            max_consecutive_failures: 2,
        }, { conversationId: "conversation-1" });
        expect(calls).toEqual([
            {
                path: "update_automation",
                body: {
                    name: "Spec automation",
                    max_runs: 12,
                    max_consecutive_failures: 2,
                },
                options: {
                    headers: {
                        "X-RalphX-Caller-Session-Id": "conversation-1",
                    },
                },
            },
        ]);
    });
    it("forwards plan gate settings in update_automation payloads", async () => {
        const { callTauri, calls } = capturePost();
        await callAutomationSetupTool("update_automation", callTauri, {
            plan_approval_mode: "automatic",
            pr_merge_mode: "automatic",
            plan_deep_verification: true,
        }, { conversationId: "conversation-1" });
        expect(calls).toEqual([
            {
                path: "update_automation",
                body: {
                    plan_approval_mode: "automatic",
                    pr_merge_mode: "automatic",
                    plan_deep_verification: true,
                },
                options: {
                    headers: {
                        "X-RalphX-Caller-Session-Id": "conversation-1",
                    },
                },
            },
        ]);
    });
    it("exposes plan gate settings in the update_automation schema", () => {
        const updateTool = AUTOMATION_SETUP_TOOLS.find((tool) => tool.name === "update_automation");
        const properties = updateTool?.inputSchema.properties;
        expect(properties.plan_approval_mode).toMatchObject({
            type: "string",
            enum: ["manual", "automatic"],
        });
        expect(properties.pr_merge_mode).toMatchObject({
            type: "string",
            enum: ["manual", "automatic"],
        });
        expect(properties.plan_deep_verification).toMatchObject({
            type: "boolean",
        });
    });
    it("forwards finalize_automation without a body payload", async () => {
        const { callTauri, calls } = capturePost();
        await callAutomationSetupTool("finalize_automation", callTauri, {}, {
            conversationId: "conversation-1",
        });
        expect(calls[0]).toEqual({
            path: "finalize_automation",
            body: {},
            options: {
                headers: {
                    "X-RalphX-Caller-Session-Id": "conversation-1",
                },
            },
        });
    });
    it("forwards decomposition verification with caller-bound identity only", async () => {
        const { callTauri, calls } = capturePost();
        await callAutomationSetupTool("verify_automation_decomposition", callTauri, { automation_id: "must-not-forward" }, { conversationId: "conversation-1" });
        expect(calls[0]).toEqual({
            path: "verify_automation_decomposition",
            body: {},
            options: {
                headers: {
                    "X-RalphX-Caller-Session-Id": "conversation-1",
                },
            },
        });
    });
    it.each(callerBoundActionTools)("forwards %s with caller-bound identity and strips supplied ids", async (name) => {
        const { callTauri, calls } = capturePost();
        await callAutomationSetupTool(name, callTauri, {
            automation_id: "must-not-forward",
            conversation_id: "must-not-forward",
            run_id: "must-not-forward",
        }, { conversationId: "conversation-1" });
        expect(calls).toEqual([
            {
                path: name,
                body: {},
                options: {
                    headers: {
                        "X-RalphX-Caller-Session-Id": "conversation-1",
                    },
                },
            },
        ]);
    });
    it("fails closed when the runtime context lacks a conversation id", async () => {
        const { callTauri } = capturePost();
        await expect(callAutomationSetupTool("get_automation", callTauri, {}, {})).rejects.toThrow("requires the current setup conversation id");
    });
});
//# sourceMappingURL=automation-tools.test.js.map