import { describe, expect, it } from "vitest";

import {
  resolveAgentTaskContext,
  withAgentTaskRuntimeContext,
} from "../agent-task-context.js";

describe("agent task runtime context", () => {
  it("uses the parent conversation as the ledger scope when available", () => {
    expect(
      resolveAgentTaskContext({
        contextType: "project",
        contextId: "project-1",
        projectId: "project-1",
        actorAgent: "ralphx-chat-project",
        parentConversationId: "conversation-1",
      })
    ).toEqual({
      context_type: "conversation",
      context_id: "conversation-1",
      project_id: "project-1",
      actor_agent: "ralphx-chat-project",
    });
  });

  it("keeps delegated task tools on the delegate-local ledger when lineage is present", () => {
    expect(
      resolveAgentTaskContext({
        contextType: "delegation",
        contextId: "delegated-session-1",
        projectId: "project-1",
        actorAgent: "ralphx-general-worker",
        parentConversationId: "root-conversation-1",
      })
    ).toEqual({
      context_type: "delegation",
      context_id: "delegated-session-1",
      project_id: "project-1",
      actor_agent: "ralphx-general-worker",
    });
  });

  it("falls back to the runtime context when no parent conversation is present", () => {
    expect(
      resolveAgentTaskContext({
        contextType: "ideation",
        contextId: "session-1",
        projectId: "project-1",
        actorAgent: "ralphx-orchestrator-ideation",
      })
    ).toEqual({
      context_type: "ideation",
      context_id: "session-1",
      project_id: "project-1",
      actor_agent: "ralphx-orchestrator-ideation",
    });
  });

  it("keeps caller-owned tool arguments while overriding hidden scope fields", () => {
    expect(
      withAgentTaskRuntimeContext(
        {
          title: "Audit ledger",
          details: "Confirm task widgets render",
          context_type: "project",
          context_id: "project-1",
        },
        {
          projectId: "project-1",
          parentConversationId: "conversation-1",
          actorAgent: "ralphx-general-worker",
        }
      )
    ).toEqual({
      title: "Audit ledger",
      details: "Confirm task widgets render",
      context_type: "conversation",
      context_id: "conversation-1",
      project_id: "project-1",
      actor_agent: "ralphx-general-worker",
    });
  });
});
