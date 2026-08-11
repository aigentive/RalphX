import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProgressIndicator } from "./ProgressIndicator";

describe("ProgressIndicator", () => {
  it("renders completed fill and faded active segment as separate layers", () => {
    render(
      <ProgressIndicator
        percentComplete={33.333}
        activePercentComplete={66.667}
        animateActiveProgress
        completedSteps={1}
        totalSteps={3}
      />
    );

    const completed = screen.getByTestId("progress-completed-fill");
    const active = screen.getByTestId("progress-active-segment");

    expect(parseFloat(completed.style.width)).toBeCloseTo(33.333, 2);
    expect(parseFloat(active.style.left)).toBeCloseTo(33.333, 2);
    expect(parseFloat(active.style.width)).toBeCloseTo(33.334, 2);
    expect(active).toHaveAttribute("data-animated", "true");
    expect(screen.getByText("33%")).toBeInTheDocument();
    expect(screen.getByText("1")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
  });

  it("does not render an active segment when active progress does not exceed completed progress", () => {
    render(
      <ProgressIndicator
        percentComplete={100}
        activePercentComplete={100}
        animateActiveProgress
        completedSteps={3}
        totalSteps={3}
      />
    );

    expect(screen.getByTestId("progress-completed-fill").style.width).toBe("100%");
    expect(screen.queryByTestId("progress-active-segment")).not.toBeInTheDocument();
  });
});
