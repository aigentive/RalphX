import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";

import { StatCard } from "./StatCard";

describe("StatCard", () => {
  it("renders label, value and optional sub copy", () => {
    render(<StatCard label="Throughput" value="42" sub="last 7 days" />);
    expect(screen.getByText("Throughput")).toBeInTheDocument();
    expect(screen.getByText("42")).toBeInTheDocument();
    expect(screen.getByText("last 7 days")).toBeInTheDocument();
  });

  it("renders the help-tooltip trigger when tooltip prop is provided", () => {
    const { container } = render(
      <StatCard label="P95" value="3.2s" tooltip="95th percentile cycle time" />,
    );
    // Lucide HelpCircle is rendered as an SVG; its presence proves the
    // tooltip-trigger branch executed.
    expect(container.querySelector("svg")).toBeInTheDocument();
  });

  it("hides the sub copy and tooltip when not provided", () => {
    const { container } = render(<StatCard label="Cycle" value="1.2s" />);
    expect(screen.queryByText("last 7 days")).toBeNull();
    expect(container.querySelector("svg")).toBeNull();
  });
});
