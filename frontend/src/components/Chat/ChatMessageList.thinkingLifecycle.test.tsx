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

function renderList(streamingContentBlocks: StreamingContentBlock[]) {
  return render(<ChatMessageList {...defaultProps} streamingContentBlocks={streamingContentBlocks} />, {
    wrapper: createWrapper(),
  });
}

describe("ChatMessageList thinking lifecycle", () => {
  it("collapses a thinking group when it settles", () => {
    const { rerender } = renderList([thinking(1, "first thought")]);

    expect(screen.getByText("first thought")).toBeInTheDocument();

    rerender(
      <ChatMessageList {...defaultProps} streamingContentBlocks={[thinking(1, "first thought", true)]} />,
    );

    expect(screen.queryByText("first thought")).not.toBeInTheDocument();
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

  it("collapses an earlier thinking group when a newer group starts", () => {
    const { rerender } = renderList([thinking(1, "first thought")]);

    expect(screen.getByText("first thought")).toBeInTheDocument();

    rerender(
      <ChatMessageList
        {...defaultProps}
        streamingContentBlocks={[thinking(1, "first thought"), thinking(2, "second thought")]}
      />,
    );

    expect(screen.queryByText("first thought")).not.toBeInTheDocument();
    expect(screen.getByText("second thought")).toBeInTheDocument();
  });

  it("keeps the same Set when an equivalent delta needs no expansion change", () => {
    const rows = buildLiveTranscriptRows([thinking(1, "live thought")], new Map());
    const current = new Set([liveThinkingGroupKey(thinking(1, "live thought"), 0)]);

    expect(synchronizeThinkingGroupExpansion(current, rows, new Map())).toBe(current);
  });
});
