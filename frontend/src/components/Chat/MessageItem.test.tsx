/**
 * MessageItem.test.tsx - Tests for MessageItem component
 *
 * Tests attachment rendering integration with MessageAttachments component
 */

import { afterEach, describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { MessageItem, MessageMeta } from "./MessageItem";
import {
  makeContentText,
  makeContentToolUse,
  makeMessageAttachment,
  makeMessageItemProps,
  makeToolCall,
} from "./__tests__/chatRenderFixtures";

function renderMessageItem(ui: ReactElement) {
  return render(<TooltipProvider delayDuration={0}>{ui}</TooltipProvider>);
}

afterEach(() => {
  vi.useRealTimers();
});

describe("MessageItem - Attachment Integration", () => {
  const baseProps = makeMessageItemProps({
    role: "user",
  });

  const mockAttachments = [
    makeMessageAttachment({ id: "att-1", fileName: "test.txt" }),
    makeMessageAttachment({
      id: "att-2",
      fileName: "image.png",
      fileSize: 2048,
      mimeType: "image/png",
    }),
  ];

  it("renders MessageAttachments for user messages with attachments", () => {
    renderMessageItem(
      <MessageItem {...baseProps} role="user" attachments={mockAttachments} />
    );

    // MessageAttachments should render chips with data-testid="attachment-chip"
    const chips = screen.getAllByTestId("attachment-chip");
    expect(chips).toHaveLength(2);

    // Verify file names are displayed
    expect(screen.getByText("test.txt")).toBeInTheDocument();
    expect(screen.getByText("image.png")).toBeInTheDocument();
  });

  it("does NOT render MessageAttachments for user messages without attachments", () => {
    renderMessageItem(<MessageItem {...baseProps} role="user" />);

    // No attachment chips should be present
    const chips = screen.queryAllByTestId("attachment-chip");
    expect(chips).toHaveLength(0);
  });

  it("does NOT render MessageAttachments for user messages with empty attachments array", () => {
    renderMessageItem(<MessageItem {...baseProps} role="user" attachments={[]} />);

    // No attachment chips should be present
    const chips = screen.queryAllByTestId("attachment-chip");
    expect(chips).toHaveLength(0);
  });

  it("does NOT render MessageAttachments for assistant messages even if attachments prop is passed", () => {
    renderMessageItem(
      <MessageItem
        {...baseProps}
        role="assistant"
        attachments={mockAttachments}
      />
    );

    // No attachment chips should be present for assistant messages
    const chips = screen.queryAllByTestId("attachment-chip");
    expect(chips).toHaveLength(0);
  });

  it("MessageAttachments appear above the text bubble for user messages", () => {
    const { container } = renderMessageItem(
      <MessageItem {...baseProps} role="user" attachments={mockAttachments} />
    );

    // Find the parent flex column container
    const flexColumn = container.querySelector(".flex.flex-col");
    expect(flexColumn).toBeInTheDocument();

    if (!flexColumn) {
      throw new Error("Flex column container not found");
    }

    // Get all children of the flex column
    const children = Array.from(flexColumn.children);

    // MessageAttachments should be first (index 0)
    const firstChild = children[0];
    expect(firstChild?.querySelector('[data-testid="attachment-chip"]')).toBeInTheDocument();

    // Text bubble should come after attachments
    const textBubble = children.find((child) =>
      child.textContent?.includes("Hello world")
    );
    expect(textBubble).toBeInTheDocument();

    // Verify attachments come before text bubble in DOM order
    const attachmentsIndex = children.indexOf(firstChild);
    const textBubbleIndex = textBubble ? children.indexOf(textBubble) : -1;
    expect(attachmentsIndex).toBeLessThan(textBubbleIndex);
  });

  it("works with content blocks rendering", () => {
    const contentBlocks = [makeContentText("First block"), makeContentText("Second block")];

    renderMessageItem(
      <MessageItem
        {...baseProps}
        role="user"
        contentBlocks={contentBlocks}
        attachments={mockAttachments}
      />
    );

    // Attachments should render
    const chips = screen.getAllByTestId("attachment-chip");
    expect(chips).toHaveLength(2);

    // Content blocks should also render
    expect(screen.getByText("First block")).toBeInTheDocument();
    expect(screen.getByText("Second block")).toBeInTheDocument();
  });

  it("works with legacy rendering (toolCalls + text)", () => {
    const toolCalls = [
      makeToolCall("read_file", {
        id: "call-1",
        arguments: { path: "test.txt" },
        result: "file content",
      }),
    ];

    renderMessageItem(
      <MessageItem
        {...baseProps}
        role="assistant"
        toolCalls={toolCalls}
        attachments={mockAttachments}
      />
    );

    // For assistant messages, attachments should NOT render
    const chips = screen.queryAllByTestId("attachment-chip");
    expect(chips).toHaveLength(0);

    // Tool calls should render (we can check for tool call indicator presence)
    expect(screen.getByText("read_file")).toBeInTheDocument();
  });

  it("renders provider metadata for assistant messages when available", () => {
    renderMessageItem(
      <MessageItem
        {...baseProps}
        role="assistant"
        providerHarness="codex"
        providerSessionId="thread-codex-1234"
        upstreamProvider="openai"
        effectiveModelId="gpt-5.4"
        effectiveEffort="high"
        inputTokens={120}
        outputTokens={40}
        cacheCreationTokens={5}
        cacheReadTokens={8}
        estimatedUsd={0.42}
      />
    );

    expect(screen.getByTestId("message-provider-meta")).toBeInTheDocument();
    const badge = screen.getByTestId("message-provider-badge");
    expect(badge).toHaveTextContent("Codex");
    expect(screen.getByTestId("message-model-effort")).toHaveTextContent("gpt-5.4 · high");
    expect(badge).toHaveAttribute(
      "title",
      "Harness: Codex • Upstream: openai • Session ref: thread-codex... • gpt-5.4 · high • Input: 120 • Output: 40 • Cache: 13 • Est. cost: $0.42",
    );
  });
});

describe("MessageItem - copy affordance", () => {
  it("renders an always-visible copy button next to the assistant timestamp", () => {
    renderMessageItem(<MessageItem {...makeMessageItemProps({ role: "assistant", content: "Hello world" })} />);

    const meta = screen.getByTestId("message-meta");
    const copyButton = screen.getByTestId("message-copy-button");

    expect(meta).toContainElement(copyButton);
    expect(copyButton).toHaveAttribute("aria-label", "Copy message");
  });

  it("renders the same inline copy button for user messages", () => {
    renderMessageItem(<MessageItem {...makeMessageItemProps({ role: "user", content: "Hello world" })} />);

    expect(screen.getByTestId("message-meta")).toContainElement(
      screen.getByTestId("message-copy-button")
    );
  });

  it("shows a tooltip on the inline copy button", async () => {
    const user = userEvent.setup();
    renderMessageItem(<MessageItem {...makeMessageItemProps({ role: "assistant", content: "Hello world" })} />);

    await user.hover(screen.getByTestId("message-copy-button"));

    expect(await screen.findByRole("tooltip")).toHaveTextContent("Copy message");
  });

  it("copies MessageMeta text and reflects the copied state", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    renderMessageItem(
      <MessageMeta
        createdAt="2026-04-18T10:00:00Z"
        copyableText="Copy this streamed block"
      />
    );

    const copyButton = screen.getByTestId("message-copy-button");
    await user.click(copyButton);

    expect(writeText).toHaveBeenCalledWith("Copy this streamed block");
    expect(copyButton).toHaveAttribute("aria-label", "Copied");
  });

  it("keeps MessageMeta copy failures silent", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockRejectedValue(new Error("clipboard denied"));
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });

    renderMessageItem(
      <MessageMeta
        createdAt="2026-04-18T10:00:00Z"
        copyableText="Cannot copy this"
      />
    );

    const copyButton = screen.getByTestId("message-copy-button");
    await user.click(copyButton);

    expect(writeText).toHaveBeenCalledWith("Cannot copy this");
    expect(copyButton).toHaveAttribute("aria-label", "Copy message");
  });
});

