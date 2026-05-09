import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { buildTaskCardSummaryParts, getTaskCardKindLabel } from "./TaskCardShared.utils";
import {
  TaskCardKindBadge,
  TaskCardSubagentTypeBadge,
  TaskCardModelBadge,
  TaskCardStatusBadge,
} from "./TaskCardShared";

describe("TaskCardShared component badges", () => {
  it("renders the Task kind label by default", () => {
    render(<TaskCardKindBadge toolName="UnknownTool" />);
    expect(screen.getByText("Task")).toBeInTheDocument();
  });

  it("renders Delegate label for delegate_start tool name", () => {
    render(<TaskCardKindBadge toolName="delegate_start" />);
    expect(screen.getByText("Delegate")).toBeInTheDocument();
  });

  it("hides TaskCardSubagentTypeBadge for default 'agent' subagent", () => {
    const { container } = render(<TaskCardSubagentTypeBadge subagentType="agent" />);
    expect(container.firstChild).toBeNull();
  });

  it("renders TaskCardSubagentTypeBadge for non-default subagent type", () => {
    render(<TaskCardSubagentTypeBadge subagentType="research" />);
    expect(screen.getByText("research")).toBeInTheDocument();
  });

  it("hides TaskCardModelBadge for null label and renders for given label", () => {
    const { container, rerender } = render(<TaskCardModelBadge label={null} />);
    expect(container.firstChild).toBeNull();
    rerender(<TaskCardModelBadge label="opus" colorKey="opus" />);
    expect(screen.getByText("opus")).toBeInTheDocument();
  });

  it("hides TaskCardStatusBadge with null label and renders for both tones", () => {
    const { container, rerender } = render(<TaskCardStatusBadge label={null} />);
    expect(container.firstChild).toBeNull();
    rerender(<TaskCardStatusBadge label="failed" tone="error" />);
    expect(screen.getByText("failed")).toBeInTheDocument();
    rerender(<TaskCardStatusBadge label="needs review" tone="warning" />);
    expect(screen.getByText("needs review")).toBeInTheDocument();
  });
});

describe("TaskCardShared", () => {
  it("classifies task card kind labels across delegate, agent, and task names", () => {
    expect(getTaskCardKindLabel("delegate_start")).toBe("Delegate");
    expect(getTaskCardKindLabel("ralphx::delegate_start")).toBe("Delegate");
    expect(getTaskCardKindLabel("Agent")).toBe("Agent");
    expect(getTaskCardKindLabel("Task")).toBe("Task");
  });

  it("builds summary parts from duration, usage, tool count, and cost", () => {
    expect(
      buildTaskCardSummaryParts({
        totalDurationMs: 6200,
        totalTokens: 1532,
        totalToolUseCount: 3,
        estimatedUsd: 0.43,
      }),
    ).toEqual(["6s", "1,532 tokens", "3 tools", "$0.43"]);
  });

  it("omits absent summary parts cleanly", () => {
    expect(
      buildTaskCardSummaryParts({
        totalTokens: 12,
      }),
    ).toEqual(["12 tokens"]);
  });
});
