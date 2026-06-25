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
  it("does not render when the conversation has no active runtime", () => {
    const { container } = render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({ isRunning: false, items: [] })}
        onViewWorkspace={vi.fn()}
        onViewIdeation={vi.fn()}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });

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

  it("routes ideation, review, merge, and workspace CTA variants", () => {
    const onViewWorkspace = vi.fn();
    const onViewIdeation = vi.fn();
    const onViewTaskRuntime = vi.fn();

    render(
      <AgentRuntimeStatusWidget
        status={runtimeStatus({
          primarySource: "merge",
          summaryLabel: "Merging tasks",
          items: [
            {
              source: "ideation",
              contextType: "ideation",
              contextId: "session-1",
              label: "Ideation running",
              title: "Plan chat",
              agentStatus: "generating",
              taskId: null,
              internalStatus: null,
              runningProcess: null,
              ideationSession: null,
              parentSessionId: null,
              childSessionId: null,
              conversationId: null,
            },
            {
              source: "review",
              contextType: "review",
              contextId: "task-2",
              label: "Reviewing",
              title: "Review task",
              agentStatus: "generating",
              taskId: "task-2",
              internalStatus: "reviewing",
              runningProcess: null,
              ideationSession: null,
              parentSessionId: null,
              childSessionId: null,
              conversationId: null,
            },
            {
              source: "merge",
              contextType: "merge",
              contextId: "task-3",
              label: "Merging",
              title: "Merge task",
              agentStatus: "generating",
              taskId: "task-3",
              internalStatus: "pending_merge",
              runningProcess: null,
              ideationSession: null,
              parentSessionId: null,
              childSessionId: null,
              conversationId: null,
            },
            {
              source: "workspace",
              contextType: "project",
              contextId: "conversation-1",
              label: "Agent running",
              title: "Workspace chat",
              agentStatus: "waiting_for_input",
              taskId: null,
              internalStatus: null,
              runningProcess: null,
              ideationSession: null,
              parentSessionId: null,
              childSessionId: null,
              conversationId: "conversation-1",
            },
          ],
        })}
        onViewWorkspace={onViewWorkspace}
        onViewIdeation={onViewIdeation}
        onViewVerification={vi.fn()}
        onViewTaskRuntime={onViewTaskRuntime}
      />,
    );

    expect(screen.getByText("4 active runtimes")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "View Ideation" }));
    fireEvent.click(screen.getAllByRole("button", { name: "View Task" })[0]);
    fireEvent.click(screen.getAllByRole("button", { name: "View Task" })[1]);
    fireEvent.click(screen.getByRole("button", { name: "View Workspace" }));

    expect(onViewIdeation).toHaveBeenCalledWith("session-1");
    expect(onViewTaskRuntime).toHaveBeenNthCalledWith(1, "task-2", "review");
    expect(onViewTaskRuntime).toHaveBeenNthCalledWith(2, "task-3", "merge");
    expect(onViewWorkspace).toHaveBeenCalledTimes(1);
  });
});
