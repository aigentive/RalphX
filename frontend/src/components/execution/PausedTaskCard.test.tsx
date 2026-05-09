/**
 * PausedTaskCard component tests
 */

import { describe, it, expect, vi, afterEach } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import {
  PausedTaskCard,
  type PauseReason,
  type ProviderErrorMetadata,
  type UserPauseMetadata,
} from "./PausedTaskCard";
import type { Task } from "@/types/task";

function makeTask(overrides: Partial<Task> = {}): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    category: "feature",
    title: "Sample Paused Task",
    description: null,
    priority: 50,
    internalStatus: "paused",
    needsReviewPoint: false,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    ...overrides,
  };
}

function providerError(
  overrides: Partial<ProviderErrorMetadata> = {}
): PauseReason {
  return {
    type: "provider_error",
    category: "rate_limit",
    message: "rate limited",
    retry_after: null,
    previous_status: "executing",
    paused_at: new Date().toISOString(),
    auto_resumable: false,
    resume_attempts: 0,
    ...overrides,
  };
}

function userPause(overrides: Partial<UserPauseMetadata> = {}): PauseReason {
  return {
    type: "user_initiated",
    previous_status: "executing",
    paused_at: new Date().toISOString(),
    scope: "task",
    ...overrides,
  };
}

describe("PausedTaskCard", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  describe("user_initiated pause", () => {
    it("renders the User Paused badge and previous status", () => {
      const task = makeTask({ id: "u-1", title: "User Paused Task" });
      render(
        <PausedTaskCard
          task={task}
          pauseReason={userPause({ previous_status: "executing" })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.getByTestId("paused-task-card-u-1")).toBeInTheDocument();
      expect(screen.getByText("User Paused")).toBeInTheDocument();
      expect(screen.getByText("Paused by user")).toBeInTheDocument();
      expect(screen.getByText("was executing")).toBeInTheDocument();
      expect(screen.getByText("just now")).toBeInTheDocument();
    });

    it("calls onResume when resume button is clicked", () => {
      const onResume = vi.fn();
      const task = makeTask({ id: "u-2" });
      render(
        <PausedTaskCard
          task={task}
          pauseReason={userPause()}
          onResume={onResume}
          onViewDetails={vi.fn()}
        />
      );
      fireEvent.click(screen.getByTestId("resume-button-u-2"));
      expect(onResume).toHaveBeenCalledWith("u-2");
    });

    it("calls onViewDetails when title or details button is clicked", () => {
      const onViewDetails = vi.fn();
      const task = makeTask({ id: "u-3", title: "Click Title" });
      render(
        <PausedTaskCard
          task={task}
          pauseReason={userPause()}
          onResume={vi.fn()}
          onViewDetails={onViewDetails}
        />
      );
      fireEvent.click(screen.getByText("Click Title"));
      fireEvent.click(screen.getByTestId("view-details-button-u-3"));
      expect(onViewDetails).toHaveBeenCalledTimes(2);
      expect(onViewDetails).toHaveBeenNthCalledWith(1, "u-3");
    });

    it("formats paused-at as 'Xm ago' for minutes-old pause", () => {
      const minutesAgo = new Date(Date.now() - 5 * 60 * 1000).toISOString();
      render(
        <PausedTaskCard
          task={makeTask({ id: "u-min" })}
          pauseReason={userPause({ paused_at: minutesAgo })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.getByText("5m ago")).toBeInTheDocument();
    });

    it("formats paused-at as 'Xh Ym ago' for hour-old pause", () => {
      const hoursAgo = new Date(Date.now() - (2 * 3600 + 15 * 60) * 1000).toISOString();
      render(
        <PausedTaskCard
          task={makeTask({ id: "u-hr" })}
          pauseReason={userPause({ paused_at: hoursAgo })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.getByText("2h 15m ago")).toBeInTheDocument();
    });

    it("updates time-since on the 30s interval tick", () => {
      vi.useFakeTimers();
      const pausedAt = new Date(Date.now() - 30 * 1000).toISOString();
      render(
        <PausedTaskCard
          task={makeTask({ id: "u-tick" })}
          pauseReason={userPause({ paused_at: pausedAt })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.getByText("just now")).toBeInTheDocument();
      act(() => {
        vi.advanceTimersByTime(30000);
      });
      expect(screen.getByText("1m ago")).toBeInTheDocument();
    });
  });

  describe("provider_error pause", () => {
    it.each([
      ["rate_limit", "Rate Limit"],
      ["auth_error", "Auth Error"],
      ["server_error", "Server Error"],
      ["network_error", "Network"],
      ["overloaded", "Overloaded"],
    ] as const)("renders %s category badge label %s", (category, label) => {
      render(
        <PausedTaskCard
          task={makeTask({ id: `cat-${category}` })}
          pauseReason={providerError({ category })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.getByText(label)).toBeInTheDocument();
    });

    it("renders error message and resume attempts counter", () => {
      render(
        <PausedTaskCard
          task={makeTask({ id: "p-1" })}
          pauseReason={providerError({
            message: "API down",
            resume_attempts: 2,
          })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.getByText("API down")).toBeInTheDocument();
      expect(screen.getByText("2/5")).toBeInTheDocument();
    });

    it("truncates long error messages to 60 chars + ellipsis", () => {
      const longMsg =
        "This is a really long error message that should definitely be truncated by the component";
      render(
        <PausedTaskCard
          task={makeTask({ id: "p-trunc" })}
          pauseReason={providerError({ message: longMsg })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      const el = screen.getByText((content) => content.endsWith("..."));
      expect(el.textContent?.length).toBe(60);
    });

    it("shows the Auto badge when auto_resumable is true", () => {
      render(
        <PausedTaskCard
          task={makeTask({ id: "p-auto" })}
          pauseReason={providerError({ auto_resumable: true })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.getByText("Auto")).toBeInTheDocument();
    });

    it("hides the Auto badge when auto_resumable is false", () => {
      render(
        <PausedTaskCard
          task={makeTask({ id: "p-noauto" })}
          pauseReason={providerError({ auto_resumable: false })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.queryByText("Auto")).not.toBeInTheDocument();
    });

    it("renders countdown when retry_after is in the future", () => {
      const future = new Date(Date.now() + 65 * 1000).toISOString();
      render(
        <PausedTaskCard
          task={makeTask({ id: "p-cd" })}
          pauseReason={providerError({ retry_after: future })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      // 1m 5s
      expect(screen.getByText(/1m\s+\ds/)).toBeInTheDocument();
    });

    it("shows 'Retrying soon...' when retry_after is in the past", () => {
      const past = new Date(Date.now() - 5 * 1000).toISOString();
      render(
        <PausedTaskCard
          task={makeTask({ id: "p-past" })}
          pauseReason={providerError({ retry_after: past })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.getByText("Retrying soon...")).toBeInTheDocument();
    });

    it("renders countdown in seconds for short retry windows", () => {
      const future = new Date(Date.now() + 30 * 1000).toISOString();
      render(
        <PausedTaskCard
          task={makeTask({ id: "p-secs" })}
          pauseReason={providerError({ retry_after: future })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.getByText(/^\d+s$/)).toBeInTheDocument();
    });

    it("renders countdown in hours for long retry windows", () => {
      const future = new Date(Date.now() + (2 * 3600 + 30 * 60) * 1000).toISOString();
      render(
        <PausedTaskCard
          task={makeTask({ id: "p-hrs" })}
          pauseReason={providerError({ retry_after: future })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.getByText(/^2h\s+\d+m$/)).toBeInTheDocument();
    });

    it("does not render countdown separator when retry_after is null", () => {
      render(
        <PausedTaskCard
          task={makeTask({ id: "p-nocd" })}
          pauseReason={providerError({ retry_after: null })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      expect(screen.queryByText("Retrying soon...")).not.toBeInTheDocument();
    });

    it("ticks countdown every second", () => {
      vi.useFakeTimers();
      const future = new Date(Date.now() + 30 * 1000).toISOString();
      render(
        <PausedTaskCard
          task={makeTask({ id: "p-tick" })}
          pauseReason={providerError({ retry_after: future })}
          onResume={vi.fn()}
          onViewDetails={vi.fn()}
        />
      );
      const initial = screen.getByText(/^\d+s$/).textContent;
      act(() => {
        vi.advanceTimersByTime(2000);
      });
      const next = screen.getByText(/^\d+s$|Retrying/).textContent;
      expect(next).not.toBe(initial);
    });

    it("calls onResume and onViewDetails for provider error variant", () => {
      const onResume = vi.fn();
      const onViewDetails = vi.fn();
      const task = makeTask({ id: "p-cb", title: "Click Provider" });
      render(
        <PausedTaskCard
          task={task}
          pauseReason={providerError()}
          onResume={onResume}
          onViewDetails={onViewDetails}
        />
      );
      fireEvent.click(screen.getByTestId("resume-button-p-cb"));
      fireEvent.click(screen.getByTestId("view-details-button-p-cb"));
      fireEvent.click(screen.getByText("Click Provider"));
      expect(onResume).toHaveBeenCalledWith("p-cb");
      expect(onViewDetails).toHaveBeenCalledTimes(2);
    });
  });
});
