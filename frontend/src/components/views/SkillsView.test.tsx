import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { SkillsView } from "./SkillsView";

const mockProject = { id: "proj-1", name: "RalphX" };

vi.mock("@/stores/projectStore", () => ({
  selectActiveProject: (state: { project: typeof mockProject | null }) => state.project,
  useProjectStore: vi.fn((selector: (state: { project: typeof mockProject | null }) => unknown) =>
    selector({ project: mockProject }),
  ),
}));

vi.mock("@/components/project-skills/ProjectSkillsCuratorPanel", () => ({
  ProjectSkillsCuratorPanel: ({ projectId }: { projectId: string }) => (
    <div data-testid="project-skills-curator" data-project={projectId} />
  ),
}));

vi.mock("@/components/projects/ProjectSelector", () => ({
  ProjectSelector: () => <button type="button">RalphX</button>,
}));

describe("SkillsView", () => {
  it("renders the project skills curator on the dedicated skills view", () => {
    render(<SkillsView />);

    expect(screen.getByTestId("skills-view")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Project Skills" })).toBeInTheDocument();
    expect(screen.getByText("Project-scoped skills")).toBeInTheDocument();
    expect(screen.getByText("Project")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "RalphX" })).toBeInTheDocument();
    expect(screen.getByTestId("project-skills-curator")).toHaveAttribute(
      "data-project",
      "proj-1",
    );
  });
});
