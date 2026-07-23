import type { ReactNode } from "react";
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

function renderPhases(element: ReactNode) {
  return render(
    <TooltipProvider delayDuration={0}>{element}</TooltipProvider>,
  );
}

describe("AutomationPhaseProgress", () => {
  it("renders the done count, progress bar, and skipped tally", () => {
    renderPhases(<AutomationPhaseProgress value={phasesJson} testId="phases" />);

    expect(screen.getByTestId("phases-count")).toHaveTextContent("2/5 done");
    expect(screen.getByText("1 skipped")).toBeInTheDocument();

    const bar = screen.getByRole("progressbar", { name: "Phase progress" });
    expect(bar).toHaveAttribute("aria-valuenow", "2");
    expect(bar).toHaveAttribute("aria-valuemax", "5");
  });

  it("uses plain accessible icons for settled/queued phases and one live pill for current work", async () => {
    const user = userEvent.setup();
    renderPhases(<AutomationPhaseProgress value={phasesJson} testId="phases" />);

    const items = screen.getAllByTestId("phases-item");
    expect(items).toHaveLength(5);

    expect(within(items[0]!).getByText("Model context")).toBeInTheDocument();
    expect(within(items[0]!).getByLabelText("Done")).toHaveAttribute("data-phase-status", "done");
    expect(within(items[2]!).getByText("In progress")).toBeInTheDocument();
    expect(within(items[3]!).getByLabelText("Skipped")).toHaveAttribute("data-phase-status", "skipped");
    expect(within(items[4]!).getByLabelText("Pending")).toHaveAttribute("data-phase-status", "pending");

    const current = within(items[2]!).getByText("In progress").closest("[data-phase-status]");
    expect(current).toHaveAttribute("data-phase-status", "in_progress");
    expect(within(items[2]!).getByText("In progress").closest("[data-tone]")).toHaveAttribute(
      "data-tone",
      "accent",
    );
    expect(
      within(items[2]!).getByTestId("automation-phase-in-progress-live-dot"),
    ).toBeInTheDocument();

    await user.hover(within(items[0]!).getByLabelText("Done"));
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Done");
  });

  it("returns null when there are no phases", () => {
    const { container } = renderPhases(<AutomationPhaseProgress value={null} />);
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

    renderPhases(<AutomationPhaseProgress value={many} limit={6} testId="phases" />);

    expect(screen.getAllByTestId("phases-item")).toHaveLength(6);
    // Count reflects the full (unsliced) list.
    expect(screen.getByTestId("phases-count")).toHaveTextContent("3/8 done");
  });

  it("shows a plan icon only for items with a mapped plan artifact", () => {
    renderPhases(
      <AutomationPhaseProgress
        value={phasesJson}
        testId="phases"
        planByGoalItemId={new Map([["p3", "plan-artifact-3"]])}
      />,
    );

    const icons = screen.getAllByTestId("phases-plan-icon");
    expect(icons).toHaveLength(1);
    expect(icons[0]).toHaveAccessibleName("View plan for Judge signal");
    const items = screen.getAllByTestId("phases-item");
    expect(within(items[0]!).queryByTestId("phases-plan-icon")).toBeNull();
  });

  it("renders no plan icons without a plan map (fail-closed for unmapped runs)", () => {
    renderPhases(<AutomationPhaseProgress value={phasesJson} testId="phases" />);
    expect(screen.queryByTestId("phases-plan-icon")).toBeNull();
  });

  it("opens the plan dialog for the clicked item", async () => {
    useArtifactMock.mockReturnValue({
      data: null,
      isLoading: true,
      isError: false,
    });
    const user = userEvent.setup();
    renderPhases(
      <AutomationPhaseProgress
        value={phasesJson}
        testId="phases"
        planByGoalItemId={new Map([["p3", "plan-artifact-3"]])}
      />,
    );

    expect(screen.queryByTestId("automation-plan-dialog")).toBeNull();
    await user.click(screen.getByTestId("phases-plan-icon"));

    const dialog = screen.getByTestId("automation-plan-dialog");
    expect(within(dialog).getByText("Judge signal")).toBeInTheDocument();
    expect(useArtifactMock).toHaveBeenCalledWith("plan-artifact-3");
  });
});
