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

const rawConflictDiff = {
  filePath: "src/conflict.rs",
  baseContent: "base\n",
  oursContent: "ours\n",
  theirsContent: "theirs\n",
  mergedWithMarkers: "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n",
  language: "rust",
};

const expectedFileChanges = [
  { path: "src/lib.rs", status: "modified", additions: 4, deletions: 1, isGenerated: false },
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

const expectedConflictDiff = {
  filePath: "src/conflict.rs",
  baseContent: "base\n",
  oursContent: "ours\n",
  theirsContent: "theirs\n",
  mergedWithMarkers: "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n",
  language: "rust",
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
          isGenerated: false,
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
      supportsWorktreeModes: true,
    });
  });

  it("loads and transforms GitHub PR annotation payloads", async () => {
    mockInvoke.mockResolvedValue({
      pr_number: 78,
      head_sha: "head-sha",
      annotations: [
        {
          id: "code-scanning:7",
          source: "code_scanning",
          path: "src/lib.rs",
          side: "right",
          start_line: 22,
          end_line: 23,
          start_column: 5,
          end_column: 12,
          level: "high",
          status: "open",
          title: "Filesystem path injection",
          message: "This path depends on user input.",
          author: null,
          check_name: "CodeQL",
          url: "https://github.com/owner/repo/security/code-scanning/7",
          is_outdated: false,
          created_at: "2026-04-22T08:00:00Z",
        },
      ],
      sources_unavailable: [
        {
          source: "check_runs",
          reason: "Missing checks permission",
        },
      ],
    });

    const annotations =
      await diffApi.getAgentConversationWorkspacePrAnnotations("conversation-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_conversation_workspace_pr_annotations",
      { conversationId: "conversation-1" }
    );
    expect(annotations).toEqual({
      prNumber: 78,
      headSha: "head-sha",
      annotations: [
        {
          id: "code-scanning:7",
          source: "code_scanning",
          path: "src/lib.rs",
          side: "right",
          startLine: 22,
          endLine: 23,
          startColumn: 5,
          endColumn: 12,
          level: "high",
          status: "open",
          title: "Filesystem path injection",
          message: "This path depends on user input.",
          author: null,
          checkName: "CodeQL",
          url: "https://github.com/owner/repo/security/code-scanning/7",
          isOutdated: false,
          createdAt: "2026-04-22T08:00:00Z",
        },
      ],
      sourcesUnavailable: [
        {
          source: "check_runs",
          reason: "Missing checks permission",
        },
      ],
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

    it("calls repair staged and unstaged file change commands", async () => {
      mockInvoke.mockResolvedValue(rawFileChanges);
      const staged =
        await diffApi.getAgentConversationWorkspaceRepairStagedFileChanges("conv-1");
      expect(mockInvoke).toHaveBeenCalledWith(
        "get_agent_conversation_workspace_repair_staged_file_changes",
        { conversationId: "conv-1" },
      );
      expect(staged).toEqual(expectedFileChanges);

      mockInvoke.mockClear();
      mockInvoke.mockResolvedValue(rawFileChanges);
      const unstaged =
        await diffApi.getAgentConversationWorkspaceRepairUnstagedFileChanges("conv-1");
      expect(mockInvoke).toHaveBeenCalledWith(
        "get_agent_conversation_workspace_repair_unstaged_file_changes",
        { conversationId: "conv-1" },
      );
      expect(unstaged).toEqual(expectedFileChanges);
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

    it("calls repair staged and unstaged file diff commands", async () => {
      mockInvoke.mockResolvedValue(rawFileDiff);
      const staged = await diffApi.getAgentConversationWorkspaceRepairStagedFileDiff(
        "conv-1",
        "src/lib.rs",
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        "get_agent_conversation_workspace_repair_staged_file_diff",
        { conversationId: "conv-1", filePath: "src/lib.rs" },
      );
      expect(staged).toEqual(expectedFileDiff);

      mockInvoke.mockClear();
      mockInvoke.mockResolvedValue(rawFileDiff);
      const unstaged = await diffApi.getAgentConversationWorkspaceRepairUnstagedFileDiff(
        "conv-1",
        "src/lib.rs",
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        "get_agent_conversation_workspace_repair_unstaged_file_diff",
        { conversationId: "conv-1", filePath: "src/lib.rs" },
      );
      expect(unstaged).toEqual(expectedFileDiff);
    });

    it("calls repair conflict file diff command", async () => {
      mockInvoke.mockResolvedValue(rawConflictDiff);
      const conflict = await diffApi.getAgentConversationWorkspaceRepairConflictFileDiff(
        "conv-1",
        "src/conflict.rs",
      );
      expect(mockInvoke).toHaveBeenCalledWith(
        "get_agent_conversation_workspace_repair_conflict_file_diff",
        { conversationId: "conv-1", filePath: "src/conflict.rs" },
      );
      expect(conflict).toEqual(expectedConflictDiff);
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

  describe("getAgentConversationWorkspaceFileDiffPage", () => {
    const rawPage = {
      file_path: "src/lib.rs",
      language: "rust",
      rows: [
        {
          kind: "hunk_header",
          header: "@@ -1,2 +1,2 @@",
          old_start: 1,
          old_lines: 2,
          new_start: 1,
          new_lines: 2,
        },
        {
          kind: "line",
          line: {
            kind: "addition",
            content: "pub fn answer() -> u8 { 42 }",
            old_line_num: null,
            new_line_num: 1,
          },
        },
      ],
      offset: 0,
      limit: 2,
      next_offset: 2,
      total_rows: 12,
      old_total_lines: 0,
      new_total_lines: 1,
      is_binary: false,
    };

    beforeEach(() => {
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue({
          ok: true,
          json: () => Promise.resolve(rawPage),
        })
      );
    });

    afterEach(() => {
      vi.unstubAllGlobals();
    });

    it("calls the HTTP diff page endpoint and transforms rows", async () => {
      const result = await diffApi.getAgentConversationWorkspaceFileDiffPage({
        conversationId: "conv-1",
        path: "src/lib.rs",
        refKind: { kind: "head" },
        offset: 0,
        limit: 2,
      });

      expect(fetch).toHaveBeenCalledOnce();
      const [calledUrl] = (fetch as ReturnType<typeof vi.fn>).mock.calls[0] as [string];
      expect(calledUrl).toContain("/api/agent-workspaces/conv-1/file-diff-page");
      expect(calledUrl).toContain("path=src%2Flib.rs");
      expect(calledUrl).toContain("ref_kind=head");
      expect(calledUrl).toContain("offset=0");
      expect(calledUrl).toContain("limit=2");
      expect(result).toEqual({
        filePath: "src/lib.rs",
        language: "rust",
        rows: [
          {
            kind: "hunk_header",
            header: "@@ -1,2 +1,2 @@",
            oldStart: 1,
            oldLines: 2,
            newStart: 1,
            newLines: 2,
          },
          {
            kind: "line",
            line: {
              kind: "addition",
              content: "pub fn answer() -> u8 { 42 }",
              oldLineNum: null,
              newLineNum: 1,
            },
          },
        ],
        offset: 0,
        limit: 2,
        nextOffset: 2,
        totalRows: 12,
        oldTotalLines: 0,
        newTotalLines: 1,
        isBinary: false,
      });
    });

    it("includes sha param for commit ref kind", async () => {
      await diffApi.getAgentConversationWorkspaceFileDiffPage({
        conversationId: "conv-1",
        path: "src/lib.rs",
        refKind: { kind: "commit", sha: "abc123" },
        offset: 4,
        limit: 8,
      });

      const [calledUrl] = (fetch as ReturnType<typeof vi.fn>).mock.calls[0] as [string];
      expect(calledUrl).toContain("ref_kind=commit");
      expect(calledUrl).toContain("sha=abc123");
      expect(calledUrl).toContain("offset=4");
      expect(calledUrl).toContain("limit=8");
    });

    it("throws on non-OK HTTP response", async () => {
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue({ ok: false, status: 404, statusText: "Not Found" })
      );

      await expect(
        diffApi.getAgentConversationWorkspaceFileDiffPage({
          conversationId: "conv-1",
          path: "src/lib.rs",
          refKind: { kind: "head" },
          offset: 0,
          limit: 2,
        })
      ).rejects.toThrow("File diff page fetch failed: 404 Not Found");
    });
  });

  it("loads and transforms the compact agent workspace change summary", async () => {
    mockInvoke.mockResolvedValue({
      supports_worktree_modes: true,
      staged: { file_count: 1, additions: 7, deletions: 2 },
      unstaged: { file_count: 2, additions: 12, deletions: 3 },
    });

    const summary =
      await diffApi.getAgentConversationWorkspaceChangeSummary("conversation-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_conversation_workspace_change_summary",
      { conversationId: "conversation-1" }
    );
    expect(summary).toEqual({
      supportsWorktreeModes: true,
      staged: { fileCount: 1, additions: 7, deletions: 2 },
      unstaged: { fileCount: 2, additions: 12, deletions: 3 },
    });
  });

  it("loads and transforms the repair-aware agent workspace change summary", async () => {
    mockInvoke.mockResolvedValue({
      supports_worktree_modes: true,
      staged: { file_count: 1, additions: 7, deletions: 2 },
      unstaged: { file_count: 2, additions: 12, deletions: 3 },
      conflicted: { file_count: 1, files: ["src/lib.rs"] },
      repair_state: {
        expected_branch: "ralphx/demo/agent-conversation-1",
        checked_out_branch: "HEAD",
        rebase_in_progress: true,
        merge_in_progress: false,
      },
    });

    const summary =
      await diffApi.getAgentConversationWorkspaceRepairChangeSummary("conversation-1");

    expect(mockInvoke).toHaveBeenCalledWith(
      "get_agent_conversation_workspace_repair_change_summary",
      { conversationId: "conversation-1" },
    );
    expect(summary).toEqual({
      supportsWorktreeModes: true,
      staged: { fileCount: 1, additions: 7, deletions: 2 },
      unstaged: { fileCount: 2, additions: 12, deletions: 3 },
      conflicted: { fileCount: 1, files: ["src/lib.rs"] },
      repairState: {
        expectedBranch: "ralphx/demo/agent-conversation-1",
        checkedOutBranch: "HEAD",
        rebaseInProgress: true,
        mergeInProgress: false,
      },
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
      supports_worktree_modes: false,
    });

    expect(review).toEqual({
      changes: [
        {
          path: "README.md",
          status: "added",
          additions: 2,
          deletions: 0,
          isGenerated: false,
        },
      ],
      commits: [],
      baseRef: "base-sha",
      headRef: "feature/publish",
      supportsWorktreeModes: false,
    });
  });

  describe("isGenerated round-trip", () => {
    it("maps is_generated=true to isGenerated=true", async () => {
      mockInvoke.mockResolvedValue([
        { path: "package-lock.json", status: "modified", additions: 10, deletions: 5, is_generated: true },
      ]);
      const result = await diffApi.getAgentConversationWorkspaceStagedFileChanges("conv-1");
      expect(result[0]).toMatchObject({ path: "package-lock.json", isGenerated: true });
    });

    it("maps missing is_generated (server omits field) to isGenerated=false via Zod default", async () => {
      mockInvoke.mockResolvedValue([
        { path: "src/main.ts", status: "modified", additions: 2, deletions: 1 },
      ]);
      const result = await diffApi.getAgentConversationWorkspaceStagedFileChanges("conv-1");
      expect(result[0]).toMatchObject({ path: "src/main.ts", isGenerated: false });
    });

    it("maps is_generated=false to isGenerated=false", async () => {
      mockInvoke.mockResolvedValue([
        { path: "src/Foo.tsx", status: "added", additions: 20, deletions: 0, is_generated: false },
      ]);
      const result = await diffApi.getAgentConversationWorkspaceStagedFileChanges("conv-1");
      expect(result[0]).toMatchObject({ path: "src/Foo.tsx", isGenerated: false });
    });
  });
});
