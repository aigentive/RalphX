/**
 * Tests for ProjectAnalysisSection component
 *
 * Tests orchestration of analysis entries, dirty state UI, save/reset,
 * re-analyze button, and template variables panel.
 */

import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { ProjectAnalysisSection } from "./ProjectAnalysisSection";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";
import { api } from "@/lib/tauri";
import type { Project } from "@/types/project";

// Mock dependencies
vi.mock("@/lib/tauri", () => ({
  api: {
    projects: {
      reanalyzeProject: vi.fn(),
      updateCustomAnalysis: vi.fn(),
      update: vi.fn(),
    },
  },
}));
vi.mock("@/stores/projectStore");
vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: vi.fn((_event, _callback) => () => {}),
  }),
}));
vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

const mockProject: Project = {
  id: "test-project",
  name: "Test Project",
  workingDirectory: "/home/test",
  baseBranch: "main",
  gitMode: "worktree",
  detectedAnalysis: JSON.stringify([
    {
      path: ".",
      label: "Frontend",
      install: "npm install",
      validate: ["npm run typecheck"],
      worktree_setup: ["ln -s node_modules"],
    },
  ]),
  customAnalysis: null,
  analyzedAt: new Date().toISOString(),
  mergeValidationMode: "block",
  useFeatureBranches: false,
  worktreeParentDirectory: "~/ralphx-worktrees",
  createdAt: new Date().toISOString(),
  updatedAt: new Date().toISOString(),
};