describe("MessageItem - timestamp display", () => {
  it("renders human-diff timestamp text with the absolute timestamp as a native title", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 3, 25, 16, 33, 0));
    const createdAt = new Date(2026, 3, 25, 14, 33, 0).toISOString();

    renderMessageItem(
      <MessageItem {...makeMessageItemProps({ createdAt, content: "Hello world" })} />
    );

    const timestamp = screen.getByText("2 hours ago");
    expect(timestamp).toHaveAttribute("title", "Apr 25, 2026, 2:33 PM");
  });
});

describe("MessageItem - list spacing", () => {
  it("removes trailing bottom margin for the last rendered message", () => {
    const { container } = renderMessageItem(
      <MessageItem
        role="assistant"
        content="Last message"
        createdAt="2026-04-18T10:00:00Z"
        isLastInList={true}
      />
    );

    const wrapper = container.firstElementChild;
    expect(wrapper).toHaveClass("mb-0");
    expect(wrapper).not.toHaveClass("mb-5");
  });

  it("keeps standard bottom margin for non-terminal messages", () => {
    const { container } = renderMessageItem(
      <MessageItem
        role="assistant"
        content="Middle message"
        createdAt="2026-04-18T10:00:00Z"
      />
    );

    const wrapper = container.firstElementChild;
    expect(wrapper).toHaveClass("mb-5");
  });
});

