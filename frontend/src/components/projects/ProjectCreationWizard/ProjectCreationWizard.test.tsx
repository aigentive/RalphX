/**
 * Tests for ProjectCreationWizard component
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProjectCreationWizard } from "./ProjectCreationWizard";

// ============================================================================
// Test Setup
// ============================================================================

const defaultProps = {
  isOpen: true,
  onClose: vi.fn(),
  onCreate: vi.fn(),
};

function renderWizard(props = {}) {
  return render(<ProjectCreationWizard {...defaultProps} {...props} />);
}

describe("ProjectCreationWizard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // ==========================================================================
  // Rendering Tests
  // ==========================================================================

  describe("rendering", () => {
    it("renders nothing when isOpen is false", () => {
      renderWizard({ isOpen: false });
      expect(screen.queryByTestId("project-creation-wizard")).not.toBeInTheDocument();
    });

    it("renders modal when isOpen is true", () => {
      renderWizard();
      expect(screen.getByTestId("project-creation-wizard")).toBeInTheDocument();
    });

    it("renders with correct title", () => {
      renderWizard();
      expect(screen.getByText("Create New Project")).toBeInTheDocument();
    });

    it("renders project name input", () => {
      renderWizard();
      expect(screen.getByTestId("project-name-input")).toBeInTheDocument();
    });

    it("renders folder input", () => {
      renderWizard();
      expect(screen.getByTestId("folder-input")).toBeInTheDocument();
    });

    it("renders Browse button when onBrowseFolder is provided", () => {
      renderWizard({ onBrowseFolder: vi.fn() });
      expect(screen.getByTestId("browse-button")).toBeInTheDocument();
    });

    it("does not render Browse button when onBrowseFolder is not provided", () => {
      renderWizard();
      expect(screen.queryByTestId("browse-button")).not.toBeInTheDocument();
    });

    it("renders git settings with base branch", () => {
      renderWizard();
      expect(screen.getByText("Git Settings")).toBeInTheDocument();
      expect(screen.getByTestId("base-branch-select")).toBeInTheDocument();
    });

    it("renders cancel and create buttons", () => {
      renderWizard();
      expect(screen.getByTestId("cancel-button")).toBeInTheDocument();
      expect(screen.getByTestId("create-button")).toBeInTheDocument();
    });

    it("renders close button (shadcn dialog has default close)", () => {
      renderWizard();
      // shadcn Dialog has a close button with sr-only "Close" text
      expect(screen.getByRole("button", { name: /close/i })).toBeInTheDocument();
    });
  });

  // ==========================================================================
  // Git Settings Tests
  // ==========================================================================

  describe("git settings", () => {
    it("shows base branch select and worktree path by default", () => {
      renderWizard();

      expect(screen.getByTestId("base-branch-select")).toBeInTheDocument();
      expect(screen.getByTestId("worktree-path-display")).toBeInTheDocument();
    });

    it("shows advanced settings trigger", () => {
      renderWizard();
      expect(screen.getByTestId("advanced-settings-trigger")).toBeInTheDocument();
    });
  });

  // ==========================================================================
  // Validation Tests
  // ==========================================================================

  describe("validation", () => {
    it("shows error when working directory is empty on submit", async () => {
      const user = userEvent.setup();
      renderWizard();

      // Try to submit without filling location
      await user.click(screen.getByTestId("create-button"));

      // Wait for error to appear after state update
      await waitFor(() => {
        expect(screen.getByTestId("folder-input-error")).toBeInTheDocument();
      });
      expect(screen.getByText("Location is required")).toBeInTheDocument();
    });

    it("does not call onCreate when form has errors", async () => {
      const user = userEvent.setup();
      const mockOnCreate = vi.fn();
      renderWizard({ onCreate: mockOnCreate });

      // Try to submit empty form
      await user.click(screen.getByTestId("create-button"));

      expect(mockOnCreate).not.toHaveBeenCalled();
    });
  });

  // ==========================================================================
  // Submission Tests
  // ==========================================================================

  describe("submission", () => {
    it("calls onCreate with worktree mode project data", async () => {
      const user = userEvent.setup();
      const mockOnCreate = vi.fn();
      const mockBrowse = vi.fn().mockResolvedValue("/Users/dev/my-app");
      renderWizard({ onCreate: mockOnCreate, onBrowseFolder: mockBrowse });

      // Browse for folder (auto-fills project name from folder)
      await user.click(screen.getByTestId("browse-button"));
      await waitFor(() => {
        expect(screen.getByTestId("folder-input")).toHaveValue("/Users/dev/my-app");
      });

      // Submit
      await user.click(screen.getByTestId("create-button"));

      expect(mockOnCreate).toHaveBeenCalledWith({
        name: "my-app", // auto-inferred from folder
        workingDirectory: "/Users/dev/my-app",
        gitMode: "worktree",
        baseBranch: "main",
        worktreeParentDirectory: "~/ralphx-worktrees",
      });
    });

    it("calls onCreate with custom project name", async () => {
      const user = userEvent.setup();
      const mockOnCreate = vi.fn();
      const mockBrowse = vi.fn().mockResolvedValue("/Users/dev/my-app");
      renderWizard({ onCreate: mockOnCreate, onBrowseFolder: mockBrowse });

      // Browse for folder
      await user.click(screen.getByTestId("browse-button"));
      await waitFor(() => {
        expect(screen.getByTestId("folder-input")).toHaveValue("/Users/dev/my-app");
      });

      // Override with custom name
      await user.clear(screen.getByTestId("project-name-input"));
      await user.type(screen.getByTestId("project-name-input"), "Custom Project Name");

      // Submit
      await user.click(screen.getByTestId("create-button"));

      expect(mockOnCreate).toHaveBeenCalledWith({
        name: "Custom Project Name",
        workingDirectory: "/Users/dev/my-app",
        gitMode: "worktree",
        baseBranch: "main",
        worktreeParentDirectory: "~/ralphx-worktrees",
      });
    });

    it("trims whitespace from form values", async () => {
      const user = userEvent.setup();
      const mockOnCreate = vi.fn();
      const mockBrowse = vi.fn().mockResolvedValue("  /Users/dev/my-app  ");
      renderWizard({ onCreate: mockOnCreate, onBrowseFolder: mockBrowse });

      // Browse for folder
      await user.click(screen.getByTestId("browse-button"));
      await waitFor(() => {
        expect(screen.getByTestId("folder-input")).toHaveValue("  /Users/dev/my-app  ");
      });

      // Override with custom name with whitespace
      await user.clear(screen.getByTestId("project-name-input"));
      await user.type(screen.getByTestId("project-name-input"), "  My Project  ");

      // Submit
      await user.click(screen.getByTestId("create-button"));

      expect(mockOnCreate).toHaveBeenCalledWith({
        name: "My Project",
        workingDirectory: "/Users/dev/my-app",
        gitMode: "worktree",
        baseBranch: "main",
        worktreeParentDirectory: "~/ralphx-worktrees",
      });
    });

    it("shows creating state when isCreating is true", () => {
      renderWizard({ isCreating: true });

      expect(screen.getByText("Creating...")).toBeInTheDocument();
    });

    it("disables form inputs when isCreating is true", () => {
      renderWizard({ isCreating: true });

      expect(screen.getByTestId("project-name-input")).toBeDisabled();
      expect(screen.getByTestId("folder-input")).toBeDisabled();
      expect(screen.getByTestId("cancel-button")).toBeDisabled();
    });
  });

  // ==========================================================================
  // Browse Folder Tests
  // ==========================================================================

  describe("browse folder", () => {
    it("calls onBrowseFolder when Browse button is clicked", async () => {
      const user = userEvent.setup();
      const mockBrowse = vi.fn().mockResolvedValue(null);
      renderWizard({ onBrowseFolder: mockBrowse });

      await user.click(screen.getByTestId("browse-button"));

      expect(mockBrowse).toHaveBeenCalled();
    });

    it("updates working directory when folder is selected", async () => {
      const user = userEvent.setup();
      const mockBrowse = vi.fn().mockResolvedValue("/Users/selected/path");
      renderWizard({ onBrowseFolder: mockBrowse });

      await user.click(screen.getByTestId("browse-button"));

      await waitFor(() => {
        expect(screen.getByTestId("folder-input")).toHaveValue("/Users/selected/path");
      });
    });

    it("auto-infers project name from selected folder", async () => {
      const user = userEvent.setup();
      const mockBrowse = vi.fn().mockResolvedValue("/Users/selected/my-awesome-app");
      renderWizard({ onBrowseFolder: mockBrowse });

      await user.click(screen.getByTestId("browse-button"));

      await waitFor(() => {
        expect(screen.getByTestId("project-name-input")).toHaveValue("my-awesome-app");
      });
    });

    it("does not update working directory when folder selection is cancelled", async () => {
      const user = userEvent.setup();
      const mockBrowse = vi.fn().mockResolvedValue(null);
      renderWizard({ onBrowseFolder: mockBrowse });

      await user.click(screen.getByTestId("browse-button"));

      expect(screen.getByTestId("folder-input")).toHaveValue("");
    });
  });

  // ==========================================================================
  // Close/Cancel Tests
  // ==========================================================================

  describe("close and cancel", () => {
    it("calls onClose when cancel button is clicked", async () => {
      const user = userEvent.setup();
      const mockOnClose = vi.fn();
      renderWizard({ onClose: mockOnClose });

      await user.click(screen.getByTestId("cancel-button"));

      expect(mockOnClose).toHaveBeenCalled();
    });

    it("calls onClose when close button is clicked", async () => {
      const user = userEvent.setup();
      const mockOnClose = vi.fn();
      renderWizard({ onClose: mockOnClose });

      // shadcn Dialog close button has sr-only "Close" text
      await user.click(screen.getByRole("button", { name: /close/i }));

      expect(mockOnClose).toHaveBeenCalled();
    });
  });

  // ==========================================================================
  // First-Run Mode Tests
  // ==========================================================================

  describe("first-run mode", () => {
    it("hides cancel button in first-run mode", () => {
      renderWizard({ isFirstRun: true });
      expect(screen.queryByTestId("cancel-button")).not.toBeInTheDocument();
    });

    it("hides close button in first-run mode", () => {
      renderWizard({ isFirstRun: true });
      // The close button should be hidden
      expect(screen.queryByRole("button", { name: /close/i })).not.toBeInTheDocument();
    });
  });

  // ==========================================================================
  // Error Display Tests
  // ==========================================================================

  describe("error display", () => {
    it("displays error message when provided", () => {
      renderWizard({ error: "Failed to create project" });

      expect(screen.getByTestId("wizard-error")).toBeInTheDocument();
      expect(screen.getByText("Failed to create project")).toBeInTheDocument();
    });

    it("does not display error message when not provided", () => {
      renderWizard();

      expect(screen.queryByTestId("wizard-error")).not.toBeInTheDocument();
    });

    it("does not display error message when null", () => {
      renderWizard({ error: null });

      expect(screen.queryByTestId("wizard-error")).not.toBeInTheDocument();
    });
  });

  // ==========================================================================
  // Form Reset Tests
  // ==========================================================================

  describe("form reset", () => {
    it("resets form when modal reopens", async () => {
      const user = userEvent.setup();
      const mockBrowse = vi.fn().mockResolvedValue("/Users/dev/my-app");
      const { rerender } = renderWizard({ onBrowseFolder: mockBrowse });

      // Browse for folder
      await user.click(screen.getByTestId("browse-button"));
      await waitFor(() => {
        expect(screen.getByTestId("folder-input")).toHaveValue("/Users/dev/my-app");
      });

      // Close and reopen
      rerender(
        <ProjectCreationWizard {...defaultProps} isOpen={false} onBrowseFolder={mockBrowse} />
      );
      rerender(
        <ProjectCreationWizard {...defaultProps} isOpen={true} onBrowseFolder={mockBrowse} />
      );

      // Check form is reset
      expect(screen.getByTestId("project-name-input")).toHaveValue("");
      expect(screen.getByTestId("folder-input")).toHaveValue("");
    });

    it("resets base branch when modal reopens", async () => {
      const { rerender } = renderWizard();

      // Base branch should be "main" by default
      expect(screen.getByTestId("base-branch-select")).toBeInTheDocument();

      // Close and reopen
      rerender(
        <ProjectCreationWizard {...defaultProps} isOpen={false} />
      );
      rerender(
        <ProjectCreationWizard {...defaultProps} isOpen={true} />
      );

      // Check git settings are still present
      expect(screen.getByTestId("base-branch-select")).toBeInTheDocument();
    });
  });
});
