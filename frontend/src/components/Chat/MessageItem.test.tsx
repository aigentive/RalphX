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

vi.mock("./tool-widgets/ThinkingWidget", () => ({
  ThinkingWidget: ({ text }: { text: string }) => <div data-testid="thinking-content">{text}</div>,
}));

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

  it("renders structured composer references for user messages", () => {
    renderMessageItem(
      <MessageItem
        {...baseProps}
        role="user"
        composerReferences={{
          folderReferences: [
            {
              id: "folder-1",
              folderPath: "/work/brand-kit",
              displayName: "brand-kit",
            },
          ],
          projectReferences: [{ path: "src/main.ts", kind: "file" }],
          artifactReferences: [],
          integrationReferences: [
            {
              provider: "atlassian",
              kind: "jira",
              id: "RX-42",
              key: "RX-42",
              title: "Fix composer references",
              url: "https://example.atlassian.net/browse/RX-42",
            },
            {
              provider: "atlassian",
              kind: "confluence",
              id: "123",
              title: "Release Notes",
            },
          ],
        }}
      />
    );

    expect(screen.getByTestId("message-reference-project:src/main.ts")).toHaveTextContent(
      "File",
    );
    expect(screen.getByTestId("message-reference-folder:folder-1")).toHaveTextContent(
      "brand-kit",
    );
    expect(screen.getByTestId("message-reference-integration:jira:RX-42")).toHaveTextContent(
      "Jira",
    );
    const jiraReference = screen.getByTestId(
      "message-reference-integration:jira:RX-42",
    );
    expect(jiraReference).toHaveTextContent("RX-42");
    // Ticket references render as in-app navigation buttons (they open the ticketing
    // view), not external links, so there is no href.
    expect(jiraReference.tagName).toBe("BUTTON");
    expect(jiraReference).not.toHaveAttribute("href");
    expect(jiraReference).toHaveClass("no-underline", "flex-wrap");
    expect(jiraReference).toHaveStyle({ textDecoration: "none" });
    expect(screen.getByText("Fix composer references")).toHaveClass("break-words");
    expect(
      screen.getByTestId("message-reference-integration:confluence:123"),
    ).toHaveTextContent("Confluence");
    expect(
      screen.getByTestId("message-reference-integration:confluence:123"),
    ).toHaveTextContent("Release Notes");
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

  it("aligns user attachments and references with the right-aligned text bubble", () => {
    renderMessageItem(
      <MessageItem
        {...baseProps}
        role="user"
        attachments={mockAttachments}
        composerReferences={{
          projectReferences: [{ path: "src/main.ts", kind: "file" }],
          integrationReferences: [],
          artifactReferences: [],
        }}
      />
    );

    expect(screen.getByTestId("text-bubble-user")).toHaveClass("self-end");
    expect(screen.getByTestId("message-attachment-list")).toHaveClass("self-end");
    expect(screen.getByTestId("message-reference-list")).toHaveClass("self-end", "justify-end");
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

  it("renders no thinking pill for a persisted thinking block with empty text", () => {
    renderMessageItem(
      <MessageItem
        {...baseProps}
        role="assistant"
        contentBlocks={[
          { type: "thinking", text: "  " },
          makeContentText("The answer remains visible."),
        ]}
      />,
    );

    expect(screen.queryByTestId("thinking-group-toggle")).not.toBeInTheDocument();
    expect(screen.getByText("The answer remains visible.")).toBeInTheDocument();
  });

  it("renders adjacent finalized thinking blocks expanded and keeps a deliberate collapse", async () => {
    const user = userEvent.setup();
    renderMessageItem(
      <MessageItem
        {...baseProps}
        role="assistant"
        contentBlocks={[
          { type: "thinking", text: "First thought", durationMs: 1_000 },
          { type: "thinking", text: "   " },
          { type: "thinking", text: "Second thought", durationMs: 2_000 },
        ]}
      />,
    );

    expect(screen.getAllByTestId("thinking-group-toggle")).toHaveLength(1);
    expect(screen.getByRole("button", { name: /Agent thought for 3s · 2 steps/ })).toBeInTheDocument();
    expect(screen.getByText(/First thought/)).toBeInTheDocument();
    expect(screen.getByText(/Second thought/)).toBeInTheDocument();
    await user.click(screen.getByTestId("thinking-group-toggle"));
    expect(screen.queryByText(/First thought/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Second thought/)).not.toBeInTheDocument();
  });

  it("keeps thinking adjacent across hidden child tool calls but splits it at visible tools", () => {
    const childToolUseId = "child-thinking-tool";
    const hiddenChildContentBlocks = [
      makeContentToolUse("Task", {
        id: "task-before-thinking",
        result: [{ type: "tool_use", id: childToolUseId }],
      }),
      { type: "thinking" as const, text: "First" },
      makeContentToolUse("Glob", { id: childToolUseId }),
      { type: "thinking" as const, text: "Second" },
    ];
    const { rerender } = renderMessageItem(
      <MessageItem {...baseProps} role="assistant" contentBlocks={hiddenChildContentBlocks} />,
    );

    expect(screen.getAllByTestId("thinking-group-toggle")).toHaveLength(1);

    rerender(
      <TooltipProvider delayDuration={0}>
        <MessageItem
          {...baseProps}
          role="assistant"
          contentBlocks={[
            { type: "thinking", text: "First" },
            makeContentToolUse("Read", { id: "visible-tool" }),
            { type: "thinking", text: "Second" },
          ]}
        />
      </TooltipProvider>,
    );

    expect(screen.getAllByTestId("thinking-group-toggle")).toHaveLength(2);
  });

  it("passes argument preview metadata from content block tool uses into diff widgets", async () => {
    const user = userEvent.setup();
    const contentBlocks = [
      {
        type: "tool_use" as const,
        id: "tool-edit-preview",
        name: "edit",
        arguments: { file_path: "src/app.ts" },
        argumentsPreviewTruncated: true,
        argumentsPreviewOriginalBytes: 2400,
        argumentsPreviewLineCount: 120,
        argumentsPreviewOmittedLines: 114,
        diffPreview: {
          filePath: "src/app.ts",
          language: "typescript",
          oldTotalLines: 0,
          newTotalLines: 1,
          isBinary: false,
          hunks: [
            {
              oldStart: 1,
              oldLines: 0,
              newStart: 1,
              newLines: 1,
              header: "@@ -1,0 +1,1 @@",
              lines: [
                {
                  kind: "addition" as const,
                  content: "export const value = 1;",
                  oldLineNum: null,
                  newLineNum: 1,
                },
              ],
            },
          ],
        },
        detailRef: {
          conversationId: "conv-1",
          messageId: "msg-1",
          toolCallId: "tool-edit-preview",
        },
      },
    ];

    renderMessageItem(
      <MessageItem
        {...baseProps}
        role="assistant"
        contentBlocks={contentBlocks}
      />
    );

    await user.click(screen.getByRole("button", {
      name: "Agent called 1 tool and edited 1 file. Expand tool details.",
    }));
    expect(await screen.findByTestId("diff-tool-call-preview-diff")).toHaveTextContent(
      "export const value = 1;"
    );
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

  it("collapses ordinary consecutive tool_use blocks, including Bash widgets, instead of rendering them raw", async () => {
    const user = userEvent.setup();
    const contentBlocks = [
      makeContentToolUse("bash", {
        id: "bash-001",
        arguments: { command: "npm test" },
        result: "file1\nfile2",
      }),
      makeContentToolUse("custom_tool", {
        id: "custom-001",
        arguments: { command: "ls" },
        result: "ok",
      }),
    ];

    const { container } = renderMessageItem(
      <MessageItem role="assistant" content="" createdAt={createdAt} contentBlocks={contentBlocks} />
    );

    expect(screen.getByRole("button", { name: "Agent called 2 tools. Expand tool details." })).toBeInTheDocument();
    expect(screen.queryByText("npm test")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Agent called 2 tools. Expand tool details." }));
    expect(await screen.findAllByText("npm test")).not.toHaveLength(0);
    const indicators = container.querySelectorAll('[data-testid="tool-call-indicator"]');
    expect(indicators.length).toBeGreaterThanOrEqual(1);
  });

  it.each([
    ["Claude", "Write", "Edit", "mcp__ralphx__delegate_start"],
    ["Codex", "write", "edit", "ralphx::delegate_start"],
  ])("renders the same mixed activity summary and promoted delegate for %s", async (_provider, write, edit, delegate) => {
    const user = userEvent.setup();
    const contentBlocks = [
      makeContentToolUse(write, {
        id: "create-file",
        arguments: { file_path: "src/new.ts" },
        diffContext: { filePath: "src/new.ts", oldFileExists: false },
      }),
      makeContentToolUse(edit, {
        id: "edit-file",
        arguments: { file_path: "src/existing.ts", old_string: "a", new_string: "b" },
        diffContext: { filePath: "src/existing.ts", oldFileExists: true },
      }),
      makeContentToolUse(delegate, {
        id: "delegate-agent",
        arguments: { agent_name: "ralphx-general-explorer", prompt: "Inspect chat" },
        result: { job_id: "job-1", status: "running" },
      }),
    ];

    const { container } = renderMessageItem(
      <MessageItem role="assistant" content="" createdAt={createdAt} contentBlocks={contentBlocks} />,
    );

    const toggle = screen.getByRole("button", {
      name: "Agent called 3 tools, created 1 file, edited 1 file, and delegated 1 agent. Expand tool details.",
    });
    expect(toggle).toBeInTheDocument();
    expect(container.querySelectorAll('[data-testid="task-tool-call-card"]')).toHaveLength(1);
    expect(container.querySelectorAll('[data-testid="diff-tool-call-view"]')).toHaveLength(0);

    await user.click(toggle);
    expect(screen.getByRole("button", {
      name: "Agent called 3 tools, created 1 file, edited 1 file, and delegated 1 agent. Collapse tool details.",
    })).toBeInTheDocument();
    expect(container.querySelectorAll('[data-testid="task-tool-call-card"]')).toHaveLength(1);
  });

  it("passes structured preview path metadata from content blocks into tool calls", () => {
    const contentBlocks = [
      {
        type: "tool_use" as const,
        id: "custom-preview-paths",
        name: "custom_tool",
        arguments: { file_path: "/src/big.log" },
        result: { output: "preview" },
        resultPreviewTruncated: true,
        resultPreviewPaths: ["$.output"],
      },
    ];

    const { container } = renderMessageItem(
      <MessageItem role="assistant" content="" createdAt={createdAt} contentBlocks={contentBlocks} />
    );

    expect(container.querySelector('[data-testid="tool-call-preview-card"]')).toBeInTheDocument();
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

  it("renders non-suppressed tool calls alongside Agent card", async () => {
    const user = userEvent.setup();
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
    // Independent generic tool is collapsed, not suppressed.
    expect(screen.getByRole("button", { name: "Agent called 2 tools. Expand tool details." })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Agent called 2 tools. Expand tool details." }));
    expect(container.querySelector('[data-testid="tool-call-indicator"]')).toBeInTheDocument();
  });
});

describe("MessageItem - hydrated content block tool results", () => {
  const createdAt = "2026-04-10T07:00:00Z";

  it("hydrates a content-block tool widget result from the matching toolCalls entry", async () => {
    const toolCallId = "tool-ask-question";
    const contentBlocks = [
      makeContentToolUse("mcp__ralphx__ask_user_question", {
        id: toolCallId,
        arguments: { question: "Which area should we focus on?" },
      }),
    ];
    const toolCalls = [
      makeToolCall("mcp__ralphx__ask_user_question", {
        id: toolCallId,
        arguments: { question: "Which area should we focus on?" },
        result: {
          answers: [
            {
              id: "scope",
              request_id: "req-1",
              question: "Which area should we focus on?",
              options: [{ label: "Backend", value: "backend" }],
              selected_options: ["backend"],
              text: null,
              skipped: false,
            },
          ],
        },
      }),
    ];

    renderMessageItem(
      <MessageItem
        role="assistant"
        content=""
        createdAt={createdAt}
        contentBlocks={contentBlocks}
        toolCalls={toolCalls}
      />,
    );

    expect(await screen.findByText("Question answered")).toBeInTheDocument();
    expect(screen.getByText("Which area should we focus on?")).toBeInTheDocument();
    expect(screen.getByText("Backend")).toBeInTheDocument();
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
