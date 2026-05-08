import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { AcceptedSessionBanner } from "./AcceptedSessionBanner";
import type { TaskProposal } from "@/types/ideation";

vi.mock("@/hooks/useTasks", () => ({
  useTasks: () => ({
    data: [
      { id: "t1", internalStatus: "executing" },
      { id: "t2", internalStatus: "merged" },
    ],
  }),
}));

function makeProposal(overrides: Partial<TaskProposal> = {}): TaskProposal {
  return {
    id: "proposal-1",
    sessionId: "session-1",
    title: "Add tests",
    description: "Backfill test coverage",
    createdTaskId: "t1",
    ...overrides,
  } as TaskProposal;
}

describe("AcceptedSessionBanner", () => {
  it("renders the accepted timestamp + View Work CTA", async () => {
    const user = userEvent.setup();
    const onViewWork = vi.fn();
    render(
      <AcceptedSessionBanner
        projectId="proj-1"
        proposals={[makeProposal()]}
        convertedAt="2026-04-22T12:00:00Z"
        onViewWork={onViewWork}
      />,
    );
    expect(screen.getByTestId("view-work-button")).toBeInTheDocument();
    await user.click(screen.getByTestId("view-work-button"));
    expect(onViewWork).toHaveBeenCalledOnce();
  });

  it("returns null when no proposals have createdTaskId set", () => {
    const { container } = render(
      <AcceptedSessionBanner
        projectId="proj-1"
        proposals={[makeProposal({ createdTaskId: null })]}
        convertedAt={null}
        onViewWork={vi.fn()}
      />,
    );
    expect(container.innerHTML).toBe("");
  });
});
