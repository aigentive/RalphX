import { beforeEach, describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ToolCallIndicator, type ToolCall } from "./ToolCallIndicator";
import { makeToolCall } from "./__tests__/chatRenderFixtures";
import { chatApi } from "@/api/chat";

vi.mock("@/api/chat", () => ({
  chatApi: {
    getAgentMessageToolCallDetail: vi.fn(),
  },
}));

vi.mock("@/hooks/useChildSessionStatus", () => ({
  useChildSessionStatus: vi.fn(() => ({
    data: undefined,
    isLoading: false,
    isError: false,
    refetch: vi.fn(),
  })),
}));

/**
 * ToolCallIndicator tests.
 *
 * Tools with dedicated widgets (bash, read, grep, glob, step tools, context,
 * artifacts, reviews, proposals, merges, ideation) are routed to their widgets
 * by the registry in tool-widgets/registry.ts. These tests cover the GENERIC
 * fallback renderer — used for tools without a widget (update_task, add_task_note,
 * custom_tool, etc.) and for edit/write error fallback cases.
 */
describe("ToolCallIndicator", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("Project orchestration widgets", () => {
    it("hides completed routine project orchestration reads", () => {
      const toolCall: ToolCall = makeToolCall("mcp__ralphx__v1_get_ideation_status", {
        id: "call-project-status",
        result: { ok: true, status: "active" },
      });

      const { container } = render(<ToolCallIndicator toolCall={toolCall} />);

      expect(container).toBeEmptyDOMElement();
      expect(screen.queryByText("mcp__ralphx__v1_get_ideation_status")).not.toBeInTheDocument();
    });

    it("hides completed ideation prompt sends when no session id is available", () => {
      const toolCall: ToolCall = makeToolCall("mcp__ralphx__v1_send_ideation_message", {
        id: "call-send-ideation",
        result: { queuedAsPending: true, nextAction: "wait_for_resume" },
      });

      const { container } = render(<ToolCallIndicator toolCall={toolCall} />);

      expect(container).toBeEmptyDOMElement();
      expect(screen.queryByText("mcp__ralphx__v1_send_ideation_message")).not.toBeInTheDocument();
    });

    it("renders ideation prompt sends with a session id as the attached ideation card", async () => {
      const toolCall: ToolCall = makeToolCall("mcp__ralphx__v1_send_ideation_message", {
        id: "call-send-ideation",
        result: {
          status: "sent",
          session_id: "session-123",
          next_action: "poll_status",
        },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      expect(await screen.findByText("Ideation Session")).toBeInTheDocument();
      expect(await screen.findByText("Ideation run")).toBeInTheDocument();
      expect(await screen.findByText("Open Run")).toBeInTheDocument();
      expect(screen.queryByText("Ideation prompt sent")).not.toBeInTheDocument();
    });
  });

  describe("Rendering (generic fallback)", () => {
    it("renders collapsed by default", () => {
      const toolCall: ToolCall = makeToolCall("update_task", {
        id: "call-1",
        arguments: { task_id: "task-1" },
        result: { ok: true },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      expect(screen.getByText("update_task")).toBeInTheDocument();
      expect(screen.getByText("Updated task")).toBeInTheDocument();

      // Should NOT show details initially
      expect(screen.queryByTestId("tool-call-details")).not.toBeInTheDocument();
    });

    it("shows tool icon and chevron", () => {
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { data: "test" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      const toggle = screen.getByTestId("tool-call-toggle");
      expect(toggle).toBeInTheDocument();
    });

    it("applies custom className", () => {
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { data: "test" },
      });

      const { container } = render(
        <ToolCallIndicator toolCall={toolCall} className="custom-class" />
      );

      const indicator = container.querySelector(".custom-class");
      expect(indicator).toBeInTheDocument();
    });
  });

  describe("Interaction (generic fallback)", () => {
    it("loads full previewed tool results only after expansion", async () => {
      const user = userEvent.setup();
      vi.mocked(chatApi.getAgentMessageToolCallDetail).mockResolvedValueOnce({
        toolCall: makeToolCall("custom_tool", {
          id: "call-1",
          arguments: { target: "big-result" },
          result: "full line 1\nfull line 2\nfull line 3",
        }),
      });
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { target: "big-result" },
        result: "preview line 1\npreview line 2",
        resultPreviewTruncated: true,
        detailRef: {
          conversationId: "conv-1",
          messageId: "msg-1",
          toolCallId: "call-1",
        },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      expect(screen.getByTestId("tool-call-preview-card")).toBeInTheDocument();
      expect(chatApi.getAgentMessageToolCallDetail).not.toHaveBeenCalled();

      await user.click(screen.getByTestId("tool-call-toggle"));

      expect(chatApi.getAgentMessageToolCallDetail).toHaveBeenCalledWith({
        conversationId: "conv-1",
        messageId: "msg-1",
        toolCallId: "call-1",
      });
      expect(await screen.findByText(/full line 3/)).toBeInTheDocument();
      expect(screen.queryByText(/preview line 2/)).not.toBeInTheDocument();
    });

    it("shows unavailable text when preview detail returns empty", async () => {
      const user = userEvent.setup();
      vi.mocked(chatApi.getAgentMessageToolCallDetail).mockResolvedValueOnce(null);
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { file_path: "/tmp/big.txt" },
        result: "preview",
        resultPreviewTruncated: true,
        detailRef: {
          conversationId: "conv-1",
          messageId: "msg-1",
          toolCallId: "call-1",
        },
      });

      render(<ToolCallIndicator toolCall={toolCall} compact isStreaming />);
      await user.click(screen.getByTestId("tool-call-toggle"));

      expect(await screen.findByText("Full result is unavailable.")).toBeInTheDocument();
    });

    it("shows detail load errors for previewed tool results", async () => {
      const user = userEvent.setup();
      vi.mocked(chatApi.getAgentMessageToolCallDetail).mockRejectedValueOnce(
        new Error("detail endpoint failed")
      );
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { file_path: "/tmp/big.txt" },
        result: "preview",
        resultPreviewTruncated: true,
        detailRef: {
          conversationId: "conv-1",
          messageId: "msg-1",
          toolCallId: "call-1",
        },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);
      await user.click(screen.getByTestId("tool-call-toggle"));

      expect(await screen.findByText("detail endpoint failed")).toBeInTheDocument();
    });

    it("expands previewed results without fetching when no detail ref exists", async () => {
      const user = userEvent.setup();
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { pattern: "**/*.ts" },
        result: "preview only",
        resultPreviewTruncated: true,
      });

      render(<ToolCallIndicator toolCall={toolCall} />);
      await user.click(screen.getByTestId("tool-call-toggle"));

      expect(chatApi.getAgentMessageToolCallDetail).not.toHaveBeenCalled();
      expect(screen.getByText("preview only")).toBeInTheDocument();
    });

    it("keeps previewed registered tools on their specialized widget path", async () => {
      const user = userEvent.setup();
      vi.mocked(chatApi.getAgentMessageToolCallDetail).mockResolvedValueOnce({
        toolCall: makeToolCall("bash", {
          id: "call-bash",
          arguments: { command: "npm test", description: "Run tests" },
          result: "full line 1\nfull line 2\nfull line 3",
        }),
      });
      const toolCall: ToolCall = makeToolCall("bash", {
        id: "call-bash",
        arguments: { command: "npm test", description: "Run tests" },
        result: "preview line 1\npreview line 2",
        resultPreviewTruncated: true,
        detailRef: {
          conversationId: "conv-1",
          messageId: "msg-1",
          toolCallId: "call-bash",
        },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      expect(screen.queryByTestId("tool-call-preview-card")).not.toBeInTheDocument();
      expect(await screen.findByText("Run tests")).toBeInTheDocument();
      expect(screen.getByText(/preview line 2/)).toBeInTheDocument();
      expect(chatApi.getAgentMessageToolCallDetail).not.toHaveBeenCalled();

      await user.click(screen.getByText("Run tests"));

      expect(chatApi.getAgentMessageToolCallDetail).toHaveBeenCalledWith({
        conversationId: "conv-1",
        messageId: "msg-1",
        toolCallId: "call-bash",
      });
      expect(await screen.findByText(/full line 3/)).toBeInTheDocument();
    });

    it("expands when clicked", async () => {
      const user = userEvent.setup();
      const toolCall: ToolCall = makeToolCall("update_task", {
        id: "call-1",
        arguments: { task_id: "task-1" },
        result: { ok: true },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      const toggle = screen.getByTestId("tool-call-toggle");
      await user.click(toggle);

      // Details should now be visible
      expect(screen.getByTestId("tool-call-details")).toBeInTheDocument();

      // Should show arguments label
      expect(screen.getByText("Arguments")).toBeInTheDocument();

      // Should show result label
      expect(screen.getByText("Result")).toBeInTheDocument();
    });

    it("collapses when clicked again", async () => {
      const user = userEvent.setup();
      const toolCall: ToolCall = makeToolCall("add_task_note", {
        id: "call-1",
        arguments: { task_id: "task-abc", note: "test" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      const toggle = screen.getByTestId("tool-call-toggle");

      // Expand
      await user.click(toggle);
      expect(screen.getByTestId("tool-call-details")).toBeInTheDocument();

      // Collapse
      await user.click(toggle);
      expect(screen.queryByTestId("tool-call-details")).not.toBeInTheDocument();
    });

    it("has correct aria-expanded attribute", async () => {
      const user = userEvent.setup();
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { data: "test" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      const toggle = screen.getByTestId("tool-call-toggle");

      // Initially collapsed
      expect(toggle).toHaveAttribute("aria-expanded", "false");

      // After click, expanded
      await user.click(toggle);
      expect(toggle).toHaveAttribute("aria-expanded", "true");
    });
  });

  describe("Summary generation", () => {
    it("shows file path for write tool", () => {
      const toolCall: ToolCall = makeToolCall("write", {
        id: "call-1",
        arguments: { file_path: "/app/config.json" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);
      expect(screen.getByText("write")).toBeInTheDocument();
      expect(screen.getByText(/\/app\/config.json/i)).toBeInTheDocument();
    });

    it("shows file path for edit tool", () => {
      const toolCall: ToolCall = makeToolCall("edit", {
        id: "call-1",
        arguments: { file_path: "/src/main.rs" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);
      expect(screen.getByText("edit")).toBeInTheDocument();
      expect(screen.getByText(/\/src\/main.rs/i)).toBeInTheDocument();
    });

    it("shows update_task summary", () => {
      const toolCall: ToolCall = makeToolCall("update_task", {
        id: "call-1",
        arguments: { task_id: "task-789" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);
      expect(screen.getByText("update_task")).toBeInTheDocument();
      expect(screen.getByText(/Updated task/i)).toBeInTheDocument();
    });

    it("shows add_task_note summary", () => {
      const toolCall: ToolCall = makeToolCall("add_task_note", {
        id: "call-1",
        arguments: { task_id: "task-abc", note: "Fixed the bug" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);
      expect(screen.getByText("add_task_note")).toBeInTheDocument();
      expect(screen.getByText(/Added note/i)).toBeInTheDocument();
    });

    it("shows tool name for unknown tools", () => {
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { foo: "bar" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);
      expect(screen.getByText("custom_tool")).toBeInTheDocument();
      // Shows formatted tool name as summary (underscores replaced with spaces)
      expect(screen.getByText("custom tool")).toBeInTheDocument();
    });
  });

  describe("Expanded details", () => {
    it("displays tool name badge in collapsed view", () => {
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { action: "test" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      // Tool name badge should be visible in collapsed view
      expect(screen.getByText("custom_tool")).toBeInTheDocument();
    });

    it("displays formatted arguments when expanded", async () => {
      const user = userEvent.setup();
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: {
          title: "Test Task",
          description: "A test description",
          priority: "high",
        },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      await user.click(screen.getByTestId("tool-call-toggle"));

      expect(screen.getByText("Arguments")).toBeInTheDocument();

      // JSON should be formatted
      const detailsContainer = screen.getByTestId("tool-call-details");
      expect(detailsContainer.textContent).toContain("Test Task");
      expect(detailsContainer.textContent).toContain("high");
    });

    it("displays result when present", async () => {
      const user = userEvent.setup();
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { action: "get" },
        result: "hello\n",
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      await user.click(screen.getByTestId("tool-call-toggle"));

      expect(screen.getByText("Result")).toBeInTheDocument();
      const detailsContainer = screen.getByTestId("tool-call-details");
      expect(detailsContainer.textContent).toContain("hello");
    });

    it("does not display result section when result is undefined", async () => {
      const user = userEvent.setup();
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { action: "get" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      await user.click(screen.getByTestId("tool-call-toggle"));

      expect(screen.queryByText("Result")).not.toBeInTheDocument();
    });
  });

  describe("Error handling", () => {
    it("displays error indicator when tool call failed", () => {
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { action: "fail" },
        error: "Command not found",
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      expect(screen.getByText("Failed")).toBeInTheDocument();
    });

    it("displays error message in expanded view", async () => {
      const user = userEvent.setup();
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { path: "/nonexistent.txt" },
        error: "File not found: /nonexistent.txt",
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      await user.click(screen.getByTestId("tool-call-toggle"));

      expect(screen.getByText("Error")).toBeInTheDocument();
      const detailsContainer = screen.getByTestId("tool-call-details");
      expect(detailsContainer.textContent).toContain("File not found");
    });

    it("does not display result when error is present", async () => {
      const user = userEvent.setup();
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { action: "list" },
        result: "should not show",
        error: "Some error occurred",
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      await user.click(screen.getByTestId("tool-call-toggle"));

      // Should show error, not result
      expect(screen.getByText("Error")).toBeInTheDocument();
      expect(screen.queryByText("Result")).not.toBeInTheDocument();
    });

    it("applies error styling when tool call failed", () => {
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { action: "fail" },
        error: "Command failed",
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      const indicator = screen.getByTestId("tool-call-indicator");
      // Error state uses a red-tinted background (design token)
      expect(indicator).toHaveStyle({ backgroundColor: "var(--status-error-muted)" });
    });
  });

  describe("Accessibility", () => {
    it("has accessible label for toggle button", () => {
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { data: "test" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      const toggle = screen.getByTestId("tool-call-toggle");
      expect(toggle).toHaveAttribute("aria-label");
      expect(toggle.getAttribute("aria-label")).toContain("custom_tool");
      expect(toggle.getAttribute("aria-label")).toContain("expand");
    });

    it("updates aria-label when expanded", async () => {
      const user = userEvent.setup();
      const toolCall: ToolCall = makeToolCall("custom_tool", {
        id: "call-1",
        arguments: { data: "test" },
      });

      render(<ToolCallIndicator toolCall={toolCall} />);

      const toggle = screen.getByTestId("tool-call-toggle");

      // Initially should say "expand"
      expect(toggle.getAttribute("aria-label")).toContain("expand");

      // After click, should say "collapse"
      await user.click(toggle);
      expect(toggle.getAttribute("aria-label")).toContain("collapse");
    });
  });
});
