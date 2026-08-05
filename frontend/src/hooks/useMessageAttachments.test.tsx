import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { chatApi } from "@/api/chat";
import type { ChatMessageData } from "@/components/Chat/ChatMessageList";
import { useMessageAttachments } from "./useMessageAttachments";

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return ({ children }: PropsWithChildren) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

function userMessage(overrides: Partial<ChatMessageData> = {}): ChatMessageData {
  return {
    id: "message-1",
    role: "user",
    content: "see attached",
    createdAt: "2026-05-20T15:20:06.014Z",
    ...overrides,
  };
}

describe("useMessageAttachments", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it("fetches timeline-hydrated user attachments by the backing message id", async () => {
    const listAttachments = vi
      .spyOn(chatApi, "listMessageAttachments")
      .mockResolvedValue([
        {
          id: "attachment-1",
          conversationId: "conversation-1",
          messageId: "message-1",
          fileName: "Screenshot.png",
          filePath: "/app-data/attachments/Screenshot.png",
          mimeType: "image/png",
          fileSize: 1024,
          createdAt: "2026-05-20T15:20:04.994Z",
        },
      ]);

    const timelineMessage = userMessage({
      id: "timeline-item-1",
      parentMessageId: "message-1",
      timelineSequence: 1,
    });

    const { result } = renderHook(
      () => useMessageAttachments([timelineMessage], "conversation-1"),
      { wrapper: createWrapper() }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(listAttachments).toHaveBeenCalledWith("message-1");
    expect(result.current.data?.unavailableMessageIds.size).toBe(0);
    expect(result.current.data?.attachments.get("timeline-item-1")).toEqual([
      {
        id: "attachment-1",
        fileName: "Screenshot.png",
        filePath: "/app-data/attachments/Screenshot.png",
        mimeType: "image/png",
        fileSize: 1024,
      },
    ]);
  });

  it("keeps a failed host attachment read distinct from a known-empty result", async () => {
    vi.spyOn(chatApi, "listMessageAttachments").mockRejectedValue({
      outcome: "commandError",
      error: "REMOTE_COMMAND_UNAVAILABLE",
    });

    const { result } = renderHook(
      () => useMessageAttachments([userMessage()], "conversation-1"),
      { wrapper: createWrapper() },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data?.unavailableMessageIds).toEqual(new Set(["message-1"]));
    expect(result.current.data?.attachments.size).toBe(0);
  });

  it("reports a successful empty attachment read as available", async () => {
    vi.spyOn(chatApi, "listMessageAttachments").mockResolvedValue([]);

    const { result } = renderHook(
      () => useMessageAttachments([userMessage()], "conversation-1"),
      { wrapper: createWrapper() },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data?.unavailableMessageIds.size).toBe(0);
    expect(result.current.data?.attachments.size).toBe(0);
  });
});
