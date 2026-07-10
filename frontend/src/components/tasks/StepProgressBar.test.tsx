/**
 * Tests for StepProgressBar component
 * Verifies progress dots rendering, status colors, and compact/full modes
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StepProgressBar } from "./StepProgressBar";
import { stepKeys } from "@/hooks/useTaskSteps";
import { api } from "@/lib/tauri";
import type { StepProgressSummary } from "@/types/task-step";

// Mock the API
vi.mock("@/lib/tauri", () => ({
  api: {
    steps: {
      getProgress: vi.fn(),
    },
  },
}));

const mockApi = api as {
  steps: {
    getProgress: ReturnType<typeof vi.fn>;
  };
};

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

function createWrapperWithProgress(taskId: string, progress: StepProgressSummary) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });
  queryClient.setQueryData(stepKeys.progress(taskId), progress);
  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

const mockProgress: StepProgressSummary = {
  taskId: "task-1",
  total: 7,
  completed: 2,
  inProgress: 1,
  pending: 3,
  skipped: 1,
  failed: 0,
  currentStep: {
    id: "step-3",
    taskId: "task-1",
    title: "Add OAuth providers",
    description: null,
    status: "in_progress",
    sortOrder: 2,
    dependsOn: null,
    createdBy: "proposal",
    completionNote: null,
    createdAt: "2026-01-26T00:00:00Z",
    updatedAt: "2026-01-26T00:00:00Z",
    startedAt: "2026-01-26T00:10:00Z",
    completedAt: null,
  },
  nextStep: {
    id: "step-4",
    taskId: "task-1",
    title: "Add session management",
    description: null,
    status: "pending",
    sortOrder: 3,
    dependsOn: null,
    createdBy: "proposal",
    completionNote: null,
    createdAt: "2026-01-26T00:00:00Z",
    updatedAt: "2026-01-26T00:00:00Z",
    startedAt: null,
    completedAt: null,
  },
  percentComplete: 42.86,
};

describe("StepProgressBar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("Loading State", () => {
    it("returns null while loading", () => {
      mockApi.steps.getProgress.mockReturnValue(
        new Promise(() => {}) // Never resolves
      );

      const { container } = render(<StepProgressBar taskId="task-1" />, {
        wrapper: createWrapper(),
      });

      expect(container.firstChild).toBeNull();
    });
  });

  describe("Empty State", () => {
    it("returns null when no data", async () => {
      mockApi.steps.getProgress.mockResolvedValue(null);

      const { container } = render(<StepProgressBar taskId="task-1" />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        expect(container.firstChild).toBeNull();
      });
    });

    it("returns null when total is 0", async () => {
      mockApi.steps.getProgress.mockResolvedValue({
        ...mockProgress,
        total: 0,
      });

      const { container } = render(<StepProgressBar taskId="task-1" />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        expect(container.firstChild).toBeNull();
      });
    });

    it("returns null when every step is skipped", async () => {
      const allSkipped = {
        ...mockProgress,
        total: 2,
        completed: 0,
        inProgress: 0,
        pending: 0,
        skipped: 2,
        failed: 0,
      };

      const { container } = render(<StepProgressBar taskId="task-1" />, {
        wrapper: createWrapperWithProgress("task-1", allSkipped),
      });

      expect(container.firstChild).toBeNull();
    });
  });

  describe("Progress Dots", () => {
    it("renders correct number of dots", async () => {
      mockApi.steps.getProgress.mockResolvedValue(mockProgress);

      const { container } = render(<StepProgressBar taskId="task-1" />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");
        expect(dots).toHaveLength(7);
      });
    });

    it("applies correct color classes for step statuses", async () => {
      mockApi.steps.getProgress.mockResolvedValue(mockProgress);

      const { container } = render(<StepProgressBar taskId="task-1" />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");

        // First 2 completed (green)
        expect(dots[0].className).toContain("bg-status-success");
        expect(dots[1].className).toContain("bg-status-success");

        // Next 1 skipped (muted)
        expect(dots[2].className).toContain("bg-text-muted");

        // Next 1 in_progress (accent with pulse)
        expect(dots[3].className).toContain("bg-accent-primary");
        expect(dots[3].className).toContain("animate-pulse");

        // Remaining 3 pending (border color)
        expect(dots[4].className).toContain("bg-border-default");
        expect(dots[5].className).toContain("bg-border-default");
        expect(dots[6].className).toContain("bg-border-default");
      });
    });

    it("handles failed steps correctly", async () => {
      const progressWithFailed: StepProgressSummary = {
        ...mockProgress,
        completed: 2,
        skipped: 0,
        failed: 1,
        inProgress: 1,
        pending: 3,
      };

      mockApi.steps.getProgress.mockResolvedValue(progressWithFailed);

      const { container } = render(<StepProgressBar taskId="task-1" />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");

        // First 2 completed
        expect(dots[0].className).toContain("bg-status-success");
        expect(dots[1].className).toContain("bg-status-success");

        // Next 1 failed (error red)
        expect(dots[2].className).toContain("bg-status-error");

        // Next 1 in_progress
        expect(dots[3].className).toContain("bg-accent-primary");
      });
    });
  });

  describe("Compact Mode", () => {
    it("does not show text in compact mode", async () => {
      mockApi.steps.getProgress.mockResolvedValue(mockProgress);

      const { container } = render(<StepProgressBar taskId="task-1" compact />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");
        expect(dots).toHaveLength(7);
      });

      // Should not contain text summary
      expect(screen.queryByText("2/6")).not.toBeInTheDocument();
    });

    it("uses completable steps for compact percent", async () => {
      mockApi.steps.getProgress.mockResolvedValue(mockProgress);

      render(<StepProgressBar taskId="task-1" compact />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        expect(screen.getByText("33%")).toBeInTheDocument();
      });
    });

    it("caps visible dots and shows +X more for large step counts", async () => {
      const manySteps: StepProgressSummary = {
        ...mockProgress,
        total: 28,
        completed: 10,
        skipped: 2,
        failed: 1,
        inProgress: 1,
        pending: 14,
      };
      mockApi.steps.getProgress.mockResolvedValue(manySteps);

      const { container } = render(<StepProgressBar taskId="task-1" compact />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");
        expect(dots).toHaveLength(19);
      });

      expect(screen.getByText("+9 more")).toBeInTheDocument();
    });
  });

  describe("Full Mode", () => {
    it("shows text summary in full mode", async () => {
      mockApi.steps.getProgress.mockResolvedValue(mockProgress);

      render(<StepProgressBar taskId="task-1" compact={false} />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        expect(screen.getByText("2/6")).toBeInTheDocument();
      });
    });

    it("excludes skipped steps from the total count", async () => {
      const progress: StepProgressSummary = {
        ...mockProgress,
        total: 10,
        completed: 4,
        skipped: 2,
        failed: 0,
        inProgress: 1,
        pending: 3,
      };

      mockApi.steps.getProgress.mockResolvedValue(progress);

      render(<StepProgressBar taskId="task-1" />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        expect(screen.getByText("4/8")).toBeInTheDocument();
      });
    });
  });

  describe("Terminal State Indicator", () => {
    it("shows completed dots as green by default (backward compatibility)", async () => {
      mockApi.steps.getProgress.mockResolvedValue(mockProgress);

      const { container } = render(<StepProgressBar taskId="task-1" />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");
        // First 2 dots are completed, should be green (backward compatible)
        expect(dots[0].className).toContain("bg-status-success");
        expect(dots[1].className).toContain("bg-status-success");
      });
    });

    it("shows completed dots as muted gray when internalStatus is executing", async () => {
      mockApi.steps.getProgress.mockResolvedValue(mockProgress);

      const { container } = render(
        <StepProgressBar taskId="task-1" internalStatus="executing" />,
        {
          wrapper: createWrapper(),
        }
      );

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");
        // First 2 dots are completed, should be muted (non-terminal state)
        expect(dots[0].className).toContain("bg-text-muted");
        expect(dots[1].className).toContain("bg-text-muted");
      });
    });

    it("shows completed dots as muted gray when internalStatus is reviewing", async () => {
      mockApi.steps.getProgress.mockResolvedValue(mockProgress);

      const { container } = render(
        <StepProgressBar taskId="task-1" internalStatus="reviewing" />,
        {
          wrapper: createWrapper(),
        }
      );

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");
        // First 2 dots are completed, should be muted (non-terminal state)
        expect(dots[0].className).toContain("bg-text-muted");
        expect(dots[1].className).toContain("bg-text-muted");
      });
    });

    it("shows completed dots as green when internalStatus is merged", async () => {
      mockApi.steps.getProgress.mockResolvedValue(mockProgress);

      const { container } = render(
        <StepProgressBar taskId="task-1" internalStatus="merged" />,
        {
          wrapper: createWrapper(),
        }
      );

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");
        // First 2 dots are completed, should be green (terminal state)
        expect(dots[0].className).toContain("bg-status-success");
        expect(dots[1].className).toContain("bg-status-success");
      });
    });

    it("shows completed dots as green when internalStatus is approved", async () => {
      mockApi.steps.getProgress.mockResolvedValue(mockProgress);

      const { container } = render(
        <StepProgressBar taskId="task-1" internalStatus="approved" />,
        {
          wrapper: createWrapper(),
        }
      );

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");
        // First 2 dots are completed, should be green (terminal state)
        expect(dots[0].className).toContain("bg-status-success");
        expect(dots[1].className).toContain("bg-status-success");
      });
    });
  });

  describe("Integration", () => {
    it("works with all completed steps", async () => {
      const allCompleted: StepProgressSummary = {
        ...mockProgress,
        total: 5,
        completed: 5,
        inProgress: 0,
        pending: 0,
        skipped: 0,
        failed: 0,
        percentComplete: 100,
      };

      mockApi.steps.getProgress.mockResolvedValue(allCompleted);

      const { container } = render(<StepProgressBar taskId="task-1" />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");
        expect(dots).toHaveLength(5);

        // All should be completed (green)
        dots.forEach((dot) => {
          expect(dot.className).toContain("bg-status-success");
        });
      });

      expect(screen.getByText("5/5")).toBeInTheDocument();
    });

    it("works with all pending steps", async () => {
      const allPending: StepProgressSummary = {
        ...mockProgress,
        total: 5,
        completed: 0,
        inProgress: 0,
        pending: 5,
        skipped: 0,
        failed: 0,
        percentComplete: 0,
      };

      mockApi.steps.getProgress.mockResolvedValue(allPending);

      const { container } = render(<StepProgressBar taskId="task-1" />, {
        wrapper: createWrapper(),
      });

      await vi.waitFor(() => {
        const dots = container.querySelectorAll(".h-1\\.5.w-1\\.5.rounded-full");
        expect(dots).toHaveLength(5);

        // All should be pending (border color)
        dots.forEach((dot) => {
          expect(dot.className).toContain("bg-border-default");
        });
      });

      expect(screen.getByText("0/5")).toBeInTheDocument();
    });
  });
});
