import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RecentRepositoriesList } from "./RecentRepositoriesList";
import { useProjectStore } from "@/stores/projectStore";
import type { Project } from "@/types/project";

function makeProject(overrides: Partial<Project>): Project {
  return {
    id: "proj-1",
    name: "My App",
    workingDirectory: "/Users/dev/my-app",
    baseBranch: "main",
    gitMode: "worktree",
    worktreeParentDirectory: "~/ralphx-worktrees",
    useFeatureBranches: true,
    mergeValidationMode: "block",
    detectedAnalysis: null,
    customAnalysis: null,
    analyzedAt: null,
    githubPrEnabled: false,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

describe("RecentRepositoriesList", () => {
  beforeEach(() => {
    useProjectStore.setState({ projects: {}, activeProjectId: null });
  });

  it("renders nothing when there are no recents", () => {
    render(<RecentRepositoriesList recents={[]} onSelect={vi.fn()} />);
    expect(screen.queryByTestId("recent-repositories-list")).not.toBeInTheDocument();
  });

  it("hides entries whose path already belongs to a registered project", () => {
    useProjectStore.setState({
      projects: { "proj-1": makeProject({ workingDirectory: "/Users/dev/my-app" }) },
      activeProjectId: null,
    });

    render(
      <RecentRepositoriesList
        recents={[
          { path: "/Users/dev/my-app", name: "my-app", lastUsedAt: "2026-08-01T00:00:00Z" },
          { path: "/Users/dev/other-app", name: "other-app", lastUsedAt: "2026-08-02T00:00:00Z" },
        ]}
        onSelect={vi.fn()}
      />
    );

    expect(screen.getAllByTestId("recent-repository-item")).toHaveLength(1);
    expect(screen.getByText("other-app")).toBeInTheDocument();
    expect(screen.queryByText("my-app")).not.toBeInTheDocument();
  });

  it("hides a registered entry regardless of path casing (macOS case-insensitivity)", () => {
    useProjectStore.setState({
      projects: { "proj-1": makeProject({ workingDirectory: "/Users/Dev/My-App" }) },
      activeProjectId: null,
    });

    render(
      <RecentRepositoriesList
        recents={[{ path: "/users/dev/my-app", name: "my-app", lastUsedAt: "2026-08-01T00:00:00Z" }]}
        onSelect={vi.fn()}
      />
    );

    expect(screen.queryByTestId("recent-repositories-list")).not.toBeInTheDocument();
  });

  it("feeds the normal probe path on selection - no validation bypass", async () => {
    const user = userEvent.setup();
    const onSelect = vi.fn();
    render(
      <RecentRepositoriesList
        recents={[{ path: "/Users/dev/other-app", name: "other-app", lastUsedAt: "2026-08-02T00:00:00Z" }]}
        onSelect={onSelect}
      />
    );

    await user.click(screen.getByTestId("recent-repository-item"));
    expect(onSelect).toHaveBeenCalledWith("/Users/dev/other-app");
  });
});