describe("MessageItem - Child tool call suppression for Task/Agent spawns", () => {
  const createdAt = new Date().toISOString();

  it("suppresses child tool_use blocks that belong to a Task result", () => {
    // A message with a Task tool call that has child tool calls in its result
    const childToolUseId = "child-toolu-001";
    const contentBlocks = [
      makeContentToolUse("Task", {
        id: "task-toolu-001",
        arguments: { description: "Explore files", subagent_type: "Explore" },
        result: [
          { type: "tool_use", id: childToolUseId, name: "Glob", input: { pattern: "**/*.ts" } },
          { type: "tool_result", tool_use_id: childToolUseId, content: ["file1.ts"] },
        ],
      }),
      makeContentToolUse("Glob", {
        id: childToolUseId,
        arguments: { pattern: "**/*.ts" },
        result: ["file1.ts"],
      }),
    ];

    const { container } = renderMessageItem(
      <MessageItem role="assistant" content="" createdAt={createdAt} contentBlocks={contentBlocks} />
    );

    // The Task card renders (TaskToolCallCard)
    expect(container.querySelector('[data-testid="task-tool-call-card"]')).toBeInTheDocument();

    // The child Glob tool call should NOT render as a top-level card
    // Only one tool-call-indicator wrapper should exist (the Task one delegated to TaskToolCallCard)
    const allToolIndicators = container.querySelectorAll('[data-testid="tool-call-indicator"]');
    expect(allToolIndicators).toHaveLength(0); // Task goes to TaskToolCallCard, not generic indicator

    // The Glob card should NOT appear at top level (it's nested inside the Task result)
    const taskCards = container.querySelectorAll('[data-testid="task-tool-call-card"]');
    expect(taskCards).toHaveLength(1); // Only the Task card at top level
  });

  it("suppresses child tool_use blocks that belong to an Agent result", () => {
    const childToolUseId = "child-toolu-agent-001";
    const contentBlocks = [
      makeContentToolUse("Agent", {
        id: "agent-toolu-001",
        arguments: { description: "Research code", subagent_type: "general-purpose" },
        result: [
          { type: "tool_use", id: childToolUseId, name: "Grep", input: { pattern: "useState" } },
          { type: "tool_result", tool_use_id: childToolUseId, content: "found 5 matches" },
        ],
      }),
      makeContentToolUse("Grep", {
        id: childToolUseId,
        arguments: { pattern: "useState" },
        result: "found 5 matches",
      }),
    ];

    const { container } = renderMessageItem(
      <MessageItem role="assistant" content="" createdAt={createdAt} contentBlocks={contentBlocks} />
    );

    // Agent card renders as TaskToolCallCard
    expect(container.querySelector('[data-testid="task-tool-call-card"]')).toBeInTheDocument();

    // Grep child tool call should NOT appear at top level
    const taskCards = container.querySelectorAll('[data-testid="task-tool-call-card"]');
    expect(taskCards).toHaveLength(1); // Only the Agent card at top level
  });

  it("does NOT suppress tool_use blocks that are not nested in Task/Agent results", () => {
    // Two independent tool calls: one Read, one Bash — neither is a Task/Agent spawn
    const contentBlocks = [
      makeContentToolUse("read", {
        id: "read-001",
        arguments: { file_path: "/src/main.ts" },
        result: "file content",
      }),
      makeContentToolUse("custom_tool", {
        id: "bash-001",
        arguments: { command: "ls" },
        result: "file1\nfile2",
      }),
    ];

    const { container } = renderMessageItem(
      <MessageItem role="assistant" content="" createdAt={createdAt} contentBlocks={contentBlocks} />
    );

    // Both tool calls should render at top level (they are not children of any Task/Agent)
    const indicators = container.querySelectorAll('[data-testid="tool-call-indicator"]');
    expect(indicators.length).toBeGreaterThanOrEqual(1);
  });

  it("collects both tool_use and tool_result IDs from Agent result for suppression", () => {
    // Verify that both the tool_use ID and tool_result's tool_use_id are suppressed
    const childId = "child-abc";
    const contentBlocks = [
      makeContentToolUse("Agent", {
        id: "agent-toolu-002",
        arguments: { description: "Plan work", subagent_type: "Plan" },
        result: [
          { type: "tool_use", id: childId, name: "Read", input: { file_path: "/foo.ts" } },
          { type: "tool_result", tool_use_id: childId, content: "file content" },
        ],
      }),
      // The child tool_use appears again at top level (as emitted by stream)
      makeContentToolUse("Read", {
        id: childId,
        arguments: { file_path: "/foo.ts" },
        result: "file content",
      }),
    ];

    const { container } = renderMessageItem(
      <MessageItem role="assistant" content="" createdAt={createdAt} contentBlocks={contentBlocks} />
    );

    // Only the Agent (Task) card should render — child Read is suppressed
    expect(container.querySelector('[data-testid="task-tool-call-card"]')).toBeInTheDocument();

    // No top-level generic indicators (the suppressed child would have been one)
    // Task card = 1, child tool = 0 at top level
    const taskCards = container.querySelectorAll('[data-testid="task-tool-call-card"]');
    expect(taskCards).toHaveLength(1);
  });

  it("renders non-suppressed tool calls alongside Agent card", () => {
    // A message with an Agent call AND an independent (non-child) tool call
    const contentBlocks = [
      makeContentToolUse("Agent", {
        id: "agent-toolu-003",
        arguments: { description: "Explore code", subagent_type: "Explore" },
        result: [
          { type: "tool_use", id: "child-nested", name: "Glob", input: { pattern: "**/*.ts" } },
          { type: "tool_result", tool_use_id: "child-nested", content: [] },
        ],
      }),
      // Independent tool call (NOT a child of the Agent result)
      makeContentToolUse("custom_standalone_tool", {
        id: "independent-001",
        arguments: { key: "value" },
        result: "ok",
      }),
    ];

    const { container } = render(
      <MessageItem role="assistant" content="" createdAt={createdAt} contentBlocks={contentBlocks} />
    );

    // Agent card renders
    expect(container.querySelector('[data-testid="task-tool-call-card"]')).toBeInTheDocument();
    // Independent tool renders as generic indicator
    expect(container.querySelector('[data-testid="tool-call-indicator"]')).toBeInTheDocument();
  });
});

