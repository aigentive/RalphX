import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { ThinkingGroupToggle } from "./ThinkingGroupToggle";

describe("ThinkingGroupToggle", () => {
  it("renders active thinking label when not settled", () => {
    render(
      <ThinkingGroupToggle
        groupKey="blk-0"
        isExpanded={false}
        isSettled={false}
        onToggle={() => {}}
      />,
    );
    expect(screen.getByTestId("thinking-group-toggle")).toBeInTheDocument();
    expect(screen.getByText(/Agent thinking/)).toBeInTheDocument();
  });

  it("renders settled label with duration", () => {
    render(
      <ThinkingGroupToggle
        groupKey="blk-1"
        isExpanded={true}
        isSettled={true}
        durationMs={12000}
        onToggle={() => {}}
      />,
    );
    expect(screen.getByText(/Agent thought for 12s/)).toBeInTheDocument();
  });

  it("renders active estimated token progress in its accessible label", () => {
    render(
      <ThinkingGroupToggle
        groupKey="blk-progress"
        isExpanded={false}
        isSettled={false}
        estimatedTokens={2_000}
        onToggle={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: /Agent thinking… · ~2,000 tokens/ })).toBeInTheDocument();
  });

  it("renders settled label without duration", () => {
    render(
      <ThinkingGroupToggle
        groupKey="blk-2"
        isExpanded={false}
        isSettled={true}
        onToggle={() => {}}
      />,
    );
    expect(screen.getByText("Agent thought")).toBeInTheDocument();
  });

  it("renders the aggregate step count for a settled multi-segment group", () => {
    render(
      <ThinkingGroupToggle
        groupKey="blk-group"
        isExpanded={false}
        isSettled
        durationMs={12_000}
        segmentCount={2}
        onToggle={() => {}}
      />,
    );

    expect(screen.getByRole("button", { name: /Agent thought for 12s · 2 steps/ })).toBeInTheDocument();
  });
});
