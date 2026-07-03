import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { agentTaskApi } from "./agent-tasks";
import { backendApiUrl } from "./backend";

const fetchMock = vi.fn();

function jsonResponse(body: unknown, init: ResponseInit = {}) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
    ...init,
  });
}

describe("agentTaskApi", () => {
  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("lists agent tasks and transforms backend fields", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({
        success: true,
        tasks: [
          {
            task_id: "task-1",
            task_number: 1,
            title: "Map ledger scope",
            state: "active",
            owner_agent: "ralphx-chat-project",
            blocked_by: ["task-0"],
            blocks: ["task-2"],
            availability: "blocked",
            updated_at: "2026-05-20T01:00:00Z",
          },
        ],
      }),
    );

    await expect(
      agentTaskApi.listAgentTasks({
        contextType: "conversation",
        contextId: "conversation-1",
        projectId: "project-1",
        includeDone: true,
      }),
    ).resolves.toEqual([
      {
        taskId: "task-1",
        taskNumber: 1,
        title: "Map ledger scope",
        state: "active",
        ownerAgent: "ralphx-chat-project",
        blockedBy: ["task-0"],
        blocks: ["task-2"],
        availability: "blocked",
        updatedAt: "2026-05-20T01:00:00Z",
      },
    ]);

    expect(fetchMock).toHaveBeenCalledWith(
      backendApiUrl("agent_tasks/list"),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          context_type: "conversation",
          context_id: "conversation-1",
          project_id: "project-1",
          include_done: true,
        }),
      },
    );
  });

  it("lists conversation tasks with optional owner normalized to null", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({
        success: true,
        tasks: [
          {
            task_id: "task-2",
            task_number: 2,
            title: "Verify composer tray",
            state: "open",
            blocked_by: [],
            blocks: [],
            availability: "ready",
            updated_at: "2026-05-20T01:01:00Z",
          },
        ],
      }),
    );

    const tasks = await agentTaskApi.listConversationTasks({
      conversationId: "conversation-2",
      includeDone: false,
    });

    expect(tasks[0]?.ownerAgent).toBeNull();
    expect(fetchMock).toHaveBeenCalledWith(
      backendApiUrl("agent_tasks/list"),
      expect.objectContaining({
        body: JSON.stringify({
          context_type: "conversation",
          context_id: "conversation-2",
          include_done: false,
        }),
      }),
    );
  });

  it("throws backend errors from unsuccessful task responses", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({
        success: false,
        tasks: [],
        error: "agent task list request failed",
      }),
    );

    await expect(
      agentTaskApi.listAgentTasks({
        contextType: "project",
        contextId: "project-1",
      }),
    ).rejects.toThrow("agent task list request failed");
  });

  it("throws HTTP failures", async () => {
    fetchMock.mockResolvedValue(jsonResponse({}, { status: 500, statusText: "Server Error" }));

    await expect(
      agentTaskApi.listConversationTasks({
        conversationId: "conversation-1",
      }),
    ).rejects.toThrow("Agent task request failed: 500 Server Error");
  });
});
