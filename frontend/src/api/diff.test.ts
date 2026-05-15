import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { diffApi, transformAgentWorkspaceReview } from "./diff";

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

// ── Shared raw fixtures ───────────────────────────────────────────────────

const rawFileChanges = [
  { path: "src/lib.rs", status: "modified", additions: 4, deletions: 1 },
];

const rawFileDiff = {
  file_path: "src/lib.rs",
  language: "rust",
  hunks: [
    {
      old_start: 1,
      old_lines: 3,
      new_start: 1,
      new_lines: 3,
      header: "@@ -1,3 +1,3 @@",
      lines: [
        { kind: "context", content: "fn main() {", old_line_num: 1, new_line_num: 1 },
        { kind: "deletion", content: "    println!(\"old\");", old_line_num: 2, new_line_num: null },
        { kind: "addition", content: "    println!(\"new\");", old_line_num: null, new_line_num: 2 },
        { kind: "context", content: "}", old_line_num: 3, new_line_num: 3 },
      ],
    },
  ],
  old_total_lines: 3,
  new_total_lines: 3,
  is_binary: false,
};

const expectedFileChanges = [
  { path: "src/lib.rs", status: "modified", additions: 4, deletions: 1 },
];

const expectedFileDiff = {
  filePath: "src/lib.rs",
  language: "rust",
  hunks: [
    {
      oldStart: 1,
      oldLines: 3,
      newStart: 1,
      newLines: 3,
      header: "@@ -1,3 +1,3 @@",
      lines: [
        { kind: "context", content: "fn main() {", oldLineNum: 1, newLineNum: 1 },
        { kind: "deletion", content: "    println!(\"old\");", oldLineNum: 2, newLineNum: null },
        { kind: "addition", content: "    println!(\"new\");", oldLineNum: null, newLineNum: 2 },
        { kind: "context", content: "}", oldLineNum: 3, newLineNum: 3 },
      ],
    },
  ],
  oldTotalLines: 3,
  newTotalLines: 3,
  isBinary: false,
};

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

  describe("staged/unstaged/cumulative API wrappers", () => {
    it("calls get_agent_conversation_workspace_staged_file_changes", async () => {
      mockInvoke.mockResolvedValue(rawFileChanges);
      const result = await diffApi.getAgentConversationWorkspaceStagedFileChanges("conv-1");
      expect(mockInvoke).toHaveBeenCalledWith(
        "get_agent_conversation_workspace_staged_file_changes",
        { conversationId: "conv-1" },
      );
      expect(result).toEqual(expectedFileChanges);
    });

    it("calls get_agent_conversation_workspace_unstaged_file_changes", async () => {
      mockInvoke.mockResolvedValue(rawFileChanges);
      const result = await diffApi.getAgentConversationWorkspaceUnstagedFileChanges("conv-1");
      expect(mockInvoke).toHaveBeenCalledWith(
        "get_agent_conversation_workspace_unstaged_file_changes",
        { conversationId: "conv-1" },
      );
      expect(result).toEqual(expectedFileChanges);
    });

    it("calls get_agent_conversation_workspace_staged_file_diff", async () => {
      mockInvoke.mockResolvedValue(rawFileDiff);
      const result = await diffApi.getAgentConversationWorkspaceStagedFileDiff(
        "conv-1",
        "src/lib.rs",
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        "get_agent_conversation_workspace_staged_file_diff",
        { conversationId: "conv-1", filePath: "src/lib.rs" },
      );
      expect(result).toEqual(expectedFileDiff);
    });

    it("calls get_agent_conversation_workspace_unstaged_file_diff", async () => {
      mockInvoke.mockResolvedValue(rawFileDiff);
      const result = await diffApi.getAgentConversationWorkspaceUnstagedFileDiff(
        "conv-1",
        "src/lib.rs",
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        "get_agent_conversation_workspace_unstaged_file_diff",
        { conversationId: "conv-1", filePath: "src/lib.rs" },
      );
      expect(result).toEqual(expectedFileDiff);
    });

    it("calls get_agent_conversation_workspace_cumulative_file_changes", async () => {
      mockInvoke.mockResolvedValue(rawFileChanges);
      const result = await diffApi.getAgentConversationWorkspaceCumulativeFileChanges("conv-1");
      expect(mockInvoke).toHaveBeenCalledWith(
        "get_agent_conversation_workspace_cumulative_file_changes",
        { conversationId: "conv-1" },
      );
      expect(result).toEqual(expectedFileChanges);
    });

    it("calls get_agent_conversation_workspace_cumulative_file_diff", async () => {
      mockInvoke.mockResolvedValue(rawFileDiff);
      const result = await diffApi.getAgentConversationWorkspaceCumulativeFileDiff(
        "conv-1",
        "src/lib.rs",
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        "get_agent_conversation_workspace_cumulative_file_diff",
        { conversationId: "conv-1", filePath: "src/lib.rs" },
      );
      expect(result).toEqual(expectedFileDiff);
    });
  });

  describe("getAgentConversationWorkspaceFileContentRange", () => {
    const rawRangeLines = [
      { line_num: 5, content: "    let x = 1;" },
      { line_num: 6, content: "    let y = 2;" },
    ];
    const expectedRangeLines = [
      { lineNum: 5, content: "    let x = 1;" },
      { lineNum: 6, content: "    let y = 2;" },
    ];

    beforeEach(() => {
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue({
          ok: true,
          json: () => Promise.resolve(rawRangeLines),
        })
      );
    });

    afterEach(() => {
      vi.unstubAllGlobals();
    });

    it("calls the HTTP range endpoint with correct query params (head ref)", async () => {
      const result = await diffApi.getAgentConversationWorkspaceFileContentRange({
        conversationId: "conv-1",
        side: "new",
        path: "src/lib.rs",
        refKind: { kind: "head" },
        from: 5,
        to: 6,
      });

      expect(fetch).toHaveBeenCalledOnce();
      const [calledUrl] = (fetch as ReturnType<typeof vi.fn>).mock.calls[0] as [string];
      expect(calledUrl).toContain("/api/agent-workspaces/conv-1/file-content-range");
      expect(calledUrl).toContain("side=new");
      expect(calledUrl).toContain("path=src%2Flib.rs");
      expect(calledUrl).toContain("ref_kind=head");
      expect(calledUrl).toContain("from=5");
      expect(calledUrl).toContain("to=6");
      expect(result).toEqual(expectedRangeLines);
    });

    it("includes sha param for commit ref kind", async () => {
      await diffApi.getAgentConversationWorkspaceFileContentRange({
        conversationId: "conv-1",
        side: "old",
        path: "src/main.rs",
        refKind: { kind: "commit", sha: "abc123" },
        from: 1,
        to: 10,
      });

      const [calledUrl] = (fetch as ReturnType<typeof vi.fn>).mock.calls[0] as [string];
      expect(calledUrl).toContain("ref_kind=commit");
      expect(calledUrl).toContain("sha=abc123");
    });

    it("throws on non-OK HTTP response", async () => {
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue({ ok: false, status: 500, statusText: "Internal Server Error" })
      );
      await expect(
        diffApi.getAgentConversationWorkspaceFileContentRange({
          conversationId: "conv-1",
          side: "new",
          path: "src/lib.rs",
          refKind: { kind: "staged" },
          from: 1,
          to: 5,
        })
      ).rejects.toThrow("File content range fetch failed: 500");
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
