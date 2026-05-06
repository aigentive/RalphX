/**
 * PausedTasksPopover + PausedTaskCard tests
 *
 * Tests parsePauseReason (new format, legacy, user-initiated),
 * card rendering for both pause types, and resume button behavior.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import type { Task } from "@/types/task";
import { parsePauseReason, PausedTasksPopover } from "./PausedTasksPopover";
import { PausedTaskCard, type PauseReason } from "./PausedTaskCard";

// Mock status-icons to avoid dependency on design tokens
vi.mock("@/types/status-icons", () => ({
  getStatusIconConfig: () => ({ color: "#aaa", bgColor: "#111" }),
}));

const resumeMock = vi.hoisted(() => vi.fn());
const navigateToTaskMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: {
      resume: resumeMock,
    },
  },
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: (selector: (state: { navigateToTask: typeof navigateToTaskMock }) => unknown) =>
    selector({ navigateToTask: navigateToTaskMock }),
}));

// Force the popover to render its content inline so tests can assert on it
vi.mock("@/components/ui/popover", () => ({
  Popover: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  PopoverTrigger: ({ children }: { children: ReactNode }) => <>{children}</>,
  PopoverContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));

/** Factory for minimal Task objects */
function makeTask(overrides: Partial<Task> = {}): Task {
  return {
    id: "task-1",
    projectId: "proj-1",
    category: "feature",
    title: "Test Task",
    description: null,
    priority: 1,
    internalStatus: "paused",
    needsReviewPoint: false,
    createdAt: "2026-02-15T10:00:00Z",
    updatedAt: "2026-02-15T10:00:00Z",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    ...overrides,
  };
}

describe("parsePauseReason", () => {
  it("returns null for task with no metadata", () => {
    const task = makeTask({ metadata: null });
    expect(parsePauseReason(task)).toBeNull();
  });

  it("returns null for task with invalid JSON metadata", () => {
    const task = makeTask({ metadata: "not json" });
    expect(parsePauseReason(task)).toBeNull();
  });

  it("returns null for task with empty metadata object", () => {
    const task = makeTask({ metadata: JSON.stringify({}) });
    expect(parsePauseReason(task)).toBeNull();
  });

  it("parses new pause_reason format for provider_error", () => {
    const task = makeTask({
      metadata: JSON.stringify({
        pause_reason: {
          type: "provider_error",
          category: "rate_limit",
          message: "Usage limit reached",
          retry_after: "2026-02-15T14:00:00Z",
          previous_status: "executing",
          paused_at: "2026-02-15T12:00:00Z",
          auto_resumable: true,
          resume_attempts: 1,
        },
      }),
    });
    const result = parsePauseReason(task);
    expect(result).not.toBeNull();
    expect(result!.type).toBe("provider_error");
    if (result!.type === "provider_error") {
      expect(result!.category).toBe("rate_limit");
      expect(result!.message).toBe("Usage limit reached");
      expect(result!.auto_resumable).toBe(true);
      expect(result!.resume_attempts).toBe(1);
    }
  });

  it("parses new pause_reason format for user_initiated", () => {
    const task = makeTask({
      metadata: JSON.stringify({
        pause_reason: {
          type: "user_initiated",
          previous_status: "executing",
          paused_at: "2026-02-15T12:00:00Z",
          scope: "global",
        },
      }),
    });
    const result = parsePauseReason(task);
    expect(result).not.toBeNull();
    expect(result!.type).toBe("user_initiated");
    if (result!.type === "user_initiated") {
      expect(result!.previous_status).toBe("executing");
      expect(result!.scope).toBe("global");
    }
  });

  it("parses legacy provider_error format", () => {
    const task = makeTask({
      metadata: JSON.stringify({
        provider_error: {
          category: "server_error",
          message: "Internal server error",
          retry_after: null,
          previous_status: "executing",
          paused_at: "2026-02-15T12:00:00Z",
          auto_resumable: false,
          resume_attempts: 3,
        },
      }),
    });
    const result = parsePauseReason(task);
    expect(result).not.toBeNull();
    expect(result!.type).toBe("provider_error");
    if (result!.type === "provider_error") {
      expect(result!.category).toBe("server_error");
      expect(result!.auto_resumable).toBe(false);
      expect(result!.resume_attempts).toBe(3);
    }
  });

  it("prefers pause_reason over legacy provider_error", () => {
    const task = makeTask({
      metadata: JSON.stringify({
        pause_reason: {
          type: "user_initiated",
          previous_status: "executing",
          paused_at: "2026-02-15T12:00:00Z",
          scope: "global",
        },
        provider_error: {
          category: "rate_limit",
          message: "Should not be used",
        },
      }),
    });
    const result = parsePauseReason(task);
    expect(result!.type).toBe("user_initiated");
  });
});

