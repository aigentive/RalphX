import { beforeEach, describe, expect, it, vi } from "vitest";

vi.unmock("@tauri-apps/api/core");

const { primitiveInvoke } = vi.hoisted(() => ({
  primitiveInvoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
}));

vi.mock("#tauri-core-primitive", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return { ...actual, invoke: primitiveInvoke };
});

import { diffApi } from "@/api/diff";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

function useRemoteEnvironment(): void {
  useEnvironmentStore.setState({
    activeEnvironmentId: "remote-1",
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: "remote-1", name: "Host Mac", kind: "remote" },
    ],
    effectiveScopes: { "remote-1": ["ui:read"] },
    connectionPresentations: {},
  });
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  useRemoteEnvironment();
});

describe("remote workspace diff snapshots", () => {
  it("transforms the real review snapshot envelope and preserves its capture metadata", async () => {
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: {
        snapshot: {
          changes: [],
          commits: [],
          base_ref: "base-sha",
          head_ref: "HEAD",
          supports_worktree_modes: false,
        },
        captured_at: "2026-08-05T18:01:00+00:00",
        cache_version: "workspace-version-1",
        context_source: "github_patch",
      },
    });

    const review = await diffApi.getAgentConversationWorkspaceReview("conversation-1");

    // The REAL remote_invoke wire shape (network-invoke.ts): struct param `input`
    // carrying {id, requestId, cmd, args}.
    expect(primitiveInvoke).toHaveBeenCalledWith("remote_invoke", {
      input: expect.objectContaining({
        cmd: "get_remote_agent_conversation_workspace_review",
        args: { conversationId: "conversation-1" },
      }),
    });
    expect(review).toMatchObject({
      changes: [],
      commits: [],
      snapshotCapturedAt: "2026-08-05T18:01:00+00:00",
      snapshotCacheVersion: "workspace-version-1",
      snapshotContextSource: "github_patch",
    });
  });

  it("keeps an uncaptured summary unknown instead of fabricating known-empty buckets", async () => {
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: {
        snapshot: null,
        captured_at: null,
        cache_version: null,
        context_source: null,
      },
    });

    await expect(
      diffApi.getAgentConversationWorkspaceChangeSummary("conversation-1"),
    ).resolves.toBeNull();
  });

  it.each([
    {
      call: () =>
        diffApi.getAgentConversationWorkspaceFileDiff("conversation-1", "src/main.rs"),
      cmd: "get_remote_agent_conversation_workspace_file_diff",
      args: { conversationId: "conversation-1", filePath: "src/main.rs" },
    },
    {
      call: () =>
        diffApi.getAgentConversationWorkspaceCommitFileDiff(
          "conversation-1",
          "abc123",
          "src/main.rs",
        ),
      cmd: "get_remote_agent_conversation_workspace_commit_file_diff",
      args: {
        conversationId: "conversation-1",
        commitSha: "abc123",
        filePath: "src/main.rs",
      },
    },
    {
      call: () =>
        diffApi.getAgentConversationWorkspaceCumulativeFileDiff(
          "conversation-1",
          "src/main.rs",
        ),
      cmd: "get_remote_agent_conversation_workspace_cumulative_file_diff",
      args: { conversationId: "conversation-1", filePath: "src/main.rs" },
    },
  ])("routes $cmd through the real remote envelope and preserves absence", async ({ call, cmd, args }) => {
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: {
        snapshot: null,
        captured_at: null,
        cache_version: null,
        context_source: null,
      },
    });

    await expect(call()).resolves.toBeNull();
    const [transportCommand, transportArgs] = primitiveInvoke.mock.calls.at(-1) ?? [];
    expect(transportCommand).toBe("remote_invoke");
    expect(transportArgs).toEqual({
      input: expect.objectContaining({ cmd, args }),
    });
  });

  it("routes exact page coordinates and returns capture metadata with the page", async () => {
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: {
        snapshot: {
          file_path: "src/main.rs",
          language: "rust",
          rows: [],
          offset: 200,
          limit: 100,
          next_offset: null,
          total_rows: 200,
          old_total_lines: 10,
          new_total_lines: 10,
          is_binary: false,
        },
        captured_at: "2026-08-05T18:01:00+00:00",
        cache_version: "workspace-version-1",
        context_source: "worktree",
      },
    });

    const page = await diffApi.getAgentConversationWorkspaceFileDiffPage({
      conversationId: "conversation-1",
      path: "src/main.rs",
      refKind: { kind: "commit", sha: "abc123" },
      offset: 200,
      limit: 100,
    });

    const [transportCommand, transportArgs] = primitiveInvoke.mock.calls.at(-1) ?? [];
    expect(transportCommand).toBe("remote_invoke");
    expect(transportArgs).toEqual({
      input: expect.objectContaining({
        cmd: "get_remote_agent_conversation_workspace_file_diff_page",
        args: {
          conversationId: "conversation-1",
          filePath: "src/main.rs",
          refKind: { kind: "commit", sha: "abc123" },
          offset: 200,
          limit: 100,
        },
      }),
    });
    expect(page).toMatchObject({
      offset: 200,
      limit: 100,
      snapshotCapturedAt: "2026-08-05T18:01:00+00:00",
    });
  });

  it("preserves an unwrapped commandError from the real remote envelope", async () => {
    primitiveInvoke.mockResolvedValue({
      outcome: "commandError",
      error: "file snapshot read failed",
    });

    await expect(
      diffApi.getAgentConversationWorkspaceFileDiff("conversation-1", "src/main.rs"),
    ).rejects.toThrow("file snapshot read failed");
  });
});
