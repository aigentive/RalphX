import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { AgentTaskWidget } from "./AgentTaskWidget";
import type { ToolCall } from "./shared.constants";

function makeToolCall(overrides: Partial<ToolCall>): ToolCall {
  return {
    id: "agent-task-tool-1",
    name: "mcp__ralphx_internal__create_agent_task",
    arguments: {},
    ...overrides,
  };
}

describe("AgentTaskWidget", () => {
  it("renders an internal MCP create_agent_task result as a task card", () => {
    render(
      <AgentTaskWidget
        toolCall={makeToolCall({
          arguments: {
            title: "Map task ledger scope",
            details: "Confirm MCP writes match composer reads.",
          },
          result: {
            success: true,
            task: {
              task_id: "task-1",
              task_number: 1,
              title: "Map task ledger scope",
              details: "Confirm MCP writes match composer reads.",
              state: "open",
              owner_agent: "ralphx-chat-project",
              blocked_by: [],
              blocks: ["task-2"],
              availability: "ready",
            },
            changed_fields: ["created"],
          },
        })}
      />,
    );

    expect(screen.getByText(/Agent task created #1 Map task ledger scope/i)).toBeInTheDocument();
    expect(screen.getByText("open")).toBeInTheDocument();
    expect(screen.getByText("ralphx-chat-project")).toBeInTheDocument();
    expect(screen.getByText("blocks 1")).toBeInTheDocument();
  });

  it("renders list_agent_tasks with task states and owners", () => {
    render(
      <AgentTaskWidget
        toolCall={makeToolCall({
          name: "mcp__ralphx_internal__list_agent_tasks",
          result: {
            success: true,
            tasks: [
              {
                task_id: "task-1",
                task_number: 1,
                title: "Inspect runtime context",
                state: "done",
                owner_agent: "worker",
                blocked_by: [],
                blocks: [],
                availability: "done",
              },
              {
                task_id: "task-2",
                task_number: 2,
                title: "Add task widgets",
                state: "active",
                owner_agent: "chat",
                blocked_by: ["task-1"],
                blocks: [],
                availability: "blocked",
              },
            ],
          },
        })}
      />,
    );

    expect(screen.getByText("Agent task ledger")).toBeInTheDocument();
    expect(screen.getByText("2 tasks")).toBeInTheDocument();
    expect(screen.getByText("Inspect runtime context")).toBeInTheDocument();
    expect(screen.getByText("Add task widgets")).toBeInTheDocument();
    expect(screen.getByText("blocked by 1")).toBeInTheDocument();
  });

  it("renders structured tool errors without falling back to the generic widget", () => {
    render(
      <AgentTaskWidget
        toolCall={makeToolCall({
          name: "mcp__ralphx_internal__claim_agent_task",
          result: {
            success: false,
            error: "Agent task has unresolved blockers",
          },
        })}
      />,
    );

    expect(screen.getByText(/Claim agent task failed/i)).toBeInTheDocument();
    expect(screen.getByText(/unresolved blockers/i)).toBeInTheDocument();
  });
});
