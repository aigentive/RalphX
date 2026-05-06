/**
 * Tests for TaskBoardSkeleton component
 */

import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { TaskBoardSkeleton } from "./TaskBoardSkeleton";

describe("TaskBoardSkeleton", () => {
  it("should render with data-testid", () => {
    render(<TaskBoardSkeleton />);
    expect(screen.getByTestId("task-board-skeleton")).toBeInTheDocument();
  });

  it("should render 5 column placeholders", () => {
    render(<TaskBoardSkeleton />);
    const columns = screen.getAllByTestId(/skeleton-column-/);
    expect(columns).toHaveLength(5);
  });

  it("should render headers for all stable v29a columns", () => {
    render(<TaskBoardSkeleton />);
    const headers = screen.getAllByTestId(/skeleton-header-/);
    expect(headers).toHaveLength(5);
  });

  it("should render card placeholders in expanded columns", () => {
    render(<TaskBoardSkeleton />);
    const cards = screen.getAllByTestId(/skeleton-card-/);
    expect(cards.length).toBeGreaterThan(0);
  });

  it("should apply animate-pulse class for loading animation", () => {
    render(<TaskBoardSkeleton />);
    const skeleton = screen.getByTestId("task-board-skeleton");
    const pulsingElements = skeleton.querySelectorAll(".animate-pulse");
    expect(pulsingElements.length).toBeGreaterThan(0);
  });

  it("should render columns as stable grid cells", () => {
    render(<TaskBoardSkeleton />);

    for (let i = 0; i < 5; i++) {
      const col = screen.getByTestId(`skeleton-column-${i}`);
      expect(col.style.background).toBe("var(--kanban-column-bg)");
    }
  });

  it("should use correct background color", () => {
    render(<TaskBoardSkeleton />);
    const skeleton = screen.getByTestId("task-board-skeleton");
    expect(skeleton.style.background).toBe("var(--kanban-board-divider)");
  });
});
