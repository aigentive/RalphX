import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ProactiveSyncNotificationBanner } from "./ProactiveSyncNotification";
import type { ProactiveSyncNotification } from "@/stores/ideationStore";

function makeNotification(
  overrides: Partial<ProactiveSyncNotification> = {},
): ProactiveSyncNotification {
  return {
    id: "n1",
    sessionId: "session-1",
    proposalIds: ["p1", "p2"],
    summary: "test",
    timestamp: Date.now(),
    ...overrides,
  } as ProactiveSyncNotification;
}

describe("ProactiveSyncNotificationBanner", () => {
  it("renders the affected proposal count and the three action buttons", () => {
    render(
      <ProactiveSyncNotificationBanner
        notification={makeNotification()}
        onDismiss={vi.fn()}
        onReview={vi.fn()}
        onUndo={vi.fn()}
      />,
    );
    expect(screen.getByText("Plan updated")).toBeInTheDocument();
    expect(screen.getByText(/2 proposals may need revision/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Review/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Undo/i })).toBeInTheDocument();
  });

  it("invokes the review callback when the Review button is clicked", async () => {
    const user = userEvent.setup();
    const onReview = vi.fn();
    render(
      <ProactiveSyncNotificationBanner
        notification={makeNotification({ proposalIds: ["only-one"] })}
        onDismiss={vi.fn()}
        onReview={onReview}
        onUndo={vi.fn()}
      />,
    );
    expect(screen.getByText(/1 proposal may need revision/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /Review/i }));
    expect(onReview).toHaveBeenCalledOnce();
  });

  it("invokes the undo callback when Undo is clicked", async () => {
    const user = userEvent.setup();
    const onUndo = vi.fn();
    render(
      <ProactiveSyncNotificationBanner
        notification={makeNotification()}
        onDismiss={vi.fn()}
        onReview={vi.fn()}
        onUndo={onUndo}
      />,
    );
    await user.click(screen.getByRole("button", { name: /Undo/i }));
    expect(onUndo).toHaveBeenCalledOnce();
  });
});