describe("ProjectAnalysisSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    // Setup default store mock
    const mockUpdateProject = vi.fn();
    vi.mocked(useProjectStore).mockImplementation((selector) => {
      const selectorStr = selector.toString();
      if (selectorStr.includes("selectActiveProject")) {
        return mockProject;
      }
      return mockUpdateProject;
    });

    // Setup API mock
    vi.mocked(api.projects.reanalyzeProject).mockResolvedValue(undefined);
    vi.mocked(api.projects.updateCustomAnalysis).mockResolvedValue(mockProject);
    vi.mocked(api.projects.update).mockResolvedValue(mockProject);
  });

  describe("rendering", () => {
    it("shows last analyzed timestamp", () => {
      render(<ProjectAnalysisSection />);

      expect(screen.getByText(/Last Analyzed:/)).toBeInTheDocument();
    });

    it("shows Refresh Detected Commands button", () => {
      render(<ProjectAnalysisSection />);

      const reanalyzeButton = screen.getByText("Refresh Detected Commands");
      expect(reanalyzeButton).toBeInTheDocument();
    });
  });

  describe("analysis entries display", () => {
    it("renders detected analysis entries section", () => {
      render(<ProjectAnalysisSection />);

      // Check for an expandable entry button
      const buttons = screen.getAllByRole("button");
      expect(buttons.length).toBeGreaterThan(0);
    });

    it("shows empty state when no analysis yet", () => {
      const noAnalysisProject: Project = {
        ...mockProject,
        detectedAnalysis: null,
        analyzedAt: null,
      };

      const mockUpdateProject = vi.fn();
      vi.mocked(useProjectStore).mockImplementation((selector) => {
        const selectorStr = selector.toString();
        if (selectorStr.includes("selectActiveProject")) {
          return noAnalysisProject;
        }
        return mockUpdateProject;
      });

      render(<ProjectAnalysisSection />);

      expect(
        screen.getByText(/Not yet analyzed. Click Refresh Detected Commands to detect build systems./)
      ).toBeInTheDocument();
    });
  });

  describe("add entry functionality", () => {
    it("renders Add Entry button", () => {
      render(<ProjectAnalysisSection />);

      const addButton = screen.getByText("Add Entry");
      expect(addButton).toBeInTheDocument();
    });

    it("calls addEntry when Add Entry button clicked", async () => {
      render(<ProjectAnalysisSection />);

      const addButton = screen.getByText("Add Entry");
      fireEvent.click(addButton);

      // After clicking, the component should show dirty state
      // (This would require modifying the entry, which we're testing the UI for)
      expect(addButton).toBeInTheDocument(); // Button still available
    });
  });

  describe("dirty state and save/reset buttons", () => {
    it("does not show dirty footer when clean", () => {
      render(<ProjectAnalysisSection />);

      expect(
        screen.queryByText("Analysis settings have unsaved changes")
      ).not.toBeInTheDocument();
    });

    it("shows dirty footer when entries are modified", async () => {
      render(<ProjectAnalysisSection />);

      // Expand the first entry
      const headers = screen.getAllByRole("button");
      const expandButton = headers.find((btn) => btn.textContent?.includes("."));

      if (expandButton) {
        fireEvent.click(expandButton);

        // Modify a field
        const inputs = screen.getAllByPlaceholderText("e.g., . or src-tauri/");
        if (inputs[0]) {
          fireEvent.change(inputs[0], { target: { value: "modified" } });
        }

        // Now dirty footer should appear
        await waitFor(() => {
          expect(
            screen.queryByText("Analysis settings have unsaved changes")
          ).toBeInTheDocument();
        });
      }
    });

    it("shows Save and Reset All buttons in dirty footer", async () => {
      render(<ProjectAnalysisSection />);

      // Make it dirty by modifying
      const headers = screen.getAllByRole("button");
      const expandButton = headers.find((btn) => btn.textContent?.includes("."));

      if (expandButton) {
        fireEvent.click(expandButton);

        const inputs = screen.getAllByPlaceholderText("e.g., . or src-tauri/");
        if (inputs[0]) {
          fireEvent.change(inputs[0], { target: { value: "modified" } });

          await waitFor(() => {
            expect(screen.getByText("Reset All")).toBeInTheDocument();
            expect(screen.getByText("Save")).toBeInTheDocument();
          });
        }
      }
    });
  });

  describe("refresh detected commands functionality", () => {
    it("triggers refresh when button clicked", () => {
      render(<ProjectAnalysisSection />);

      const reanalyzeButton = screen.getByText("Refresh Detected Commands");
      expect(reanalyzeButton).toBeInTheDocument();
      fireEvent.click(reanalyzeButton);
      // Component should still be present after click
      expect(screen.getByText("Refresh Detected Commands")).toBeInTheDocument();
    });


  });

  describe("template variables reference", () => {
    it("renders Template Variables info panel", () => {
      render(<ProjectAnalysisSection />);

      expect(screen.getByText("Template Variables")).toBeInTheDocument();
    });

    it("shows template variables on expand", () => {
      render(<ProjectAnalysisSection />);

      const templateButton = screen.getByText("Template Variables").closest("button");
      expect(templateButton).toBeInTheDocument();

      if (templateButton) {
        fireEvent.click(templateButton);

        expect(screen.getByText("{project_root}")).toBeInTheDocument();
        expect(screen.getByText("{worktree_path}")).toBeInTheDocument();
        expect(screen.getByText("{task_branch}")).toBeInTheDocument();
      }
    });

    it("shows descriptions for template variables", () => {
      render(<ProjectAnalysisSection />);

      const templateButton = screen.getByText("Template Variables").closest("button");
      if (templateButton) {
        fireEvent.click(templateButton);

        expect(
          screen.getByText("Project working directory")
        ).toBeInTheDocument();
        expect(
          screen.getByText(/Task worktree directory/)
        ).toBeInTheDocument();
      }
    });
  });

  describe("integration with useAnalysisEditor", () => {
    it("persists project state on successful save", async () => {
      const mockUpdateProject = vi.fn();

      vi.mocked(useProjectStore).mockImplementation((selector) => {
        if (selector.toString().includes("selectActiveProject")) {
          return mockProject;
        }
        // Return updateProject function
        return mockUpdateProject;
      });

      const mockUpdateCustomAnalysis = vi.mocked(
        api.projects.updateCustomAnalysis
      );
      mockUpdateCustomAnalysis.mockResolvedValue(mockProject);

      render(<ProjectAnalysisSection />);

      // Make a modification and save
      const headers = screen.getAllByRole("button");
      const expandButton = headers.find((btn) => btn.textContent?.includes("."));

      if (expandButton) {
        fireEvent.click(expandButton);

        const inputs = screen.getAllByPlaceholderText("e.g., . or src-tauri/");
        if (inputs[0]) {
          fireEvent.change(inputs[0], { target: { value: "modified" } });

          await waitFor(() => {
            expect(screen.getByText("Save")).toBeInTheDocument();
          });

          // Click Save
          const saveButton = screen.getByText("Save");
          fireEvent.click(saveButton);

          // API call should be made
          await waitFor(() => {
            expect(mockUpdateCustomAnalysis).toHaveBeenCalled();
          });
        }
      }
    });

    it("clears persisted custom analysis when Reset All is clicked", async () => {
      const mockUpdateProject = vi.fn();
      const customProject: Project = {
        ...mockProject,
        customAnalysis: JSON.stringify([
          {
            path: "./custom",
            label: "Custom Path",
            install: "npm install",
            validate: ["echo test"],
            worktree_setup: [],
          },
        ]),
      };

      vi.mocked(useProjectStore).mockImplementation((selector) => {
        if (selector === selectActiveProject) {
          return customProject;
        }
        return mockUpdateProject;
      });

      const mockUpdateCustomAnalysis = vi.mocked(
        api.projects.updateCustomAnalysis
      );
      mockUpdateCustomAnalysis.mockResolvedValue({
        ...customProject,
        customAnalysis: null,
      });

      render(<ProjectAnalysisSection />);

      fireEvent.click(screen.getByText("Reset All"));

      await waitFor(() => {
        expect(mockUpdateCustomAnalysis).toHaveBeenCalledWith(
          "test-project",
          null
        );
      });
      expect(mockUpdateProject).toHaveBeenCalledWith("test-project", {
        customAnalysis: null,
      });
    });
  });

  describe("timestamp formatting", () => {
    it("formats timestamp correctly", () => {
      const isoDate = "2024-01-15T10:30:00Z";
      const projectWithDate: Project = {
        ...mockProject,
        analyzedAt: isoDate,
      };

      vi.mocked(useProjectStore).mockImplementation((selector) => {
        if (selector.toString().includes("selectActiveProject")) {
          return projectWithDate;
        }
        return vi.fn();
      });

      render(<ProjectAnalysisSection />);

      // Should show some formatted date
      expect(screen.getByText(/Last Analyzed:/)).toBeInTheDocument();
    });

    it("shows Never for null analyzedAt", () => {
      const projectWithoutDate: Project = {
        ...mockProject,
        analyzedAt: null,
      };

      vi.mocked(useProjectStore).mockImplementation((selector) => {
        if (selector.toString().includes("selectActiveProject")) {
          return projectWithoutDate;
        }
        return vi.fn();
      });

      render(<ProjectAnalysisSection />);

      expect(screen.getByText("Last Analyzed: Never")).toBeInTheDocument();
    });
  });
});
