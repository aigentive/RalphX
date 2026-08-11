import { describe, it, expect, beforeEach, vi } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { useChatAttachments, type ChatAttachment } from "./useChatAttachments";
import { invoke } from "@tauri-apps/api/core";
import { useChatStore } from "@/stores/chatStore";

// Mock Tauri invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

// Helper to create mock File with arrayBuffer support
function createMockFile(content: string, name: string, type?: string): File {
  const buffer = new TextEncoder().encode(content).buffer;

  // Create a File-like object with mocked arrayBuffer
  const file = {
    name,
    lastModified: Date.now(),
    size: buffer.byteLength,
    type: type || '',
    arrayBuffer: vi.fn().mockResolvedValue(buffer),
    stream: vi.fn(),
    text: vi.fn(),
    slice: vi.fn(),
  } as unknown as File;

  return file;
}

describe("useChatAttachments", () => {
  const conversationId = "conv-123";
  const mockInvoke = vi.mocked(invoke);

  beforeEach(() => {
    vi.clearAllMocks();
    useChatStore.setState({
      messages: {},
      context: null,
      isLoading: false,
      activeConversationIds: {},
      activeAgentRunIds: {},
      queuedMessages: {},
      agentStatus: {},
      agentActivityLabels: {},
      isSending: {},
      lastAgentEventTimestamp: {},
      toolCallStartTimes: {},
      lastToolCallCompletionTimestamp: {},
      toolCallCompletionTimestamps: {},
      effectiveModel: {},
      composerDraftsByKey: {},
    });
  });

  describe("uploadFiles", () => {
    it("should upload a single file successfully", async () => {
      const mockAttachment: ChatAttachment = {
        id: "att-1",
        conversationId,
        fileName: "test.txt",
        filePath: "/path/to/test.txt",
        mimeType: "text/plain",
        fileSize: 100,
        createdAt: "2026-02-14T00:00:00Z",
      };

      mockInvoke.mockResolvedValueOnce(mockAttachment);

      const { result } = renderHook(() => useChatAttachments(conversationId));

      const file = createMockFile("test content", "test.txt", "text/plain");

      let uploadedAttachments: ChatAttachment[] = [];
      await act(async () => {
        uploadedAttachments = await result.current.uploadFiles([file]);
      });

      expect(uploadedAttachments).toHaveLength(1);
      expect(uploadedAttachments[0]).toEqual(mockAttachment);
      expect(result.current.attachments).toHaveLength(1);
      expect(result.current.attachments[0]).toEqual(mockAttachment);
      expect(mockInvoke).toHaveBeenCalledWith("upload_chat_attachment", {
        input: {
          conversationId,
          fileName: "test.txt",
          fileData: expect.any(Array),
          mimeType: "text/plain",
        },
      });
    });

    it("should upload markdown files as normal chat attachments", async () => {
      const mockAttachment: ChatAttachment = {
        id: "att-plan",
        conversationId,
        fileName: "dropped-plan.md",
        filePath: "/path/to/dropped-plan.md",
        mimeType: "text/markdown",
        fileSize: 12,
        createdAt: "2026-02-14T00:00:00Z",
      };

      mockInvoke.mockResolvedValueOnce(mockAttachment);

      const { result } = renderHook(() => useChatAttachments(conversationId));
      const file = createMockFile("# Plan", "dropped-plan.md", "text/markdown");

      await act(async () => {
        await result.current.uploadFiles([file]);
      });

      expect(result.current.attachments).toEqual([mockAttachment]);
      expect(mockInvoke).toHaveBeenCalledWith("upload_chat_attachment", {
        input: {
          conversationId,
          fileName: "dropped-plan.md",
          fileData: expect.any(Array),
          mimeType: "text/markdown",
        },
      });
      expect(mockInvoke).not.toHaveBeenCalledWith(
        "import_agent_conversation_plan",
        expect.anything(),
      );
    });

    it("should upload multiple files successfully", async () => {
      const mockAttachment1: ChatAttachment = {
        id: "att-1",
        conversationId,
        fileName: "file1.txt",
        filePath: "/path/to/file1.txt",
        fileSize: 100,
        createdAt: "2026-02-14T00:00:00Z",
      };

      const mockAttachment2: ChatAttachment = {
        id: "att-2",
        conversationId,
        fileName: "file2.txt",
        filePath: "/path/to/file2.txt",
        fileSize: 200,
        createdAt: "2026-02-14T00:00:00Z",
      };

      mockInvoke
        .mockResolvedValueOnce(mockAttachment1)
        .mockResolvedValueOnce(mockAttachment2);

      const { result } = renderHook(() => useChatAttachments(conversationId));

      const file1 = createMockFile("content 1", "file1.txt", "text/plain");
      const file2 = createMockFile("content 2", "file2.txt", "text/plain");

      await act(async () => {
        await result.current.uploadFiles([file1, file2]);
      });

      expect(result.current.attachments).toHaveLength(2);
      expect(mockInvoke).toHaveBeenCalledTimes(2);
    });

    it("should restore uploaded attachments from a composer draft key", async () => {
      const mockAttachment: ChatAttachment = {
        id: "att-draft",
        conversationId,
        fileName: "draft.txt",
        filePath: "/path/to/draft.txt",
        fileSize: 100,
        createdAt: "2026-02-14T00:00:00Z",
      };
      mockInvoke.mockResolvedValueOnce(mockAttachment);

      const first = renderHook(() =>
        useChatAttachments(conversationId, { draftKey: "conversation:conv-123" }),
      );
      const file = createMockFile("draft", "draft.txt");

      await act(async () => {
        await first.result.current.uploadFiles([file]);
      });
      expect(first.result.current.attachments).toEqual([mockAttachment]);

      first.unmount();

      const second = renderHook(() =>
        useChatAttachments(conversationId, { draftKey: "conversation:conv-123" }),
      );

      expect(second.result.current.attachments).toEqual([mockAttachment]);
    });

    it("should reject files exceeding size limit", async () => {
      const { result } = renderHook(() => useChatAttachments(conversationId));

      // Create a file larger than 10MB
      const largeContent = new Uint8Array(11 * 1024 * 1024);
      const largeFile = new File([largeContent], "large.txt", {
        type: "text/plain",
      });

      await expect(
        act(async () => {
          await result.current.uploadFiles([largeFile]);
        })
      ).rejects.toThrow("Files exceed 10MB limit");

      expect(mockInvoke).not.toHaveBeenCalled();
    });

    it("should reject when total file count exceeds limit", async () => {
      // Pre-populate with 3 attachments
      const mockAttachment: ChatAttachment = {
        id: "att-1",
        conversationId,
        fileName: "existing.txt",
        filePath: "/path/to/existing.txt",
        fileSize: 100,
        createdAt: "2026-02-14T00:00:00Z",
      };

      mockInvoke
        .mockResolvedValueOnce(mockAttachment)
        .mockResolvedValueOnce({ ...mockAttachment, id: "att-2" })
        .mockResolvedValueOnce({ ...mockAttachment, id: "att-3" });

      const { result } = renderHook(() => useChatAttachments(conversationId));

      // Upload 3 files first
      const file1 = createMockFile("1", "file1.txt");
      const file2 = createMockFile("2", "file2.txt");
      const file3 = createMockFile("3", "file3.txt");

      await act(async () => {
        await result.current.uploadFiles([file1, file2, file3]);
      });

      expect(result.current.attachments).toHaveLength(3);

      // Try to upload 3 more (would exceed limit of 5)
      const file4 = createMockFile("4", "file4.txt");
      const file5 = createMockFile("5", "file5.txt");
      const file6 = createMockFile("6", "file6.txt");

      await expect(
        act(async () => {
          await result.current.uploadFiles([file4, file5, file6]);
        })
      ).rejects.toThrow("Cannot upload more than 5 files total");
    });

    it("should track upload progress", async () => {
      const mockAttachment: ChatAttachment = {
        id: "att-1",
        conversationId,
        fileName: "test.txt",
        filePath: "/path/to/test.txt",
        fileSize: 100,
        createdAt: "2026-02-14T00:00:00Z",
      };

      mockInvoke.mockResolvedValueOnce(mockAttachment);

      const { result } = renderHook(() => useChatAttachments(conversationId));

      const file = createMockFile("test", "test.txt");

      act(() => {
        void result.current.uploadFiles([file]);
      });

      // Check that uploading state is set
      expect(result.current.uploading).toBe(true);
      expect(result.current.uploadProgress).toHaveLength(1);
      expect(result.current.uploadProgress[0]).toEqual({
        fileName: "test.txt",
        status: "uploading",
      });

      // Wait for upload to complete
      await waitFor(() => {
        expect(result.current.uploading).toBe(false);
      });

      // Check that progress was updated to complete
      expect(result.current.uploadProgress[0].status).toBe("complete");
    });

    it("should handle upload errors gracefully", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("Network error"));

      const { result } = renderHook(() => useChatAttachments(conversationId));

      const file = createMockFile("test", "test.txt");

      // Start the upload and capture error
      let uploadError: Error | null = null;
      await act(async () => {
        try {
          await result.current.uploadFiles([file]);
        } catch (error) {
          uploadError = error as Error;
        }
      });

      // Should have thrown the error
      expect(uploadError).toBeTruthy();
      expect(uploadError?.message).toBe("Network error");

      // Should not add to attachments
      expect(result.current.attachments).toHaveLength(0);

      // Should mark as error in progress (before timeout clears it)
      await waitFor(() => {
        expect(result.current.uploadProgress[0]).toEqual({
          fileName: "test.txt",
          status: "error",
          error: "Network error",
        });
      });
    });
  });

  describe("removeAttachment", () => {
    it("should remove an attachment", async () => {
      const mockAttachment: ChatAttachment = {
        id: "att-1",
        conversationId,
        fileName: "test.txt",
        filePath: "/path/to/test.txt",
        fileSize: 100,
        createdAt: "2026-02-14T00:00:00Z",
      };

      mockInvoke.mockResolvedValueOnce(mockAttachment);

      const { result } = renderHook(() => useChatAttachments(conversationId));

      // Upload a file first
      const file = createMockFile("test", "test.txt");
      await act(async () => {
        await result.current.uploadFiles([file]);
      });

      expect(result.current.attachments).toHaveLength(1);

      // Mock successful delete
      mockInvoke.mockResolvedValueOnce(undefined);

      // Remove the attachment
      await act(async () => {
        await result.current.removeAttachment("att-1");
      });

      expect(result.current.attachments).toHaveLength(0);
      expect(mockInvoke).toHaveBeenLastCalledWith("delete_chat_attachment", {
        attachmentId: "att-1",
      });
    });
  });

  describe("clearAttachments", () => {
    it("should clear all attachments", async () => {
      const mockAttachment1: ChatAttachment = {
        id: "att-1",
        conversationId,
        fileName: "file1.txt",
        filePath: "/path/to/file1.txt",
        fileSize: 100,
        createdAt: "2026-02-14T00:00:00Z",
      };

      const mockAttachment2: ChatAttachment = {
        id: "att-2",
        conversationId,
        fileName: "file2.txt",
        filePath: "/path/to/file2.txt",
        fileSize: 200,
        createdAt: "2026-02-14T00:00:00Z",
      };

      mockInvoke
        .mockResolvedValueOnce(mockAttachment1)
        .mockResolvedValueOnce(mockAttachment2);

      const { result } = renderHook(() => useChatAttachments(conversationId));

      // Upload files
      const file1 = createMockFile("1", "file1.txt");
      const file2 = createMockFile("2", "file2.txt");

      await act(async () => {
        await result.current.uploadFiles([file1, file2]);
      });

      expect(result.current.attachments).toHaveLength(2);

      // Clear attachments
      act(() => {
        result.current.clearAttachments();
      });

      expect(result.current.attachments).toHaveLength(0);
      expect(result.current.uploadProgress).toHaveLength(0);
    });
  });
});
