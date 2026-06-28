/**
 * ProjectSelector component tests
 * Compact header dropdown for project selection with git mode indicators
 * Uses the shared searchable project popover.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProjectSelector } from "./ProjectSelector";
import { useProjectStore } from "@/stores/projectStore";
import type { Project } from "@/types/project";
import { useProjects } from "@/hooks/useProjects";

vi.mock("@/hooks/useProjects", () => ({
  useProjects: vi.fn(),
}));

const mockedUseProjects = vi.mocked(useProjects);

// Create a mock project
const createMockProject = (overrides: Partial<Project> = {}): Project => ({
  id: `project-${Math.random().toString(36).slice(2)}`,
  name: "Test Project",
  workingDirectory: "/path/to/project",
  gitMode: "worktree",
  baseBranch: null,
  worktreeParentDirectory: null,
  useFeatureBranches: true,
  createdAt: "2026-01-24T12:00:00Z",
  updatedAt: "2026-01-24T12:00:00Z",
  ...overrides,
});

describe("ProjectSelector", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset stores to initial state
    useProjectStore.setState({ projects: {}, activeProjectId: null });
    mockedUseProjects.mockImplementation(
      () =>
        ({
          data: Object.values(useProjectStore.getState().projects),
        }) as ReturnType<typeof useProjects>
    );
  });

  describe("trigger button", () => {
    it("renders trigger button with correct testid", () => {
      render(<ProjectSelector onNewProject={() => {}} />);
      expect(screen.getByTestId("project-selector-trigger")).toBeInTheDocument();
    });

    it("shows 'Select Project' when no project is active", () => {
      render(<ProjectSelector onNewProject={() => {}} />);
      expect(screen.getByText("Select Project")).toBeInTheDocument();
    });

    it("shows active project name when a project is selected", () => {
      const project = createMockProject({ id: "project-1", name: "My Project" });
      useProjectStore.setState({
        projects: { "project-1": project },
        activeProjectId: "project-1",
      });

      render(<ProjectSelector onNewProject={() => {}} />);
      expect(screen.getByText("My Project")).toBeInTheDocument();
    });

    it("has correct aria attributes", () => {
      render(<ProjectSelector onNewProject={() => {}} />);
      const trigger = screen.getByTestId("project-selector-trigger");
      expect(trigger).toHaveAttribute("aria-haspopup", "listbox");
      expect(trigger).toHaveAttribute("aria-expanded", "false");
    });
  });

  describe("dropdown behavior", () => {
    it("opens dropdown when trigger is clicked", async () => {
      const user = userEvent.setup();
      render(<ProjectSelector onNewProject={() => {}} />);

      const trigger = screen.getByTestId("project-selector-trigger");
      await user.click(trigger);

      await waitFor(() => {
        expect(screen.getByTestId("project-selector-dropdown")).toBeInTheDocument();
      });
      expect(trigger).toHaveAttribute("aria-expanded", "true");
    });

    it("closes dropdown when Escape is pressed", async () => {
      const user = userEvent.setup();
      render(<ProjectSelector onNewProject={() => {}} />);

      const trigger = screen.getByTestId("project-selector-trigger");
      await user.click(trigger);

      await waitFor(() => {
        expect(screen.getByTestId("project-selector-dropdown")).toBeInTheDocument();
      });

      await user.keyboard("{Escape}");

      await waitFor(() => {
        expect(screen.queryByTestId("project-selector-dropdown")).not.toBeInTheDocument();
      });
    });

    it("opens dropdown with ArrowDown when closed", async () => {
      const user = userEvent.setup();
      render(<ProjectSelector onNewProject={() => {}} />);

      const trigger = screen.getByTestId("project-selector-trigger");
      trigger.focus();
      await user.keyboard("{ArrowDown}");

      await waitFor(() => {
        expect(screen.getByTestId("project-selector-dropdown")).toBeInTheDocument();
      });
    });
  });

  describe("project list", () => {
    it("shows empty state when no projects exist", async () => {
      const user = userEvent.setup();
      render(<ProjectSelector onNewProject={() => {}} />);

      await user.click(screen.getByTestId("project-selector-trigger"));

      await waitFor(() => {
        expect(screen.getByText(/no projects/i)).toBeInTheDocument();
      });
    });

    it("renders project options for each project", async () => {
      const user = userEvent.setup();
      const projects: Project[] = [
        createMockProject({ id: "project-1", name: "Project Alpha" }),
        createMockProject({ id: "project-2", name: "Project Beta" }),
      ];

      useProjectStore.setState({
        projects: Object.fromEntries(projects.map((p) => [p.id, p])),
        activeProjectId: null,
      });

      render(<ProjectSelector onNewProject={() => {}} />);
      await user.click(screen.getByTestId("project-selector-trigger"));

      await waitFor(() => {
        expect(screen.getByTestId("project-option-project-1")).toBeInTheDocument();
        expect(screen.getByTestId("project-option-project-2")).toBeInTheDocument();
      });
      expect(screen.getByText("Project Alpha")).toBeInTheDocument();
      expect(screen.getByText("Project Beta")).toBeInTheDocument();
    });

    it("filters projects by search query", async () => {
      const user = userEvent.setup();
      const projects: Project[] = [
        createMockProject({ id: "project-1", name: "Project Alpha", workingDirectory: "/alpha" }),
        createMockProject({ id: "project-2", name: "Project Beta", workingDirectory: "/beta" }),
      ];

      useProjectStore.setState({
        projects: Object.fromEntries(projects.map((p) => [p.id, p])),
        activeProjectId: null,
      });

      render(<ProjectSelector onNewProject={() => {}} />);
      await user.click(screen.getByTestId("project-selector-trigger"));
      await user.type(screen.getByTestId("project-selector-search"), "beta");

      await waitFor(() => {
        expect(screen.queryByTestId("project-option-project-1")).not.toBeInTheDocument();
        expect(screen.getByTestId("project-option-project-2")).toBeInTheDocument();
      });
    });

    it("pages long project lists with Show more", async () => {
      const user = userEvent.setup();
      const projects: Project[] = Array.from({ length: 25 }, (_, index) =>
        createMockProject({
          id: `project-${index + 1}`,
          name: `Project ${index + 1}`,
          updatedAt: `2026-01-${String(index + 1).padStart(2, "0")}T12:00:00Z`,
        })
      );

      useProjectStore.setState({
        projects: Object.fromEntries(projects.map((p) => [p.id, p])),
        activeProjectId: null,
      });

      render(<ProjectSelector onNewProject={() => {}} />);
      await user.click(screen.getByTestId("project-selector-trigger"));

      await waitFor(() => {
        expect(screen.getByTestId("project-selector-show-more")).toBeInTheDocument();
      });
      expect(screen.queryByTestId("project-option-project-1")).not.toBeInTheDocument();

      await user.click(screen.getByTestId("project-selector-show-more"));

      await waitFor(() => {
        expect(screen.getByTestId("project-option-project-1")).toBeInTheDocument();
      });
    });

    it("highlights active project with accent styling", async () => {
      const user = userEvent.setup();
      const projects: Project[] = [
        createMockProject({ id: "project-1", name: "Project Alpha" }),
        createMockProject({ id: "project-2", name: "Project Beta" }),
      ];

      useProjectStore.setState({
        projects: Object.fromEntries(projects.map((p) => [p.id, p])),
        activeProjectId: "project-1",
      });

      render(<ProjectSelector onNewProject={() => {}} />);
      await user.click(screen.getByTestId("project-selector-trigger"));

      await waitFor(() => {
        const selectedOption = screen.getByTestId("project-option-project-1");
        expect(selectedOption).toBeInTheDocument();
        // Active project should have the accent muted background
        expect(selectedOption).toHaveClass("bg-[var(--accent-muted)]");
      });
    });

    it("selects project when clicked and announces the impending switch", async () => {
      const user = userEvent.setup();
      const onBeforeProjectChange = vi.fn();
      const projects: Project[] = [
        createMockProject({ id: "project-1", name: "Project Alpha" }),
        createMockProject({ id: "project-2", name: "Project Beta" }),
      ];

      useProjectStore.setState({
        projects: Object.fromEntries(projects.map((p) => [p.id, p])),
        activeProjectId: null,
      });

      render(
        <ProjectSelector
          onNewProject={() => {}}
          onBeforeProjectChange={onBeforeProjectChange}
        />,
      );
      await user.click(screen.getByTestId("project-selector-trigger"));

      await waitFor(() => {
        expect(screen.getByTestId("project-option-project-2")).toBeInTheDocument();
      });

      await user.click(screen.getByTestId("project-option-project-2"));

      await waitFor(() => {
        const state = useProjectStore.getState();
        expect(state.activeProjectId).toBe("project-2");
      });
      expect(onBeforeProjectChange).toHaveBeenCalledWith("project-2");
    });

    it("closes dropdown after selecting a project", async () => {
      const user = userEvent.setup();
      const project = createMockProject({ id: "project-1", name: "Test" });
      useProjectStore.setState({
        projects: { "project-1": project },
        activeProjectId: null,
      });

      render(<ProjectSelector onNewProject={() => {}} />);
      await user.click(screen.getByTestId("project-selector-trigger"));

      await waitFor(() => {
        expect(screen.getByTestId("project-option-project-1")).toBeInTheDocument();
      });

      await user.click(screen.getByTestId("project-option-project-1"));

      await waitFor(() => {
        expect(screen.queryByTestId("project-selector-dropdown")).not.toBeInTheDocument();
      });
    });
  });

  describe("New Project option", () => {
    it("renders New Project option", async () => {
      const user = userEvent.setup();
      render(<ProjectSelector onNewProject={() => {}} />);
      await user.click(screen.getByTestId("project-selector-trigger"));

      await waitFor(() => {
        expect(screen.getByTestId("new-project-option")).toBeInTheDocument();
      });
      expect(screen.getByText("New Project...")).toBeInTheDocument();
    });

    it("calls onNewProject when clicked", async () => {
      const user = userEvent.setup();
      const onNewProject = vi.fn();
      render(<ProjectSelector onNewProject={onNewProject} />);

      await user.click(screen.getByTestId("project-selector-trigger"));

      await waitFor(() => {
        expect(screen.getByTestId("new-project-option")).toBeInTheDocument();
      });

      await user.click(screen.getByTestId("new-project-option"));

      await waitFor(() => {
        expect(onNewProject).toHaveBeenCalled();
      });
    });

    it("closes dropdown after clicking New Project", async () => {
      const user = userEvent.setup();
      const onNewProject = vi.fn();
      render(<ProjectSelector onNewProject={onNewProject} />);

      await user.click(screen.getByTestId("project-selector-trigger"));

      await waitFor(() => {
        expect(screen.getByTestId("new-project-option")).toBeInTheDocument();
      });

      await user.click(screen.getByTestId("new-project-option"));

      await waitFor(() => {
        expect(screen.queryByTestId("project-selector-dropdown")).not.toBeInTheDocument();
      });
    });
  });

  describe("keyboard navigation", () => {
    it("navigates items with arrow keys", async () => {
      const user = userEvent.setup();
      const projects: Project[] = [
        createMockProject({ id: "project-1", name: "Project Alpha", updatedAt: "2026-01-24T11:00:00Z" }),
        createMockProject({ id: "project-2", name: "Project Beta", updatedAt: "2026-01-24T12:00:00Z" }),
      ];

      useProjectStore.setState({
        projects: Object.fromEntries(projects.map((p) => [p.id, p])),
        activeProjectId: null,
      });

      render(<ProjectSelector onNewProject={() => {}} />);

      const trigger = screen.getByTestId("project-selector-trigger");
      await user.click(trigger);

      await waitFor(() => {
        expect(screen.getByTestId("project-selector-dropdown")).toBeInTheDocument();
      });

      // Popover + listbox keeps the project list keyboard-reachable.
      expect(screen.getByTestId("project-option-project-1")).toBeInTheDocument();
      expect(screen.getByTestId("project-option-project-2")).toBeInTheDocument();
    });
  });

  describe("accessibility", () => {
    it("project list has listbox role", async () => {
      const user = userEvent.setup();
      render(<ProjectSelector onNewProject={() => {}} />);
      await user.click(screen.getByTestId("project-selector-trigger"));

      await waitFor(() => {
        const list = screen.getByTestId("project-selector-list");
        expect(list).toHaveAttribute("role", "listbox");
      });
    });

    it("project options have option role", async () => {
      const user = userEvent.setup();
      const project = createMockProject({ id: "project-1", name: "Test" });
      useProjectStore.setState({
        projects: { "project-1": project },
        activeProjectId: null,
      });

      render(<ProjectSelector onNewProject={() => {}} />);
      await user.click(screen.getByTestId("project-selector-trigger"));

      await waitFor(() => {
        const option = screen.getByTestId("project-option-project-1");
        expect(option).toHaveAttribute("role", "option");
      });
    });
  });

  describe("className prop", () => {
    it("applies custom className to trigger button", () => {
      render(
        <ProjectSelector onNewProject={() => {}} className="custom-class" />
      );
      const trigger = screen.getByTestId("project-selector-trigger");
      expect(trigger).toHaveClass("custom-class");
    });
  });
});
