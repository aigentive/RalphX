import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("./team/TeamPanelContent", () => ({
  TeamPanelContent: () => <div data-testid="agents-team-panel-content">Team content</div>,
}));

import { AgentsTeamPanel } from "./AgentsTeamPanel";

describe("AgentsTeamPanel", () => {
  const originalRequestAnimationFrame = window.requestAnimationFrame;
  const originalCancelAnimationFrame = window.cancelAnimationFrame;

  afterEach(() => {
    vi.useRealTimers();
    window.requestAnimationFrame = originalRequestAnimationFrame;
    window.cancelAnimationFrame = originalCancelAnimationFrame;
  });

  it("renders its Team shell synchronously and defers content until after a paint", () => {
    vi.useFakeTimers();
    window.requestAnimationFrame = ((callback: FrameRequestCallback) =>
      window.setTimeout(() => callback(performance.now()), 1)) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = ((frame: number) =>
      window.clearTimeout(frame)) as typeof window.cancelAnimationFrame;

    render(
      <AgentsTeamPanel
        conversationId="conversation-1"
        projectId="project-1"
        activeAgentRunId="run-1"
      />,
    );

    expect(screen.getByTestId("agents-team-panel")).toHaveAttribute(
      "data-hydrated",
      "false",
    );
    expect(screen.getByTestId("agents-team-panel-shell")).toBeVisible();
    expect(screen.queryByTestId("agents-team-panel-content")).not.toBeInTheDocument();

    act(() => {
      vi.runAllTimers();
    });

    expect(screen.getByTestId("agents-team-panel")).toHaveAttribute(
      "data-hydrated",
      "true",
    );
    expect(screen.queryByTestId("agents-team-panel-shell")).not.toBeInTheDocument();
  });
});
