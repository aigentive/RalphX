import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AgentPlanStartPanel } from "./AgentPlanStartPanel";

describe("AgentPlanStartPanel", () => {
  it("renders the lightweight search and import shells", () => {
    render(<AgentPlanStartPanel />);

    expect(screen.getByTestId("agent-plan-start-panel")).toBeInTheDocument();
    expect(
      screen.getByRole("searchbox", { name: "Search project plans" }),
    ).toBeDisabled();
    expect(screen.getByText("Import markdown")).toBeInTheDocument();
    expect(screen.getByTestId("agent-plan-start-status-idle")).toHaveTextContent(
      "No plan selected",
    );
  });

  it("renders loading, error, and pending states", () => {
    const { rerender } = render(<AgentPlanStartPanel status="loading" />);

    expect(screen.getByTestId("agent-plan-start-status-loading")).toHaveTextContent(
      "Loading plans...",
    );

    rerender(
      <AgentPlanStartPanel
        status="error"
        errorMessage="Unable to prepare plan setup."
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Unable to prepare plan setup.",
    );

    rerender(<AgentPlanStartPanel status="pending" />);
    expect(screen.getByTestId("agent-plan-start-status-pending")).toHaveTextContent(
      "Preparing draft plan...",
    );
  });
});
