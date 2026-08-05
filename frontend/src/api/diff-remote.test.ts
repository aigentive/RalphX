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
});