describe("MessageItem - persisted delegation replay", () => {
  it("renders one delegated task card from delegate_start plus delegate_wait content blocks", async () => {
    const user = userEvent.setup();
    const createdAt = new Date().toISOString();
    const contentBlocks = [
      makeContentToolUse("delegate_start", {
        id: "toolu-delegate-start",
        arguments: {
          agent_name: "ralphx-execution-reviewer",
          prompt: "Review the patch",
          harness: "codex",
          model: "gpt-5.4",
        },
        result: [{
          type: "text",
          text: JSON.stringify({
            job_id: "job-123",
            status: "running",
          }),
        }],
      }),
      makeContentToolUse("delegate_wait", {
        id: "toolu-delegate-wait",
        arguments: {
          job_id: "job-123",
        },
        result: [{
          type: "text",
          text: JSON.stringify({
            job_id: "job-123",
            status: "completed",
            content: "Delegated review finished",
            delegated_status: {
              latest_run: {
                harness: "codex",
                effective_model_id: "gpt-5.4",
                logical_effort: "high",
                input_tokens: 120,
                output_tokens: 45,
              },
            },
          }),
        }],
      }),
    ];

    const { container } = renderMessageItem(
      <MessageItem
        role="assistant"
        content=""
        createdAt={createdAt}
        contentBlocks={contentBlocks}
      />,
    );

    const taskCards = container.querySelectorAll('[data-testid="task-tool-call-card"]');
    expect(taskCards).toHaveLength(1);
    expect(screen.getByText("ralphx-execution-reviewer")).toBeInTheDocument();
    expect(screen.getByText("Codex")).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: /delegated task: ralphx-execution-reviewer/i }),
    );
    expect(screen.getByText("Delegated review finished")).toBeInTheDocument();
  });

  it("renders one delegated task card from namespaced delegate_start plus delegate_wait content blocks", () => {
    const createdAt = new Date().toISOString();
    const contentBlocks = [
      makeContentToolUse("ralphx::delegate_start", {
        id: "toolu-delegate-start",
        arguments: {
          agent_name: "ralphx-plan-critic-completeness",
        },
        result: [{
          type: "text",
          text: JSON.stringify({
            job_id: "job-123",
            status: "running",
          }),
        }],
      }),
      makeContentToolUse("ralphx::delegate_wait", {
        id: "toolu-delegate-wait",
        arguments: {
          job_id: "job-123",
        },
        result: [{
          type: "text",
          text: JSON.stringify({
            job_id: "job-123",
            status: "completed",
            content: "Critic artifact published",
          }),
        }],
      }),
    ];

    const { container } = renderMessageItem(
      <MessageItem
        role="assistant"
        content=""
        createdAt={createdAt}
        contentBlocks={contentBlocks}
      />,
    );

    expect(container.querySelectorAll('[data-testid="task-tool-call-card"]')).toHaveLength(1);
    expect(container.querySelectorAll('[data-testid="tool-call-indicator"]')).toHaveLength(0);
    expect(screen.getByText("ralphx-plan-critic-completeness")).toBeInTheDocument();
  });

  it("renders a delegated task card for a standalone namespaced delegate_wait block", () => {
    const createdAt = new Date().toISOString();
    const contentBlocks = [
      makeContentToolUse("ralphx::delegate_wait", {
        id: "toolu-delegate-wait-only",
        arguments: {
          job_id: "job-789",
        },
        result: [{
          type: "text",
          text: JSON.stringify({
            job_id: "job-789",
            status: "completed",
            agent_name: "ralphx-plan-critic-completeness",
            content: "Critic artifact published",
          }),
        }],
      }),
    ];

    const { container } = renderMessageItem(
      <MessageItem
        role="assistant"
        content=""
        createdAt={createdAt}
        contentBlocks={contentBlocks}
      />,
    );

    expect(container.querySelectorAll('[data-testid="task-tool-call-card"]')).toHaveLength(1);
    expect(container.querySelectorAll('[data-testid="tool-call-indicator"]')).toHaveLength(0);
    expect(screen.getByText("ralphx-plan-critic-completeness")).toBeInTheDocument();
  });
});

