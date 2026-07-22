import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { AutomationPhaseProgress } from "./AutomationPhases";

const { useArtifactMock } = vi.hoisted(() => ({
  useArtifactMock: vi.fn(),
}));

vi.mock("@/hooks/useArtifacts", () => ({
  useArtifact: (...args: unknown[]) => useArtifactMock(...args),
}));

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

  it("shows a plan icon only for items with a mapped plan artifact", () => {
    render(
      <TooltipProvider>
        <AutomationPhaseProgress
          value={phasesJson}
          testId="phases"
          planByGoalItemId={new Map([["p3", "plan-artifact-3"]])}
        />
      </TooltipProvider>,
    );

    const icons = screen.getAllByTestId("phases-plan-icon");
    expect(icons).toHaveLength(1);
    expect(icons[0]).toHaveAccessibleName("View plan for Judge signal");
    const items = screen.getAllByTestId("phases-item");
    expect(within(items[0]!).queryByTestId("phases-plan-icon")).toBeNull();
  });

  it("renders no plan icons without a plan map (fail-closed for unmapped runs)", () => {
    render(<AutomationPhaseProgress value={phasesJson} testId="phases" />);
    expect(screen.queryByTestId("phases-plan-icon")).toBeNull();
  });

  it("opens the plan dialog for the clicked item", async () => {
    useArtifactMock.mockReturnValue({
      data: null,
      isLoading: true,
      isError: false,
    });
    const user = userEvent.setup();
    render(
      <TooltipProvider>
        <AutomationPhaseProgress
          value={phasesJson}
          testId="phases"
          planByGoalItemId={new Map([["p3", "plan-artifact-3"]])}
        />
      </TooltipProvider>,
    );

    expect(screen.queryByTestId("automation-plan-dialog")).toBeNull();
    await user.click(screen.getByTestId("phases-plan-icon"));

    const dialog = screen.getByTestId("automation-plan-dialog");
    expect(within(dialog).getByText("Judge signal")).toBeInTheDocument();
    expect(useArtifactMock).toHaveBeenCalledWith("plan-artifact-3");
  });
});
