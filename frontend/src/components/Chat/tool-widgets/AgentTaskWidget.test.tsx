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

  it("clarifies that an empty default list is an unresolved-task snapshot", () => {
    render(
      <AgentTaskWidget
        toolCall={makeToolCall({
          name: "mcp__ralphx_internal__list_agent_tasks",
          arguments: {
            include_done: false,
          },
          result: {
            content: [
              {
                type: "text",
                text: JSON.stringify({
                  success: true,
                  tasks: [],
                  error: null,
                }),
              },
            ],
          },
        })}
      />,
    );

    expect(screen.getByText("0 unresolved")).toBeInTheDocument();
    expect(
      screen.getByText("No unresolved agent tasks found in this snapshot"),
    ).toBeInTheDocument();
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

  it("uses a compact inline row when a completed task call has no body details", () => {
    render(
      <AgentTaskWidget
        toolCall={makeToolCall({
          name: "mcp__ralphx_internal__complete_agent_task",
          arguments: {
            task_ref: "1",
          },
          result: {
            success: true,
          },
        })}
      />,
    );

    expect(screen.getByTestId("agent-task-widget-inline")).toHaveTextContent(
      "Complete agent task #1",
    );
    expect(screen.getByTestId("agent-task-widget-inline")).toHaveTextContent("done");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it.each([
    {
      toolName: "mcp__ralphx_internal__claim_agent_task",
      expectedText: "Agent task claimed #7 Validate ledger labels",
    },
    {
      toolName: "mcp__ralphx_internal__complete_agent_task",
      expectedText: "Agent task completed #7 Validate ledger labels",
    },
  ])("renders task identity from a truncated $toolName preview", ({ toolName, expectedText }) => {
    render(
      <AgentTaskWidget
        toolCall={makeToolCall({
          name: toolName,
          arguments: {},
          resultPreviewTruncated: true,
          result:
            "{\n  \"success\": true,\n  \"task\": {\n    \"task_number\": 7,\n    \"title\": \"Validate ledger labels\"\n",
        })}
      />,
    );

    expect(screen.getByTestId("agent-task-widget-inline")).toHaveTextContent(expectedText);
  });

  it("renders task identity from object text preview payloads", () => {
    render(
      <AgentTaskWidget
        toolCall={makeToolCall({
          name: "mcp__ralphx_internal__claim_agent_task",
          resultPreviewTruncated: true,
          result: {
            text: String.raw`{"success":true,"task":{"task_number":8,"title":"Bad\q title"}}`,
          },
        })}
      />,
    );

    expect(screen.getByTestId("agent-task-widget-inline")).toHaveTextContent(
      String.raw`Agent task claimed #8 Bad\q title`,
    );
  });

  it("falls back to the requested ref when a preview result has no task text", () => {
    render(
      <AgentTaskWidget
        toolCall={makeToolCall({
          name: "mcp__ralphx_internal__claim_agent_task",
          arguments: {
            task_ref: "9",
          },
          resultPreviewTruncated: true,
          result: 7,
        })}
      />,
    );

    expect(screen.getByTestId("agent-task-widget-inline")).toHaveTextContent(
      "Claim agent task #9",
    );
  });

  it.each([
    {
      toolName: "mcp__ralphx_internal__get_delegate_assignment",
      assignmentState: "active",
      expectedTitle: "Assigned work #4 Inspect recovery",
      expectedState: "in progress",
    },
    {
      toolName: "mcp__ralphx_internal__complete_delegate_assignment",
      assignmentState: "completion_requested",
      expectedTitle: "Completion requested #4 Inspect recovery",
      expectedState: "completion requested",
    },
    {
      toolName: "mcp__ralphx_internal__release_delegate_assignment",
      assignmentState: "release_requested",
      expectedTitle: "Release requested #4 Inspect recovery",
      expectedState: "release requested",
    },
    {
      toolName: "mcp__ralphx_internal__get_delegate_assignment",
      assignmentState: "released",
      expectedTitle: "Assigned work #4 Inspect recovery",
      expectedState: "released",
    },
    {
      toolName: "mcp__ralphx_internal__get_delegate_assignment",
      assignmentState: "failed",
      expectedTitle: "Assigned work #4 Inspect recovery",
      expectedState: "failed",
    },
    {
      toolName: "mcp__ralphx_internal__get_delegate_assignment",
      assignmentState: "cancelled",
      expectedTitle: "Assigned work #4 Inspect recovery",
      expectedState: "cancelled",
    },
  ])(
    "renders assignment lifecycle result for $toolName",
    ({ toolName, assignmentState, expectedTitle, expectedState }) => {
      render(
        <AgentTaskWidget
          toolCall={makeToolCall({
            name: toolName,
            result: {
              success: true,
              assignment: {
                task_number: 4,
                title: "Inspect recovery",
                details: "Verify the exact attempt.",
                task_state: "active",
                assignment_state: assignmentState,
                delegate_agent_name: "ralphx-general-explorer",
                caller_scope_type: "conversation",
              },
            },
          })}
        />,
      );

      expect(screen.getByText(expectedTitle)).toBeInTheDocument();
      expect(screen.getByText(expectedState)).toBeInTheDocument();
      expect(screen.getByText("ralphx-general-explorer")).toBeInTheDocument();
      expect(screen.getByText("Verify the exact attempt.")).toBeInTheDocument();
    },
  );
});
