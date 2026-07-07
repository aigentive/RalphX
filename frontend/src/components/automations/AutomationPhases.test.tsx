import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AutomationPhaseProgress } from "./AutomationPhases";

const phasesJson = JSON.stringify([
  { id: "p1", title: "Model context", status: "done" },
  { id: "p2", title: "Wire scheduler", status: "done" },
  { id: "p3", title: "Judge signal", status: "in_progress" },
  { id: "p4", title: "Cleanup", status: "skipped" },
  { id: "p5", title: "Docs", status: "pending" },
]);

describe("AutomationPhaseProgress", () => {
  it("renders the done count, progress bar, and skipped tally", () => {
    render(<AutomationPhaseProgress value={phasesJson} testId="phases" />);

    expect(screen.getByTestId("phases-count")).toHaveTextContent("2/5 done");
    expect(screen.getByText("1 skipped")).toBeInTheDocument();

    const bar = screen.getByRole("progressbar", { name: "Phase progress" });
    expect(bar).toHaveAttribute("aria-valuenow", "2");
    expect(bar).toHaveAttribute("aria-valuemax", "5");
  });

  it("renders every phase title with a distinct status badge", () => {
    render(<AutomationPhaseProgress value={phasesJson} testId="phases" />);

    const items = screen.getAllByTestId("phases-item");
    expect(items).toHaveLength(5);

    expect(within(items[0]!).getByText("Model context")).toBeInTheDocument();
    expect(within(items[0]!).getByText("Done")).toBeInTheDocument();
    expect(within(items[2]!).getByText("In progress")).toBeInTheDocument();
    expect(within(items[3]!).getByText("Skipped")).toBeInTheDocument();
    expect(within(items[4]!).getByText("Pending")).toBeInTheDocument();

    // Status badges expose the normalized status for styling assertions.
    expect(within(items[0]!).getByText("Done").closest("[data-phase-status]"))
      .toHaveAttribute("data-phase-status", "done");
    expect(within(items[2]!).getByText("In progress").closest("[data-phase-status]"))
      .toHaveAttribute("data-phase-status", "in_progress");
  });

  it("returns null when there are no phases", () => {
    const { container } = render(<AutomationPhaseProgress value={null} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("truncates the rendered list to the limit but keeps the full done count", () => {
    const many = JSON.stringify(
      Array.from({ length: 8 }, (_, index) => ({
        id: `p${index}`,
        title: `Phase ${index + 1}`,
        status: index < 3 ? "done" : "pending",
      })),
    );

    render(<AutomationPhaseProgress value={many} limit={6} testId="phases" />);

    expect(screen.getAllByTestId("phases-item")).toHaveLength(6);
    // Count reflects the full (unsliced) list.
    expect(screen.getByTestId("phases-count")).toHaveTextContent("3/8 done");
  });
});