describe("PausedTaskCard", () => {
  const onResume = vi.fn();
  const onViewDetails = vi.fn();

  beforeEach(() => {
    onResume.mockClear();
    onViewDetails.mockClear();
  });

  describe("provider_error type", () => {
    const providerErrorReason: PauseReason = {
      type: "provider_error",
      category: "rate_limit",
      message: "Usage limit reached for plan tier",
      retry_after: null,
      previous_status: "executing",
      paused_at: "2026-02-15T12:00:00Z",
      auto_resumable: false,
      resume_attempts: 2,
    };

    it("renders with data-testid", () => {
      const task = makeTask();
      render(
        <PausedTaskCard
          task={task}
          pauseReason={providerErrorReason}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      expect(screen.getByTestId("paused-task-card-task-1")).toBeInTheDocument();
    });

    it("shows task title", () => {
      const task = makeTask({ title: "My Important Task" });
      render(
        <PausedTaskCard
          task={task}
          pauseReason={providerErrorReason}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      expect(screen.getByText("My Important Task")).toBeInTheDocument();
    });

    it("shows category badge for rate_limit", () => {
      const task = makeTask();
      render(
        <PausedTaskCard
          task={task}
          pauseReason={providerErrorReason}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      expect(screen.getByText("Rate Limit")).toBeInTheDocument();
    });

    it("shows error message", () => {
      const task = makeTask();
      render(
        <PausedTaskCard
          task={task}
          pauseReason={providerErrorReason}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      expect(screen.getByText("Usage limit reached for plan tier")).toBeInTheDocument();
    });

    it("shows resume attempts count", () => {
      const task = makeTask();
      render(
        <PausedTaskCard
          task={task}
          pauseReason={providerErrorReason}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      expect(screen.getByText("2/5")).toBeInTheDocument();
    });

    it("shows Auto badge when auto_resumable", () => {
      const task = makeTask();
      render(
        <PausedTaskCard
          task={task}
          pauseReason={{ ...providerErrorReason, auto_resumable: true }}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      expect(screen.getByText("Auto")).toBeInTheDocument();
    });

    it("calls onResume with task id when resume button clicked", () => {
      const task = makeTask({ id: "task-42" });
      render(
        <PausedTaskCard
          task={task}
          pauseReason={providerErrorReason}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      fireEvent.click(screen.getByTestId("resume-button-task-42"));
      expect(onResume).toHaveBeenCalledWith("task-42");
    });

    it("calls onViewDetails with task id when view button clicked", () => {
      const task = makeTask({ id: "task-42" });
      render(
        <PausedTaskCard
          task={task}
          pauseReason={providerErrorReason}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      fireEvent.click(screen.getByTestId("view-details-button-task-42"));
      expect(onViewDetails).toHaveBeenCalledWith("task-42");
    });
  });

  describe("user_initiated type", () => {
    const userPauseReason: PauseReason = {
      type: "user_initiated",
      previous_status: "executing",
      paused_at: new Date().toISOString(),
      scope: "global",
    };

    it("renders with data-testid", () => {
      const task = makeTask({ id: "task-user-1" });
      render(
        <PausedTaskCard
          task={task}
          pauseReason={userPauseReason}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      expect(screen.getByTestId("paused-task-card-task-user-1")).toBeInTheDocument();
    });

    it("shows User Paused badge", () => {
      const task = makeTask();
      render(
        <PausedTaskCard
          task={task}
          pauseReason={userPauseReason}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      expect(screen.getByText("User Paused")).toBeInTheDocument();
    });

    it("shows 'Paused by user' label", () => {
      const task = makeTask();
      render(
        <PausedTaskCard
          task={task}
          pauseReason={userPauseReason}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      expect(screen.getByText("Paused by user")).toBeInTheDocument();
    });

    it("shows previous status", () => {
      const task = makeTask();
      render(
        <PausedTaskCard
          task={task}
          pauseReason={{ ...userPauseReason, previous_status: "reviewing" }}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      expect(screen.getByText("was reviewing")).toBeInTheDocument();
    });

    it("calls onResume when resume button clicked", () => {
      const task = makeTask({ id: "task-u1" });
      render(
        <PausedTaskCard
          task={task}
          pauseReason={userPauseReason}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      fireEvent.click(screen.getByTestId("resume-button-task-u1"));
      expect(onResume).toHaveBeenCalledWith("task-u1");
    });
  });
});

describe("PausedTasksPopover (component)", () => {
  beforeEach(() => {
    resumeMock.mockReset();
    navigateToTaskMock.mockReset();
  });

  function makeProviderErrorTask(id: string, msg = "rate limited"): Task {
    return makeTask({
      id,
      title: `Task ${id}`,
      metadata: JSON.stringify({
        pause_reason: {
          type: "provider_error",
          category: "rate_limit",
          message: msg,
          retry_after: null,
          previous_status: "executing",
          paused_at: "2026-02-15T12:00:00Z",
          auto_resumable: true,
          resume_attempts: 0,
        },
      }),
    });
  }

  function makeUserPausedTask(id: string): Task {
    return makeTask({
      id,
      title: `Task ${id}`,
      metadata: JSON.stringify({
        pause_reason: {
          type: "user_initiated",
          previous_status: "executing",
          paused_at: "2026-02-15T12:00:00Z",
          scope: "global",
        },
      }),
    });
  }

  it("renders header with paused task count", () => {
    render(
      <PausedTasksPopover pausedTasks={[makeUserPausedTask("a"), makeUserPausedTask("b")]}>
        <button>Open</button>
      </PausedTasksPopover>
    );

    expect(screen.getByText("Paused Tasks (2)")).toBeInTheDocument();
  });

  it("shows 'No paused tasks' empty state", () => {
    render(
      <PausedTasksPopover pausedTasks={[]}>
        <button>Open</button>
      </PausedTasksPopover>
    );

    expect(screen.getByText("No paused tasks")).toBeInTheDocument();
  });

  it("shows summary chip 'user paused' when only user-paused tasks", () => {
    render(
      <PausedTasksPopover pausedTasks={[makeUserPausedTask("u1")]}>
        <button>Open</button>
      </PausedTasksPopover>
    );

    expect(screen.getByText("user paused")).toBeInTheDocument();
  });

  it("shows summary chip 'provider errors' when only provider error tasks", () => {
    render(
      <PausedTasksPopover pausedTasks={[makeProviderErrorTask("p1")]}>
        <button>Open</button>
      </PausedTasksPopover>
    );

    expect(screen.getByText("provider errors")).toBeInTheDocument();
  });

  it("shows mixed-group section labels and combined chip when both groups present", () => {
    render(
      <PausedTasksPopover
        pausedTasks={[makeProviderErrorTask("p1"), makeUserPausedTask("u1")]}
      >
        <button>Open</button>
      </PausedTasksPopover>
    );

    expect(screen.getByText("Provider Errors")).toBeInTheDocument();
    expect(screen.getAllByText("User Paused").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("1 errors · 1 user")).toBeInTheDocument();
  });

  it("renders unparsed tasks with fallback user-paused card", () => {
    const unparsed = makeTask({
      id: "no-meta",
      title: "Mystery Task",
      metadata: null,
    });
    render(
      <PausedTasksPopover pausedTasks={[unparsed]}>
        <button>Open</button>
      </PausedTasksPopover>
    );

    expect(screen.getByTestId("paused-task-card-no-meta")).toBeInTheDocument();
  });

  it("calls api.tasks.resume when card resume clicked", async () => {
    resumeMock.mockResolvedValueOnce(undefined);
    render(
      <PausedTasksPopover pausedTasks={[makeProviderErrorTask("p1")]}>
        <button>Open</button>
      </PausedTasksPopover>
    );

    fireEvent.click(screen.getByTestId("resume-button-p1"));

    await waitFor(() => {
      expect(resumeMock).toHaveBeenCalledWith("p1");
    });
  });

  it("logs error and does not throw when resume fails", async () => {
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    resumeMock.mockRejectedValueOnce(new Error("backend down"));
    render(
      <PausedTasksPopover pausedTasks={[makeProviderErrorTask("p1")]}>
        <button>Open</button>
      </PausedTasksPopover>
    );

    fireEvent.click(screen.getByTestId("resume-button-p1"));

    await waitFor(() => {
      expect(errSpy).toHaveBeenCalled();
    });
    errSpy.mockRestore();
  });

  it("calls navigateToTask when view-details clicked", () => {
    render(
      <PausedTasksPopover pausedTasks={[makeProviderErrorTask("p1")]}>
        <button>Open</button>
      </PausedTasksPopover>
    );

    fireEvent.click(screen.getByTestId("view-details-button-p1"));

    expect(navigateToTaskMock).toHaveBeenCalledWith("p1");
  });

  it("renders trigger children", () => {
    render(
      <PausedTasksPopover pausedTasks={[]}>
        <button>Open Paused</button>
      </PausedTasksPopover>
    );

    expect(screen.getByRole("button", { name: "Open Paused" })).toBeInTheDocument();
  });

  it("accepts custom alignOffset prop without throwing", () => {
    expect(() =>
      render(
        <PausedTasksPopover pausedTasks={[]} alignOffset={-100}>
          <button>Open</button>
        </PausedTasksPopover>
      )
    ).not.toThrow();
  });
});
