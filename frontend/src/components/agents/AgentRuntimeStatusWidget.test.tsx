import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AgentConversationRuntimeStatus } from "@/api/chat";
import { AgentRuntimeStatusWidget } from "./AgentRuntimeStatusWidget";

function runtimeStatus(
  overrides: Partial<AgentConversationRuntimeStatus> = {},
): AgentConversationRuntimeStatus {
  return {
    conversationId: "conversation-1",
    isRunning: true,
    agentStatus: "generating",
    primarySource: "task_execution",
    summaryLabel: "Executing",
    items: [
      {
        source: "task_execution",
        contextType: "task_execution",
        contextId: "task-1",
        label: "Executing",
        title: "Runtime task",
        agentStatus: "generating",
        taskId: "task-1",
        internalStatus: "executing",
        runningProcess: null,
        ideationSession: null,
        parentSessionId: null,
        childSessionId: null,
        conversationId: null,
      },
    ],
    ...overrides,
  };
}

describe("AgentRuntimeStatusWidget", () => {
  it("renders active runtime status and routes task CTA", () => {
    const onViewTaskRuntime = vi.fn();

    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus()}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={onViewTaskRuntime}
      />,
    );

    expect(screen.getAllByText("Executing")).toHaveLength(2);
    expect(screen.getAllByText("Runtime task")).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "View Task" }));

    expect(onViewTaskRuntime).toHaveBeenCalledWith("task-1", "task_execution");
  });

  it("routes verification CTA with parent and child session ids", () => {
    const onViewVerification = vi.fn();

    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          primarySource: "verification",
          summaryLabel: "Verifying",
          items: [
            {
              source: "verification",
              contextType: "ideation",
              contextId: "child-session",
              label: "Verifying",
              title: "Verification run",
              agentStatus: "generating",
              taskId: null,
              internalStatus: null,
              runningProcess: null,
              ideationSession: null,
              parentSessionId: "parent-session",
              childSessionId: "child-session",
              conversationId: null,
            },
          ],
        })}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={onViewVerification}
        onViewTaskRuntime={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "View Verification" }));

    expect(onViewVerification).toHaveBeenCalledWith(
      "parent-session",
      "child-session",
    );
  });
});
