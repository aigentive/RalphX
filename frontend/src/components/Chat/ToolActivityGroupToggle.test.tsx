import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ToolActivityGroupToggle } from "./ToolActivityGroupToggle";

const summary = {
  totalTools: 2,
  createdPaths: ["src/new.ts"],
  editedPaths: [],
  changedPaths: [],
  delegatedJobKeys: ["job-1"],
};

describe("ToolActivityGroupToggle", () => {
  it("keeps the summary sentence visible and exposes expansion state", () => {
    const onToggle = vi.fn();
    const { rerender } = render(
      <ToolActivityGroupToggle
        groupKey="group-1"
        summary={summary}
        isExpanded={false}
        onToggle={onToggle}
      />,
    );

    const button = screen.getByRole("button", {
      name: "Agent called 2 tools, created 1 file, and delegated 1 agent. Expand tool details.",
    });
    expect(button).toHaveTextContent(
      "Agent called 2 tools, created 1 file, and delegated 1 agent.",
    );
    expect(button).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(button);
    expect(onToggle).toHaveBeenCalledTimes(1);

    rerender(
      <ToolActivityGroupToggle
        groupKey="group-1"
        summary={summary}
        isExpanded
        onToggle={onToggle}
      />,
    );
    expect(screen.getByRole("button", {
      name: "Agent called 2 tools, created 1 file, and delegated 1 agent. Collapse tool details.",
    })).toHaveAttribute("aria-expanded", "true");
  });
});
