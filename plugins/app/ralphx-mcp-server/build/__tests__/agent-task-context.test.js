import { describe, expect, it } from "vitest";
import { resolveAgentTaskContext, withAgentTaskRuntimeContext, } from "../agent-task-context.js";
describe("agent task runtime context", () => {
    it("uses the parent conversation as the ledger scope when available", () => {
        expect(resolveAgentTaskContext({
            contextType: "project",
            contextId: "project-1",
            projectId: "project-1",
            actorAgent: "ralphx-chat-project",
            parentConversationId: "conversation-1",
        })).toEqual({
            context_type: "conversation",
            context_id: "conversation-1",
            project_id: "project-1",
            actor_agent: "ralphx-chat-project",
        });
    });
    it("uses the current conversation as the ledger scope when no parent is present", () => {
        expect(resolveAgentTaskContext({
            contextType: "project",
            contextId: "project-1",
            projectId: "project-1",
            conversationId: "conversation-1",
        })).toEqual({
            context_type: "conversation",
            context_id: "conversation-1",
            project_id: "project-1",
        });
    });
    it("keeps delegated task tools on the delegate-local ledger when lineage is present", () => {
        expect(resolveAgentTaskContext({
            contextType: "delegation",
            contextId: "delegated-session-1",
            projectId: "project-1",
            actorAgent: "ralphx-general-worker",
            parentConversationId: "root-conversation-1",
        })).toEqual({
            context_type: "delegation",
            context_id: "delegated-session-1",
            project_id: "project-1",
            actor_agent: "ralphx-general-worker",
        });
    });
    it("keeps session-scoped task tools on their runtime session", () => {
        expect(resolveAgentTaskContext({
            contextType: "ideation",
            contextId: "session-1",
            projectId: "project-1",
            actorAgent: "ralphx-orchestrator-ideation",
        })).toEqual({
            context_type: "ideation",
            context_id: "session-1",
            project_id: "project-1",
            actor_agent: "ralphx-orchestrator-ideation",
        });
    });
    it("uses an agent run ledger when a project runtime has no conversation", () => {
        expect(resolveAgentTaskContext({
            contextType: "project",
            contextId: "project-1",
            projectId: "project-1",
            agentRunId: "run-1",
        })).toEqual({
            context_type: "agent_run",
            context_id: "run-1",
            project_id: "project-1",
        });
    });
    it("refuses project-scoped fallback without an isolated runtime identity", () => {
        const callerArgs = {
            title: "Do not share",
            context_type: "project",
            context_id: "project-1",
        };
        expect(() => withAgentTaskRuntimeContext(callerArgs, {
            contextType: "project",
            contextId: "project-1",
            projectId: "project-1",
        })).toThrow("Agent task ledger requires conversation identity; refusing shared project scope.");
        expect(callerArgs).toEqual({
            title: "Do not share",
            context_type: "project",
            context_id: "project-1",
        });
    });
    it("keeps caller-owned tool arguments while overriding hidden scope fields", () => {
        expect(withAgentTaskRuntimeContext({
            title: "Audit ledger",
            details: "Confirm task widgets render",
            context_type: "project",
            context_id: "project-1",
        }, {
            projectId: "project-1",
            parentConversationId: "conversation-1",
            actorAgent: "ralphx-general-worker",
        })).toEqual({
            title: "Audit ledger",
            details: "Confirm task widgets render",
            context_type: "conversation",
            context_id: "conversation-1",
            project_id: "project-1",
            actor_agent: "ralphx-general-worker",
        });
    });
});
//# sourceMappingURL=agent-task-context.test.js.map