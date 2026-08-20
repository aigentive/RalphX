/**
 * Tests for the ProjectCreationWizard shell (Dialog + step router).
 * Per-step form/probe/submission behavior is covered by the co-located
 * steps/*.test.tsx files.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, act, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ProjectCreationWizard } from "./ProjectCreationWizard";

// CloneStep depends on useCloneJob (EventProvider context) and the GitHub
// auth hooks (react-query context). The shell test only exercises step
// routing/dismissal, so both are stubbed rather than wired with real providers.
// `start` resolves true (simulating a successful clone-job start) and never
// resolves job.status, which is exactly what a dismissal-guard test needs
// (the step stays in the "running" phase).
vi.mock("@/hooks/useCloneJob", () => ({
  useCloneJob: () => ({
    phase: null,
    percent: null,
    received: null,
    total: null,
    lines: [],
    status: null,
    error: null,
    cleanedUp: null,
    start: vi.fn().mockResolvedValue(true),
    cancel: vi.fn(),
    reset: vi.fn(),
  }),
}));

vi.mock("@/hooks/useGithubSettings", () => ({
  useGhAuthStatus: () => ({ data: false, isLoading: false }),
  useLoginGhWithBrowser: () => ({ mutate: vi.fn(), isPending: false }),
}));

vi.mock("@/api/projects", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/projects")>();
  return {
    ...actual,
    projectsApi: {
      ...actual.projectsApi,
      validateCloneTarget: vi.fn().mockResolvedValue({
        normalizedUrl: "https://github.com/o/r.git",
        folderName: "r",
        branch: null,
        suggestedSshUrl: null,
        destination: "/tmp/dest/r",
        ready: true,
        problem: null,
      }),
    },
  };
});

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

  afterEach(() => {
    vi.useRealTimers();
  });

  describe("rendering", () => {
    it("renders nothing when isOpen is false", () => {
      renderWizard({ isOpen: false });
      expect(screen.queryByTestId("project-creation-wizard")).not.toBeInTheDocument();
    });

    it("renders modal when isOpen is true", () => {
      renderWizard();
      expect(screen.getByTestId("project-creation-wizard")).toBeInTheDocument();
    });

    it("lands on the intent chooser by default", () => {
      renderWizard();
      expect(screen.getByTestId("intent-clone")).toBeInTheDocument();
      expect(screen.getByTestId("intent-create")).toBeInTheDocument();
      expect(screen.getByTestId("intent-existing")).toBeInTheDocument();
    });

    it("does not show the back button on the intent step", () => {
      renderWizard();
      expect(screen.queryByTestId("wizard-back-button")).not.toBeInTheDocument();
    });

    it("renders close button (shadcn dialog has default close)", () => {
      renderWizard();
      expect(screen.getByRole("button", { name: /close/i })).toBeInTheDocument();
    });
  });

  describe("intent navigation", () => {
    it("selecting Add Existing Repository shows the existing-repository step", async () => {
      const user = userEvent.setup();
      renderWizard();
      await user.click(screen.getByTestId("intent-existing"));
      expect(screen.getByTestId("folder-input")).toBeInTheDocument();
      expect(screen.getByTestId("wizard-back-button")).toBeInTheDocument();
    });

    it("selecting Create New Repository shows the new-repository step", async () => {
      const user = userEvent.setup();
      renderWizard();
      await user.click(screen.getByTestId("intent-create"));
      expect(screen.getByTestId("new-project-parent-input")).toBeInTheDocument();
      expect(screen.getByTestId("new-project-folder-name-input")).toBeInTheDocument();
    });

    it("selecting Clone Repository shows the clone step", async () => {
      const user = userEvent.setup();
      renderWizard();
      await user.click(screen.getByTestId("intent-clone"));
      expect(screen.getByTestId("clone-url-input")).toBeInTheDocument();
      expect(screen.getByTestId("wizard-back-button")).toBeInTheDocument();
    });

    it("the back button returns to the intent chooser", async () => {
      const user = userEvent.setup();
      renderWizard();
      await user.click(screen.getByTestId("intent-existing"));
      await user.click(screen.getByTestId("wizard-back-button"));
      expect(screen.getByTestId("intent-existing")).toBeInTheDocument();
      expect(screen.queryByTestId("folder-input")).not.toBeInTheDocument();
    });

    it("reopening the dialog resets to the intent chooser", () => {
      const { rerender } = renderWizard();
      rerender(<ProjectCreationWizard {...defaultProps} isOpen={false} />);
      rerender(<ProjectCreationWizard {...defaultProps} isOpen={true} />);
      expect(screen.getByTestId("intent-existing")).toBeInTheDocument();
    });
  });

  describe("close and cancel", () => {
    it("calls onClose when the dialog close button is clicked", async () => {
      const user = userEvent.setup();
      const onClose = vi.fn();
      renderWizard({ onClose });
      await user.click(screen.getByRole("button", { name: /close/i }));
      expect(onClose).toHaveBeenCalled();
    });

    it("calls onClose when cancel is clicked from a step", async () => {
      const user = userEvent.setup();
      const onClose = vi.fn();
      renderWizard({ onClose });
      await user.click(screen.getByTestId("intent-existing"));
      await user.click(screen.getByTestId("cancel-button"));
      expect(onClose).toHaveBeenCalled();
    });

    it("blocks Escape and disables Back while a clone is running", async () => {
      // Debounced validate_clone_target runs under fake timers; interactions
      // use raw DOM events wrapped in `act` (userEvent + fake timers hangs -
      // see CloneStep.test.tsx for the same convention).
      vi.useFakeTimers();
      const onClose = vi.fn();
      const onBrowseFolder = vi.fn().mockResolvedValue("/tmp/dest");
      renderWizard({ onClose, onBrowseFolder });

      await act(async () => {
        screen.getByTestId("intent-clone").click();
        await Promise.resolve();
      });
      await act(async () => {
        fireEvent.change(screen.getByTestId("clone-url-input"), {
          target: { value: "https://github.com/o/r.git" },
        });
      });
      await act(async () => {
        screen.getByTestId("browse-parent-button").click();
        await Promise.resolve();
        await Promise.resolve();
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(300);
      });
      expect(screen.getByText("/tmp/dest/r")).toBeInTheDocument();

      await act(async () => {
        screen.getByTestId("clone-start-button").click();
        await Promise.resolve();
      });
      expect(screen.getByTestId("clone-progress")).toBeInTheDocument();
      expect(screen.getByTestId("wizard-back-button")).toBeDisabled();

      await act(async () => {
        fireEvent.keyDown(document, { key: "Escape", code: "Escape" });
      });
      expect(onClose).not.toHaveBeenCalled();
    });
  });

  describe("first-run mode", () => {
    it("hides the close button in first-run mode", () => {
      renderWizard({ isFirstRun: true });
      expect(screen.queryByRole("button", { name: /close/i })).not.toBeInTheDocument();
    });

    it("still allows Back to the intent chooser in first-run mode", async () => {
      const user = userEvent.setup();
      renderWizard({ isFirstRun: true });
      await user.click(screen.getByTestId("intent-existing"));
      await user.click(screen.getByTestId("wizard-back-button"));
      expect(screen.getByTestId("intent-existing")).toBeInTheDocument();
    });

    it("does not show a cancel button on the existing-repository step in first-run mode", async () => {
      const user = userEvent.setup();
      renderWizard({ isFirstRun: true });
      await user.click(screen.getByTestId("intent-existing"));
      expect(screen.queryByTestId("cancel-button")).not.toBeInTheDocument();
    });
  });

  describe("error and creating state pass-through", () => {
    it("surfaces the error prop inside the active step", async () => {
      const user = userEvent.setup();
      renderWizard({ error: "Failed to create project" });
      await user.click(screen.getByTestId("intent-existing"));
      expect(screen.getByTestId("wizard-error")).toHaveTextContent("Failed to create project");
    });

    it("disables inputs on the active step while creating", async () => {
      const user = userEvent.setup();
      renderWizard({ isCreating: true });
      await user.click(screen.getByTestId("intent-existing"));
      expect(screen.getByTestId("folder-input")).toBeDisabled();
      expect(screen.getByTestId("create-button")).toBeDisabled();
    });
  });
});
