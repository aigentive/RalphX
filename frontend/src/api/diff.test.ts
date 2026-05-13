import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { diffApi, transformAgentWorkspaceReview } from "./diff";

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

describe("diff api", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("loads and transforms the combined agent workspace review payload", async () => {
    mockInvoke.mockResolvedValue({
      changes: [
        {
          path: "src/lib.rs",
          status: "modified",
          additions: 4,
          deletions: 1,
        },
      ],
      commits: [
        {
          sha: "abcdef0123456789abcdef0123456789abcdef01",
          short_sha: "abcdef0",
          message: "Improve publish review",
          author: "Test User",
          timestamp: "2026-05-13T10:00:00Z",
        },
      ],
      base_ref: "origin/main",
      head_ref: "HEAD",
    });

    const review =
      await diffApi.getAgentConversationWorkspaceReview("conversation-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_conversation_workspace_review",
      { conversationId: "conversation-1" }
    );
    expect(review).toEqual({
      changes: [
        {
          path: "src/lib.rs",
          status: "modified",
          additions: 4,
          deletions: 1,
        },
      ],
      commits: [
        {
          sha: "abcdef0123456789abcdef0123456789abcdef01",
          shortSha: "abcdef0",
          message: "Improve publish review",
          author: "Test User",
          date: new Date("2026-05-13T10:00:00Z"),
        },
      ],
      baseRef: "origin/main",
      headRef: "HEAD",
    });
  });

  it("transforms agent workspace review fields without invoking Tauri", () => {
    const review = transformAgentWorkspaceReview({
      changes: [
        {
          path: "README.md",
          status: "added",
          additions: 2,
          deletions: 0,
        },
      ],
      commits: [],
      base_ref: "base-sha",
      head_ref: "feature/publish",
    });

    expect(review).toEqual({
      changes: [
        {
          path: "README.md",
          status: "added",
          additions: 2,
          deletions: 0,
        },
      ],
      commits: [],
      baseRef: "base-sha",
      headRef: "feature/publish",
    });
  });
});
