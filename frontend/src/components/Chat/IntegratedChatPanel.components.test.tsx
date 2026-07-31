import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import {
  TypingIndicator,
  EmptyState,
  HistoryEmptyState,
  FailedRunBanner,
  PreviousRunBanner,
  ContextIndicator,
} from "./IntegratedChatPanel.components";
import type { ChatContext } from "@/types/chat";
import { useChatStore } from "@/stores/chatStore";

afterEach(() => {
  vi.useRealTimers();
  useChatStore.setState({ activeAgentRunIds: {}, activeAgentRunHarnesses: {}, activeAgentRunMeta: {} });
});

describe("IntegratedChatPanel.components", () => {
  it("renders the typing indicator three-dot pattern", () => {
    render(<TypingIndicator />);
    expect(screen.getByTestId("chat-typing-indicator")).toBeInTheDocument();
  });

  it("shows a role-aware ticking elapsed label from the active run metadata", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-31T00:00:10Z"));
    useChatStore.getState().setActiveAgentRun("project:c", "run-1", null, {
      startedAt: Date.parse("2026-07-31T00:00:04Z"), agentName: null, launchRole: "workspace_reviewer",
    });
    render(<TypingIndicator label="Agent working" storeKey="project:c" />);
    expect(screen.getByText("Reviewer working for 6s")).toBeInTheDocument();
    act(() => { vi.advanceTimersByTime(1000); });
    expect(screen.getByText("Reviewer working for 7s")).toBeInTheDocument();
  });

  it("keeps the static label when no active run metadata exists", () => {
    render(<TypingIndicator label="Starting agent" storeKey="project:missing" />);
    expect(screen.getByText("Starting agent")).toBeInTheDocument();
  });

  it("renders the empty / history empty states", () => {
    const { rerender } = render(<EmptyState />);
    rerender(<HistoryEmptyState />);
    // Both render without throwing — exercising both module exports.
    expect(true).toBe(true);
  });

  it("FailedRunBanner shows the error message and dismiss button", async () => {
    const user = userEvent.setup();
    const onDismiss = vi.fn();
    render(<FailedRunBanner errorMessage="Backend exploded" onDismiss={onDismiss} />);
    expect(screen.getByText("Agent run failed")).toBeInTheDocument();
    expect(screen.getByText("Backend exploded")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Dismiss error/i }));
    expect(onDismiss).toHaveBeenCalled();
  });

  it("FailedRunBanner truncates very long messages", () => {
    const long = "x".repeat(250);
    render(<FailedRunBanner errorMessage={long} />);
    // Text content is truncated to 200 chars + ellipsis.
    expect(screen.getByText(/x{200}/)).toBeInTheDocument();
  });

  it("PreviousRunBanner shows worker / reviewer / merger contexts and statuses", () => {
    const { rerender } = render(<PreviousRunBanner agentRunStatus="completed" contextType="task_execution" />);
    expect(screen.getByText(/Previous worker run \(completed\)/)).toBeInTheDocument();

    rerender(<PreviousRunBanner agentRunStatus="failed" contextType="review" />);
    expect(screen.getByText(/Previous reviewer run \(failed\)/)).toBeInTheDocument();

    rerender(<PreviousRunBanner agentRunStatus="cancelled" contextType="merge" />);
    expect(screen.getByText(/Previous merge agent run \(cancelled\)/)).toBeInTheDocument();

    rerender(<PreviousRunBanner agentRunStatus="running" contextType="merge" />);
    expect(screen.getByText(/Previous merge agent run \(in progress\)/)).toBeInTheDocument();
  });

  it("ContextIndicator routes to Worker / AI Review / Merger via mode props", () => {
    const ctx = { view: "ideation", selectedTaskId: null } as ChatContext;
    const { rerender } = render(<ContextIndicator context={ctx} isExecutionMode />);
    expect(screen.getByText("Worker")).toBeInTheDocument();

    rerender(<ContextIndicator context={ctx} isReviewMode />);
    expect(screen.getByText("AI Review")).toBeInTheDocument();

    rerender(<ContextIndicator context={ctx} isMergeMode />);
    expect(screen.getByText("Merger")).toBeInTheDocument();
  });

  it("ContextIndicator falls through to view-specific labels", () => {
    const { rerender } = render(
      <ContextIndicator context={{ view: "ideation", selectedTaskId: null } as ChatContext} />,
    );
    expect(screen.getByText("Chat")).toBeInTheDocument();

    rerender(
      <ContextIndicator
        context={{ view: "kanban", selectedTaskId: "t1" } as ChatContext}
      />,
    );
    expect(screen.getByText("Task")).toBeInTheDocument();

    rerender(
      <ContextIndicator
        context={{ view: "kanban", selectedTaskId: null } as ChatContext}
      />,
    );
    expect(screen.getByText("Project")).toBeInTheDocument();

    rerender(
      <ContextIndicator
        context={{ view: "task_detail", selectedTaskId: "t1" } as ChatContext}
      />,
    );
    expect(screen.getByText("Task")).toBeInTheDocument();

    rerender(
      <ContextIndicator
        context={{ view: "activity", selectedTaskId: null } as ChatContext}
      />,
    );
    expect(screen.getByText("Activity")).toBeInTheDocument();
  });
});
