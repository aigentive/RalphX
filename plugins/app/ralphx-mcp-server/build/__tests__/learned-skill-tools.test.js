import { describe, expect, it } from "vitest";
import { learnedSkillEndpoint, learnedSkillTransportOptions, } from "../learned-skill-tools.js";
describe("learned skill dispatch", () => {
    it("maps every read and write tool to its backend route", () => {
        expect(learnedSkillEndpoint("list_project_skills")).toBe("project_skills/list");
        expect(learnedSkillEndpoint("get_project_skill")).toBe("project_skills/get");
        expect(learnedSkillEndpoint("upsert_project_skill")).toBe("project_skills/upsert");
        expect(learnedSkillEndpoint("patch_project_skill")).toBe("project_skills/patch");
        expect(learnedSkillEndpoint("retire_project_skill")).toBe("project_skills/retire");
    });
    it("adds hidden runtime headers only to write calls", () => {
        const runtime = {
            filesystemEnforced: false,
            agentType: "ralphx-memory-maintainer",
            pipelineRole: "memory_maintainer",
            projectId: "project-1",
            contextType: "project",
            contextId: "project-1",
            conversationId: "conversation-1",
        };
        expect(learnedSkillTransportOptions("list_project_skills", runtime)).toBeUndefined();
        expect(learnedSkillTransportOptions("get_project_skill", runtime)).toBeUndefined();
        expect(learnedSkillTransportOptions("upsert_project_skill", runtime)).toEqual({
            headers: {
                "x-ralphx-agent-name": "ralphx-memory-maintainer",
                "x-ralphx-pipeline-role": "memory_maintainer",
                "x-ralphx-project-id": "project-1",
                "x-ralphx-context-type": "project",
                "x-ralphx-context-id": "project-1",
                "x-ralphx-conversation-id": "conversation-1",
            },
        });
    });
});
//# sourceMappingURL=learned-skill-tools.test.js.map