import { describe, expect, it } from "vitest";
import { transformMergePipelineTask } from "./merge-pipeline.transforms";

describe("transformMergePipelineTask", () => {
  const baseRaw = {
    task_id: "merge-task-1",
    title: "Merge plan into main",
    internal_status: "pending_merge",
    source_branch: "ralphx/project/task",
    target_branch: "main",
    is_deferred: false,
    is_main_merge_deferred: false,
    blocking_branch: null,
    conflict_files: null,
    error_context: null,
  };

  it("falls back to the task title when display_title is absent", () => {
    expect(transformMergePipelineTask(baseRaw)).toMatchObject({
      taskId: "merge-task-1",
      title: "Merge plan into main",
      displayTitle: "Merge plan into main",
    });
  });

  it("transforms display title and optional Agent workspace target", () => {
    expect(
      transformMergePipelineTask({
        ...baseRaw,
        display_title: "Agent Conversation Workspace",
        agent_workspace: {
          conversation_id: "conversation-1",
          project_id: "project-1",
          title: "Agent Conversation Workspace",
        },
      }),
    ).toMatchObject({
      displayTitle: "Agent Conversation Workspace",
      agentWorkspace: {
        conversationId: "conversation-1",
        projectId: "project-1",
        title: "Agent Conversation Workspace",
      },
    });
  });
});
