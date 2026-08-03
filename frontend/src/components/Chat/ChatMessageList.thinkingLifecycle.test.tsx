import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ChatMessageList } from "./ChatMessageList";
import {
  buildLiveTranscriptRows,
  liveThinkingGroupKey,
  synchronizeThinkingGroupExpansion,
} from "./ChatMessageList.liveRows";
import { createWrapper } from "@/test/store-utils";
import type { StreamingContentBlock } from "@/types/streaming-task";
import type { ChatMessageData } from "./ChatMessageList";

vi.mock("@/hooks/useMessageAttachments", () => ({
  useMessageAttachments: () => ({ data: new Map() }),
}));

vi.mock("./tool-widgets/ThinkingWidget", () => ({
  ThinkingWidget: ({ text }: { text: string }) => <div data-testid="thinking-content">{text}</div>,
}));

const defaultProps = {
  messages: [],
  conversationId: "thinking-lifecycle",
  failedRun: null,
  onDismissFailedRun: vi.fn(),
  isSending: false,
  isAgentRunning: true,
  streamingToolCalls: [],
  streamingTasks: new Map(),
};

function thinking(blockIndex: number, text: string, isSettled = false): StreamingContentBlock {
  return { type: "thinking", blockIndex, text, isSettled, durationMs: 2_000 };
}

function renderList(
  streamingContentBlocks: StreamingContentBlock[],
  messages: ChatMessageData[] = [],
) {
  return render(<ChatMessageList {...defaultProps} messages={messages} streamingContentBlocks={streamingContentBlocks} />, {
    wrapper: createWrapper(),
  });
}

describe("ChatMessageList thinking lifecycle", () => {
  it("keeps a live thinking group expanded when it settles", () => {
    const { rerender } = renderList([thinking(1, "first thought"), thinking(2, "second thought")]);

    expect(screen.getByTestId("thinking-content")).toHaveTextContent("first thought");
    expect(screen.getByTestId("thinking-content")).toHaveTextContent("second thought");

    rerender(
      <ChatMessageList
        {...defaultProps}
        streamingContentBlocks={[thinking(1, "first thought", true), thinking(2, "second thought", true)]}
      />,
    );

    expect(screen.getByTestId("thinking-content")).toHaveTextContent("first thought");
    expect(screen.getByTestId("thinking-content")).toHaveTextContent("second thought");
  });

  it("keeps an empty live thinking block pill-only while showing token progress", () => {
    renderList([{ type: "thinking", blockIndex: 9, text: "", estimatedTokens: 2_000 }]);

    expect(screen.getByRole("button", { name: /Agent thinking… · ~2,000 tokens/ })).toBeInTheDocument();
    expect(screen.queryByTestId("thinking-content")).not.toBeInTheDocument();
  });

  it("keeps a user's collapsed or expanded thinking choice across later deltas", () => {
    const { rerender } = renderList([thinking(1, "live thought")]);

    fireEvent.click(screen.getByTestId("thinking-group-toggle"));
    expect(screen.queryByText("live thought")).not.toBeInTheDocument();

    rerender(
      <ChatMessageList {...defaultProps} streamingContentBlocks={[thinking(1, "live thought updated")]} />,
    );
    expect(screen.queryByText("live thought updated")).not.toBeInTheDocument();

    rerender(
      <ChatMessageList {...defaultProps} streamingContentBlocks={[thinking(1, "settled thought", true)]} />,
    );
    fireEvent.click(screen.getByTestId("thinking-group-toggle"));
    expect(screen.getByText("settled thought")).toBeInTheDocument();

    rerender(
      <ChatMessageList {...defaultProps} streamingContentBlocks={[thinking(1, "settled thought updated", true)]} />,
    );
    expect(screen.getByText("settled thought updated")).toBeInTheDocument();
  });

  it("keeps earlier thinking expanded when a newer group starts after visible tool activity", () => {
    const { rerender } = renderList([thinking(1, "first thought")]);

    expect(screen.getByText("first thought")).toBeInTheDocument();

    rerender(
      <ChatMessageList
        {...defaultProps}
        streamingContentBlocks={[
          thinking(1, "first thought"),
          { type: "tool_use", toolCall: { id: "tool-1", name: "Read", arguments: {} } },
          thinking(2, "second thought"),
        ]}
      />,
    );

    expect(screen.getByText("first thought")).toBeInTheDocument();
    expect(screen.getByText("second thought")).toBeInTheDocument();
  });

  it("renders consecutive live segments in one toggle and preserves a user collapse as segments append", () => {
    const { rerender } = renderList([thinking(1, "first"), thinking(2, "second")]);

    expect(screen.getAllByTestId("thinking-group-toggle")).toHaveLength(1);
    expect(screen.getByTestId("thinking-content")).toHaveTextContent("first");
    expect(screen.getByTestId("thinking-content")).toHaveTextContent("second");

    fireEvent.click(screen.getByTestId("thinking-group-toggle"));
    rerender(
      <ChatMessageList {...defaultProps} streamingContentBlocks={[
        thinking(1, "first"), thinking(2, "second"), thinking(3, "third"),
      ]} />,
    );

    expect(screen.queryByTestId("thinking-content")).not.toBeInTheDocument();
  });

  it("keeps the same Set when an equivalent delta needs no expansion change", () => {
    const rows = buildLiveTranscriptRows([thinking(1, "live thought")], new Map());
    const current = new Set([liveThinkingGroupKey(thinking(1, "live thought"), 0)]);

    expect(synchronizeThinkingGroupExpansion(current, rows, new Map())).toBe(current);
  });

  it.each(["claude", "codex", "other"])("renders a finalized persisted %s thinking run expanded", (providerHarness) => {
    const messages: ChatMessageData[] = [
      {
        id: "thinking-1", role: "assistant", content: "", createdAt: "2026-08-03T00:00:00Z",
        parentMessageId: "message-1", timelineSequence: 4, providerHarness,
        contentBlocks: [{ type: "thinking", text: "First persisted", durationMs: 1_000 }],
      },
      {
        id: "thinking-2", role: "assistant", content: "", createdAt: "2026-08-03T00:00:01Z",
        parentMessageId: "message-1", timelineSequence: 5, providerHarness,
        contentBlocks: [{ type: "thinking", text: "Second persisted", durationMs: 2_000 }],
      },
    ];

    renderList([], messages);

    expect(screen.getAllByTestId("thinking-group-toggle")).toHaveLength(1);
    expect(screen.getByTestId("thinking-content")).toHaveTextContent("First persisted");
    expect(screen.getByTestId("thinking-content")).toHaveTextContent("Second persisted");

    fireEvent.click(screen.getByTestId("thinking-group-toggle"));
    expect(screen.queryByTestId("thinking-content")).not.toBeInTheDocument();
  });
});
