/**
 * SettingsView component tests
 *
 * Tests for the premium Settings View implementation with:
 * - Glass effect header with Settings icon
 * - Section cards with gradient borders
 * - shadcn Switch, Input, Select components
 * - Master toggle → sub-settings disabled pattern
 * - Lucide icons throughout
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render as rtlRender, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { SettingsView } from "./SettingsView";
import { DEFAULT_PROJECT_SETTINGS } from "@/types/settings";

const mockSubscribe = vi.fn(() => vi.fn());

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: mockSubscribe,
  }),
}));

vi.mock("@/api/execution", () => ({
  executionApi: {
    getGlobalSettings: vi.fn().mockResolvedValue({
      globalMaxConcurrent: 20,
      workspaceMaxConcurrent: 10,
      globalIdeationMax: 10,
      allowIdeationBorrowIdleExecution: false,
    }),
    updateGlobalSettings: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock("./sections/WorkspaceReviewSection", () => ({
  default: () => <div data-testid="workspace-review-section">Workspace Review</div>,
}));

const createTestQueryClient = () =>
  new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  });

const render = (ui: Parameters<typeof rtlRender>[0]) =>
  rtlRender(
    <QueryClientProvider client={createTestQueryClient()}>{ui}</QueryClientProvider>
  );

describe("SettingsView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("Rendering", () => {
    it("renders with default settings", () => {
      render(<SettingsView />);
      expect(screen.getByTestId("settings-view")).toBeInTheDocument();
    });

    it("renders loading skeleton when isLoading is true", () => {
      render(<SettingsView isLoading={true} />);
      expect(screen.getByTestId("settings-skeleton")).toBeInTheDocument();
    });

    it("renders error message when error is provided", () => {
      const errorMessage = "Failed to save settings";
      render(<SettingsView error={errorMessage} />);
      expect(screen.getByText(errorMessage)).toBeInTheDocument();
    });

    it("renders core sections", () => {
      render(<SettingsView />);
      // Sections carry no headers of their own; identify them by their rows.
      expect(screen.getByText("Max Concurrent Tasks")).toBeInTheDocument();
      expect(screen.getByText("Project Ideation Cap")).toBeInTheDocument();
    });
  });

  describe("Execution Section", () => {
    it("renders execution settings", () => {
      render(<SettingsView />);
      expect(screen.getByText("Max Concurrent Tasks")).toBeInTheDocument();
      expect(screen.getByText("Project Ideation Cap")).toBeInTheDocument();
      expect(screen.getByText("Default Autofix CI & Reviews")).toBeInTheDocument();
      expect(screen.getByText("Default GitHub auto-merge")).toBeInTheDocument();
      expect(
        screen.getByText(
          "RalphX monitors this PR for failing checks and review feedback, then publishes follow-up fixes from the workspace automatically."
        )
      ).toBeInTheDocument();
      expect(
        screen.getByText(
          "RalphX asks GitHub to merge the PR after required checks and review requirements pass."
        )
      ).toBeInTheDocument();
    });

    it("displays default max concurrent tasks value", () => {
      render(<SettingsView />);
      const input = screen.getByTestId("max-concurrent-tasks");
      expect(input).toHaveValue(DEFAULT_PROJECT_SETTINGS.execution.max_concurrent_tasks);
    });

    it("displays default project ideation cap value", () => {
      render(<SettingsView />);
      const input = screen.getByTestId("project-ideation-max");
      expect(input).toHaveValue(DEFAULT_PROJECT_SETTINGS.execution.project_ideation_max);
    });

    it("updates max concurrent tasks", () => {
      const onChange = vi.fn();
      render(<SettingsView onSettingsChange={onChange} />);

      const input = screen.getByTestId("max-concurrent-tasks");
      fireEvent.change(input, { target: { value: "5" } });

      expect(onChange).toHaveBeenCalledTimes(1);
      const calledWith = onChange.mock.calls[0][0];
      expect(calledWith.execution.max_concurrent_tasks).toBe(5);
    });

    it("updates project ideation cap", () => {
      const onChange = vi.fn();
      render(<SettingsView onSettingsChange={onChange} />);

      const input = screen.getByTestId("project-ideation-max");
      fireEvent.change(input, { target: { value: "4" } });

      expect(onChange).toHaveBeenCalledTimes(1);
      const calledWith = onChange.mock.calls[0][0];
      expect(calledWith.execution.project_ideation_max).toBe(4);
    });

    it("updates agent workspace PR automation defaults", async () => {
      const user = userEvent.setup();
      const onChange = vi.fn();
      render(<SettingsView onSettingsChange={onChange} />);

      await user.click(screen.getByTestId("agent-workspace-pr-autofix-default"));
      await user.click(screen.getByTestId("agent-workspace-pr-auto-merge-default"));

      expect(onChange).toHaveBeenCalledTimes(2);
      expect(
        onChange.mock.calls[0][0].execution.agent_workspace_pr_autofix_default
      ).toBe(true);
      expect(
        onChange.mock.calls[1][0].execution.agent_workspace_pr_auto_merge_default
      ).toBe(true);
    });
  });

  describe("Review Policy Section", () => {
    it("renders review policy section placeholder when settings not loaded", () => {
      // ReviewPolicySection fetches its own data — renders null when invoke returns undefined
      render(<SettingsView />);
      // Section is self-contained; null render is expected when hook data is loading/absent
      expect(screen.queryByTestId("require-human-review")).not.toBeInTheDocument();
    });
  });

  describe("Initial Settings", () => {
    it("uses provided initial settings", () => {
      const customSettings = {
        ...DEFAULT_PROJECT_SETTINGS,
        execution: {
          ...DEFAULT_PROJECT_SETTINGS.execution,
          max_concurrent_tasks: 5,
        },
      };

      render(<SettingsView initialSettings={customSettings} />);

      expect(screen.getByTestId("max-concurrent-tasks")).toHaveValue(5);
    });

    it("updates settings when initialSettings prop changes", () => {
      const initialSettings = {
        ...DEFAULT_PROJECT_SETTINGS,
        execution: {
          ...DEFAULT_PROJECT_SETTINGS.execution,
          max_concurrent_tasks: 3,
        },
      };

      const { rerender } = render(<SettingsView initialSettings={initialSettings} />);
      expect(screen.getByTestId("max-concurrent-tasks")).toHaveValue(3);

      // Simulate project switch - new settings with different max_concurrent_tasks
      const newSettings = {
        ...DEFAULT_PROJECT_SETTINGS,
        execution: {
          ...DEFAULT_PROJECT_SETTINGS.execution,
          max_concurrent_tasks: 7,
        },
      };

      rerender(
        <QueryClientProvider client={createTestQueryClient()}>
          <SettingsView initialSettings={newSettings} />
        </QueryClientProvider>
      );

      // Verify the input now shows the new project's value
      expect(screen.getByTestId("max-concurrent-tasks")).toHaveValue(7);
    });
  });

  describe("Disabled State", () => {
    it("disables execution inputs when isSaving is true", () => {
      render(<SettingsView isSaving={true} />);

      // Check number inputs are disabled
      expect(screen.getByTestId("max-concurrent-tasks")).toBeDisabled();
    });
  });

  describe("Accessibility", () => {
    it("associates descriptions with number inputs", () => {
      render(<SettingsView />);

      const input = screen.getByTestId("max-concurrent-tasks");
      const descId = input.getAttribute("aria-describedby");
      expect(descId).toBeTruthy();

      const description = document.getElementById(descId!);
      expect(description).toBeInTheDocument();
    });
  });

  describe("Error Banner", () => {
    it("can dismiss an error with the accessible dismiss button", async () => {
      const user = userEvent.setup();
      const errorMessage = "Failed to save settings";
      render(<SettingsView error={errorMessage} />);

      expect(screen.getByText(errorMessage)).toBeInTheDocument();

      const dismissButton = screen.getByRole("button", {
        name: "Dismiss error",
      });
      await user.click(dismissButton);

      expect(screen.queryByText(errorMessage)).not.toBeInTheDocument();
    });
  });

  describe("Global Capacity Section", () => {
    it("renders all global capacity settings", async () => {
      render(<SettingsView />);

      expect(await screen.findByText("Global Max Concurrent")).toBeInTheDocument();
      expect(screen.getByText("Global Ideation Cap")).toBeInTheDocument();
      expect(screen.getByText("Allow Ideation Borrowing")).toBeInTheDocument();
    });

    it("displays default global capacity values", async () => {
      render(<SettingsView />);

      expect(await screen.findByTestId("global-max-concurrent")).toHaveValue(20);
      expect(screen.getByTestId("global-ideation-max")).toHaveValue(10);
      expect(screen.getByTestId("allow-ideation-borrow-idle-execution")).toHaveAttribute(
        "data-state",
        "unchecked"
      );
    });
  });
});
