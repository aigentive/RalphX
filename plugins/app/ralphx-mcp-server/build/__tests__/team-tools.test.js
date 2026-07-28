import { afterEach, describe, expect, it } from "vitest";
import { applyTeamToolPolicy } from "../team-tool-policy.js";
import { callTeamTool, TEAM_TOOLS } from "../team-tools.js";
function captureCalls() {
    const calls = [];
    return {
        calls,
        post: async (path, body, options) => {
            calls.push({ path, body, options });
            return { ok: true };
        },
        get: async (path, options) => {
            calls.push({ path, options });
            return { ok: true };
        },
    };
}
describe("Team coordinator tools", () => {
    afterEach(() => {
        delete process.env.RALPHX_COORDINATION_MODE;
    });
    it("exposes no model-facing Team ids", () => {
        for (const tool of TEAM_TOOLS) {
            const properties = tool.inputSchema.properties;
            expect(Object.keys(properties).some((name) => /(^|_)id$|run|session/i.test(name))).toBe(false);
        }
    });
    it("denies Team tools outside RX-native Team mode", () => {
        expect(applyTeamToolPolicy(["team_assign", "get_artifact"])).toEqual(["get_artifact"]);
        process.env.RALPHX_COORDINATION_MODE = "rx_native_team";
        expect(applyTeamToolPolicy(["team_assign", "get_artifact"])).toEqual([
            "team_assign",
            "get_artifact",
        ]);
    });
    it("sends coordinator authority only through transport headers", async () => {
        const { calls, post, get } = captureCalls();
        await callTeamTool("team_assign", post, get, { member_name: "worker one", task_ref: "2", work_classification: "read_only" }, { conversationId: "conversation-1", agentRunId: "run-1" });
        expect(calls).toEqual([
            {
                path: "managed_team/member/assign",
                body: { member_name: "worker one", task_ref: "2", work_classification: "read_only" },
                options: {
                    headers: {
                        "x-ralphx-conversation-id": "conversation-1",
                        "x-ralphx-agent-run-id": "run-1",
                    },
                },
            },
        ]);
    });
    it("rejects Team dispatch without trusted runtime authority", async () => {
        const { post, get } = captureCalls();
        await expect(callTeamTool("team_list", post, get, {}, {})).rejects.toThrow("requires trusted coordinator conversation and run context");
    });
});
//# sourceMappingURL=team-tools.test.js.map