import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { ProposalWidget } from "../ProposalWidget";
import type { ToolCall } from "../shared.constants";

/** Wrap a JSON object in the MCP content array format */
function mcpWrap(obj: Record<string, unknown>): Array<{ type: string; text: string }> {
  return [{ type: "text", text: JSON.stringify(obj) }];
}

function makeToolCall(overrides: Partial<ToolCall>): ToolCall {
  return {
    id: "test-id",
    name: "create_task_proposal",
    arguments: {},
    ...overrides,
  };
}

describe("ProposalWidget", () => {
  it("shows title and category badge for create with title in args", () => {
    const toolCall = makeToolCall({
      name: "create_task_proposal",
      arguments: { title: "My Task", category: "feature" },
      result: mcpWrap({ id: "p1", title: "My Task", category: "feature" }),
    });

    render(<ProposalWidget toolCall={toolCall} />);

    expect(screen.getByText("My Task")).toBeInTheDocument();
    expect(screen.getByText("feature")).toBeInTheDocument();
    expect(screen.getByText("Created")).toBeInTheDocument();
  });

  it("shows actual title for update when title is only in MCP-wrapped result", () => {
    const toolCall = makeToolCall({
      name: "update_task_proposal",
      arguments: { proposal_id: "p1", steps: [{ title: "Step 1" }, { title: "Step 2" }] },
      result: mcpWrap({ id: "p1", title: "Real Proposal Title", category: "refactor" }),
    });

    render(<ProposalWidget toolCall={toolCall} />);

    // Should show actual title from result, NOT the "Proposal" fallback
    expect(screen.getByText("Real Proposal Title")).toBeInTheDocument();
    expect(screen.getByText("Updated")).toBeInTheDocument();
    // Steps count appears in changed fields
    expect(screen.getByText(/steps \(2\)/)).toBeInTheDocument();
  });

  it("shows 'Proposal' fallback gracefully when update has no result", () => {
    const toolCall = makeToolCall({
      name: "update_task_proposal",
      arguments: { proposal_id: "p1", user_priority: "high" },
      result: undefined,
    });

    render(<ProposalWidget toolCall={toolCall} />);

    expect(screen.getByText("Proposal")).toBeInTheDocument();
    expect(screen.getByText("Updated")).toBeInTheDocument();
    expect(screen.getByText(/priority → high/)).toBeInTheDocument();
  });

  it("shows title with strikethrough for delete when title is in MCP-wrapped result", () => {
    const toolCall = makeToolCall({
      name: "archive_task_proposal",
      arguments: { proposal_id: "p1" },
      result: mcpWrap({ id: "p1", title: "Deleted Proposal" }),
    });

    const { container } = render(<ProposalWidget toolCall={toolCall} />);

    expect(screen.getByText("Deleted Proposal")).toBeInTheDocument();
    expect(screen.getByText("Deleted")).toBeInTheDocument();
    // Title span has line-through style
    const titleSpan = container.querySelector('span[style*="line-through"]');
    expect(titleSpan).toBeInTheDocument();
  });
});
