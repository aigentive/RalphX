import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { TimelineEntry } from "./TimelineEntry";
import type { TimelineEvent } from "@/api/task-graph.types";

function makeEvent(overrides: Partial<TimelineEvent> = {}): TimelineEvent {
  return {
    id: "ev-1",
    timestamp: new Date(Date.now() - 5 * 60 * 1000).toISOString(),
    taskId: "t1",
    taskTitle: "Implement search bar",
    eventType: "status_change",
    fromStatus: "ready",
    toStatus: "executing",
    description: "Started executing",
    trigger: null,
    planArtifactId: null,
    sessionTitle: null,
    ...overrides,
  };
}

describe("TimelineEntry", () => {
  it("renders the description and relative time for a status_change", () => {
    render(<TimelineEntry event={makeEvent()} onTaskClick={vi.fn()} />);
    expect(screen.getByText(/Started executing/i)).toBeInTheDocument();
    expect(screen.getByText(/m ago|Just now/i)).toBeInTheDocument();
  });

  it("invokes onTaskClick when an event with a taskId is clicked", async () => {
    const user = userEvent.setup();
    const onTaskClick = vi.fn();
    render(<TimelineEntry event={makeEvent()} onTaskClick={onTaskClick} />);
    await user.click(screen.getByRole("button"));
    expect(onTaskClick).toHaveBeenCalledWith("t1");
  });

  it("supports keyboard activation via Enter and Space", () => {
    const onTaskClick = vi.fn();
    render(<TimelineEntry event={makeEvent()} onTaskClick={onTaskClick} />);
    const btn = screen.getByRole("button");
    fireEvent.keyDown(btn, { key: "Enter" });
    fireEvent.keyDown(btn, { key: " " });
    expect(onTaskClick).toHaveBeenCalledTimes(2);
  });

  it("renders plan_completed events with the approved status style", () => {
    render(
      <TimelineEntry
        event={makeEvent({
          eventType: "plan_completed",
          taskId: null,
          taskTitle: null,
          fromStatus: null,
          toStatus: null,
          description: "Plan accepted and completed",
        })}
      />,
    );
    expect(screen.getByText(/Plan accepted and completed/i)).toBeInTheDocument();
  });

  it("highlights the entry when isHighlighted is true", () => {
    const { container } = render(
      <TimelineEntry event={makeEvent()} onTaskClick={vi.fn()} isHighlighted />,
    );
    const entry = container.firstChild as HTMLElement;
    expect(entry.style.background).toContain("status-info-muted");
  });
});
