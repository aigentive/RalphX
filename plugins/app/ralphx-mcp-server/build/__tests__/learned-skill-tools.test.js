import { describe, expect, it } from "vitest";
import { LEARNED_SKILL_TOOLS, learnedSkillEndpoint, learnedSkillTransportOptions, } from "../learned-skill-tools.js";
describe("learned skill dispatch", () => {
    it("maps every read and write tool to its backend route", () => {
        expect(learnedSkillEndpoint("list_project_skills")).toBe("project_skills/list");
        expect(learnedSkillEndpoint("get_project_skill")).toBe("project_skills/get");
        expect(learnedSkillEndpoint("upsert_project_skill")).toBe("project_skills/upsert");
        expect(learnedSkillEndpoint("patch_project_skill")).toBe("project_skills/patch");
        expect(learnedSkillEndpoint("retire_project_skill")).toBe("project_skills/retire");
    });
    it("adds transport-owned read identity without changing model schemas", () => {
        const runtime = {
            filesystemEnforced: false,
            agentType: "ralphx-memory-maintainer",
            pipelineRole: "memory_maintainer",
            projectId: "project-1",
            contextType: "project",
            contextId: "project-1",
            conversationId: "conversation-1",
            agentRunId: "run-1",
        };
        for (const toolName of ["list_project_skills", "get_project_skill"]) {
            expect(learnedSkillTransportOptions(toolName, runtime)).toEqual({
                headers: {
                    "x-ralphx-conversation-id": "conversation-1",
                    "x-ralphx-agent-run-id": "run-1",
                },
            });
        }
        expect(learnedSkillTransportOptions("upsert_project_skill", runtime)).toEqual({
            headers: {
                "x-ralphx-agent-name": "ralphx-memory-maintainer",
                "x-ralphx-agent-run-id": "run-1",
                "x-ralphx-pipeline-role": "memory_maintainer",
                "x-ralphx-project-id": "project-1",
                "x-ralphx-context-type": "project",
                "x-ralphx-context-id": "project-1",
                "x-ralphx-conversation-id": "conversation-1",
            },
        });
        for (const tool of LEARNED_SKILL_TOOLS) {
            expect(tool.inputSchema.properties).not.toHaveProperty("conversation_id");
            expect(tool.inputSchema.properties).not.toHaveProperty("agent_run_id");
            expect(tool.inputSchema.properties).not.toHaveProperty("orchestration_id");
        }
    });
    it("uses bounded conversation identity for read calls when no run exists", () => {
        expect(learnedSkillTransportOptions("get_project_skill", {
            filesystemEnforced: false,
            conversationId: "conversation-1",
        })).toEqual({
            headers: { "x-ralphx-conversation-id": "conversation-1" },
        });
    });
});
//# sourceMappingURL=learned-skill-tools.test.js.map