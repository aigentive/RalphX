import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  useQuery: vi.fn(),
}));

vi.mock("@tanstack/react-query", () => ({
  useQuery: mocks.useQuery,
}));

import { TeamTaskBoard } from "./TeamTaskBoard";

describe("TeamTaskBoard", () => {
  beforeEach(() => {
    mocks.useQuery.mockReset();
  });

  it("shows a lightweight loading state while board data is pending", () => {
    mocks.useQuery.mockReturnValue({ isLoading: true, isError: false });

    render(<TeamTaskBoard conversationId="conversation-1" projectId="project-1" />);

    expect(screen.getByTestId("team-task-board-loading")).toBeInTheDocument();
  });

  it("shows an inline error instead of an empty board when the board query fails", () => {
    mocks.useQuery.mockReturnValue({ isLoading: false, isError: true });

    render(<TeamTaskBoard conversationId="conversation-1" projectId="project-1" />);

    expect(screen.getByText("Could not load Team board tasks.")).toBeInTheDocument();
  });

  it("keeps rendering task columns after a successful fetch", () => {
    mocks.useQuery.mockReturnValue({
      isLoading: false,
      isError: false,
      data: [{ taskId: "task-1", taskNumber: 1, title: "Investigate", state: "active" }],
    });

    render(<TeamTaskBoard conversationId="conversation-1" projectId="project-1" />);

    expect(screen.getByText("#1 Investigate")).toBeInTheDocument();
    expect(screen.getByText("Active")).toBeInTheDocument();
  });
});