describe("MessageItem - Empty content guard (legacy rendering path)", () => {
  const createdAt = new Date().toISOString();

  it("does NOT render TextBubble for assistant with empty content", () => {
    const { container } = renderMessageItem(
      <MessageItem role="assistant" content="" createdAt={createdAt} />
    );

    // No bubble element should appear
    const bubble = container.querySelector(".rounded-xl");
    expect(bubble).not.toBeInTheDocument();
  });

  it("does NOT render TextBubble for assistant with whitespace-only content", () => {
    const { container } = renderMessageItem(
      <MessageItem role="assistant" content="   " createdAt={createdAt} />
    );

    const bubble = container.querySelector(".rounded-xl");
    expect(bubble).not.toBeInTheDocument();
  });

  it("does NOT render TextBubble for assistant with newline-only content", () => {
    // Use curly braces so JSX treats the value as a JS expression (escape sequences)
    const { container } = renderMessageItem(
      <MessageItem role="assistant" content={"\n\t  \n"} createdAt={createdAt} />
    );

    const bubble = container.querySelector(".rounded-xl");
    expect(bubble).not.toBeInTheDocument();
  });

  it("renders TextBubble for assistant with non-empty content", () => {
    renderMessageItem(
      <MessageItem role="assistant" content="Hello there" createdAt={createdAt} />
    );

    expect(screen.getByText("Hello there")).toBeInTheDocument();
  });

  it("renders TextBubble for user even when content is empty (user always shows)", () => {
    const { container } = renderMessageItem(
      <MessageItem role="user" content="" createdAt={createdAt} />
    );

    // User bubbles use the same TextBubble — the guard only skips assistant empty bubbles
    const bubble = container.querySelector(".rounded-xl");
    expect(bubble).toBeInTheDocument();
  });

  it("renders tool calls alongside empty assistant content (no text bubble, but tool cards show)", () => {
    // Use a tool name not in the widget registry so generic ToolCallIndicator renders
    const toolCalls = [
      makeToolCall("read_file", {
        id: "tc-1",
        arguments: { path: "/foo.ts" },
        result: "content",
      }),
    ];
    const { container } = renderMessageItem(
      <MessageItem role="assistant" content="" createdAt={createdAt} toolCalls={toolCalls} />
    );

    // Generic ToolCallIndicator (data-testid="tool-call-indicator") should render
    expect(container.querySelector('[data-testid="tool-call-indicator"]')).toBeInTheDocument();
    // But no TextBubble (.rounded-xl) for the empty content
    const textBubbles = container.querySelectorAll(".rounded-xl");
    // Only the tool call card renders, no text bubble
    // ToolCallIndicator uses rounded-lg, TextBubble uses rounded-xl
    expect(textBubbles).toHaveLength(0);
  });
});
