import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Task } from "@/types/task";
import { TaskDetailContext } from "./TaskDetailContext";
import type { TaskDetailContextModel } from "./TaskDetailContext";
import { TwoColumnLayout } from "./TwoColumnLayout";

function expectBefore(first: HTMLElement, second: HTMLElement) {
  expect(
    first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
}

function contextModel(): TaskDetailContextModel {
  return {
    task: {
      id: "task-1",
      projectId: "project-1",
      title: "One column task detail",
      description: "Readable task summary belongs in the main column.",
      internalStatus: "merged",
      category: "feature",
      taskBranch: "task/one-column",
      completedAt: "2026-07-07T12:00:00.000Z",
      mergeCommitSha: "abcdef1234567890",
    } as unknown as Task,
    viewMode: {
      kind: "historical",
      status: "merged",
      timestamp: "2026-07-07T12:00:00.000Z",
      conversationId: "conversation-1",
    },
    taskContext: null,
    planBranch: null,
    isLoading: false,
    isUnavailable: false,
    planArtifactId: null,
    sessionId: null,
    branch: {
      label: "Task branch",
      source: "task/one-column",
      target: "main",
      status: null,
    },
    pullRequest: null,
    merge: {
      target: "main",
      commitSha: "abcdef1234567890",
      mergedAt: "2026-07-07T12:00:00.000Z",
    },
  };
}

describe("TwoColumnLayout", () => {
  it("renders the Agents detail shell as one ordered column without an automatic validation footer", () => {
    render(
      <TaskDetailContext.Provider value={contextModel()}>
        <TwoColumnLayout
          description="Fallback description"
          testId="task-detail-shell"
          evidence={<section>Evidence Slot</section>}
          actions={<section>Actions Slot</section>}
        >
          <section>Stage Body</section>
        </TwoColumnLayout>
      </TaskDetailContext.Provider>,
    );

    const shell = screen.getByTestId("task-detail-shell");
    expect(shell).not.toHaveClass("grid");
    expect(shell.className).not.toContain("xl:grid-cols");

    const summary = within(shell).getByTestId("task-detail-summary");
    const stageBody = within(shell).getByTestId("task-detail-stage-body");
    const evidence = within(shell).getByTestId("task-detail-evidence");
    const context = within(shell).getByTestId("task-detail-context");
    const actions = within(shell).getByTestId("task-detail-actions");

    expect(within(summary).getByText("Task")).toBeInTheDocument();
    expect(
      within(summary).getByText("Readable task summary belongs in the main column."),
    ).toBeInTheDocument();
    expect(within(summary).getByText("Historical State")).toBeInTheDocument();
    expect(within(stageBody).getByText("Stage Body")).toBeInTheDocument();
    expect(within(evidence).getByText("Evidence Slot")).toBeInTheDocument();
    expect(within(context).getByText("Merge")).toBeInTheDocument();
    expect(within(actions).getByText("Actions Slot")).toBeInTheDocument();
    expectBefore(summary, stageBody);
    expectBefore(stageBody, evidence);
    expectBefore(evidence, context);
    expectBefore(context, actions);
    expect(screen.queryByTestId("task-validation-section")).not.toBeInTheDocument();
  });
});
