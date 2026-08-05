import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Task } from "@/types/task";
import { TaskContextRail } from "./TaskDetailContextRail";
import type { TaskDetailContextModel } from "./TaskDetailContext";

function contextModel(): TaskDetailContextModel {
  return {
    task: {
      id: "task-1",
      projectId: "project-1",
      title: "Inline validation",
      description: "Move validation into the task detail body.",
      internalStatus: "executing",
      category: "feature",
    } as unknown as Task,
    viewMode: { kind: "current" },
    taskContext: null,
    planBranch: null,
    isLoading: false,
    isUnavailable: false,
    planArtifactId: null,
    sessionId: null,
    branch: null,
    pullRequest: null,
    merge: null,
  };
}

describe("TaskContextRail", () => {
  it("does not render task validation as a rail section", () => {
    render(<TaskContextRail model={contextModel()} />);

    expect(screen.getByText("Task")).toBeInTheDocument();
    expect(
      screen.getByText("Move validation into the task detail body."),
    ).toBeInTheDocument();
    expect(screen.queryByText("Task Validation")).not.toBeInTheDocument();
    expect(screen.queryByText("Validation")).not.toBeInTheDocument();
  });

  it("renders unavailable plan context instead of hiding the surface", () => {
    render(<TaskContextRail model={{ ...contextModel(), isUnavailable: true }} />);

    expect(screen.getByText("Plan, branch, and merge details could not be loaded.")).toBeInTheDocument();
  });
});
