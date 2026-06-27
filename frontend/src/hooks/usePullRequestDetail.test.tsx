import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { agentWorkspaceKeys } from "@/components/agents/agentWorkspaceQueries";

import {
  mapPullRequestDetail,
  normalizePrStatus,
  usePullRequestDetail,
} from "./usePullRequestDetail";

type RawWorkspaceResponse = Record<string, unknown>;

function makeRawWorkspace(
  overrides: Partial<RawWorkspaceResponse> = {},
): RawWorkspaceResponse {
  return {
    conversation_id: "conversation-1",
    project_id: "project-1",
    mode: "edit",
    base_ref_kind: "project_default",
    base_ref: "main",
    base_display_name: "main",
    base_commit: "abc123",
    branch_name: "ralphx/test/pr-detail",
    worktree_path: "/tmp/ralphx-worktree",
    linked_ideation_session_id: null,
    linked_plan_branch_id: "plan-branch-1",
    source_pull_request: null,
    mode_switch_locked: false,
    mode_switch_lock_reason: null,
    publication_pr_number: null,
    publication_pr_url: null,
    publication_pr_status: null,
    publication_push_status: null,
    auto_publish_enabled: true,
    auto_publish_initial_pr_enabled: false,
    auto_publish_paused_pr_autofix_enabled: null,
    auto_publish_paused_pr_auto_merge_desired: null,
    pr_autofix_enabled: false,
    pr_auto_merge_desired: false,
    pr_auto_merge_method: "squash",
    pr_auto_merge_current: null,
    pr_supervision_status: null,
    pr_supervision_summary: null,
    pr_supervision_updated_at: null,
    status: "active",
    created_at: "2026-06-01T12:00:00Z",
    updated_at: "2026-06-01T12:05:00Z",
    ...overrides,
  };
}

function makeWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        gcTime: 0,
        retry: false,
      },
    },
  });
}

describe("normalizePrStatus", () => {
  it("maps known lowercase and TitleCase statuses to the strict badge union", () => {
    expect(normalizePrStatus("draft")).toBe("Draft");
    expect(normalizePrStatus("Draft")).toBe("Draft");
    expect(normalizePrStatus("open")).toBe("Open");
    expect(normalizePrStatus("Open")).toBe("Open");
    expect(normalizePrStatus("merged")).toBe("Merged");
    expect(normalizePrStatus("Merged")).toBe("Merged");
    expect(normalizePrStatus("closed")).toBe("Closed");
    expect(normalizePrStatus("Closed")).toBe("Closed");
  });

  it("returns null for empty and unknown producer values", () => {
    expect(normalizePrStatus(null)).toBeNull();
    expect(normalizePrStatus("")).toBeNull();
    expect(normalizePrStatus("   ")).toBeNull();
    expect(normalizePrStatus("failed")).toBeNull();
    expect(normalizePrStatus("changes_requested")).toBeNull();
  });
});

describe("mapPullRequestDetail", () => {
  it("maps publication PR fields first and source-only fields from sourcePullRequest", () => {
    const detail = mapPullRequestDetail({
      conversationId: "conversation-1",
      projectId: "project-1",
      mode: "edit",
      baseRefKind: "project_default",
      baseRef: "release/2026",
      baseDisplayName: "Release 2026",
      baseCommit: "abc123",
      branchName: "ralphx/test/pr-detail",
      worktreePath: "/tmp/ralphx-worktree",
      linkedIdeationSessionId: null,
      linkedPlanBranchId: "plan-branch-1",
      sourcePullRequest: {
        number: 17,
        url: "https://github.com/acme/app/pull/17",
        title: "Source branch title",
        headRefName: "feature/source",
        baseRefName: "main",
        headRefOid: "def456",
      },
      modeSwitchLocked: false,
      modeSwitchLockReason: null,
      publicationPrNumber: 42,
      publicationPrUrl: "https://github.com/acme/app/pull/42",
      publicationPrStatus: "Open",
      publicationPushStatus: "pushed",
      autoPublishEnabled: true,
      autoPublishInitialPrEnabled: false,
      autoPublishPausedPrAutofixEnabled: null,
      autoPublishPausedPrAutoMergeDesired: null,
      prAutofixEnabled: false,
      prAutoMergeDesired: false,
      prAutoMergeMethod: "squash",
      prAutoMergeCurrent: null,
      prSupervisionStatus: "blocked",
      prSupervisionSummary: "Mergeability blocker",
      prSupervisionUpdatedAt: "2026-06-01T12:07:00Z",
      status: "active",
      createdAt: "2026-06-01T12:00:00Z",
      updatedAt: "2026-06-01T12:05:00Z",
    });

    expect(detail).toEqual({
      origin: "publication",
      number: 42,
      status: "Open",
      url: "https://github.com/acme/app/pull/42",
      headRef: "feature/source",
      baseRef: "release/2026",
      pushStatus: "pushed",
      supervisionStatus: "blocked",
      supervisionSummary: "Mergeability blocker",
      supervisionUpdatedAt: "2026-06-01T12:07:00Z",
      title: "Source branch title",
    });
    expect(detail).not.toHaveProperty("author");
    expect(detail).not.toHaveProperty("body");
    expect(detail).not.toHaveProperty("publicationPrTitle");
  });

  it("falls back to the source pull request and omits unavailable fields", () => {
    const detail = mapPullRequestDetail({
      conversationId: "conversation-1",
      projectId: "project-1",
      mode: "edit",
      baseRefKind: "project_default",
      baseRef: "",
      baseDisplayName: null,
      baseCommit: null,
      branchName: "ralphx/test/source",
      worktreePath: "/tmp/ralphx-worktree",
      linkedIdeationSessionId: null,
      linkedPlanBranchId: null,
      sourcePullRequest: {
        number: 17,
        url: "https://github.com/acme/app/pull/17",
        title: null,
        headRefName: "feature/source",
        baseRefName: "main",
        headRefOid: null,
      },
      modeSwitchLocked: false,
      modeSwitchLockReason: null,
      publicationPrNumber: null,
      publicationPrUrl: null,
      publicationPrStatus: null,
      publicationPushStatus: null,
      autoPublishEnabled: true,
      autoPublishInitialPrEnabled: false,
      autoPublishPausedPrAutofixEnabled: null,
      autoPublishPausedPrAutoMergeDesired: null,
      prAutofixEnabled: false,
      prAutoMergeDesired: false,
      prAutoMergeMethod: "squash",
      prAutoMergeCurrent: null,
      prSupervisionStatus: null,
      prSupervisionSummary: null,
      prSupervisionUpdatedAt: null,
      status: "active",
      createdAt: "2026-06-01T12:00:00Z",
      updatedAt: "2026-06-01T12:05:00Z",
    });

    expect(detail).toEqual({
      origin: "source",
      number: 17,
      status: null,
      url: "https://github.com/acme/app/pull/17",
      headRef: "feature/source",
      baseRef: "main",
      pushStatus: null,
      supervisionStatus: null,
      supervisionSummary: null,
      supervisionUpdatedAt: null,
    });
    expect(detail).not.toHaveProperty("title");
    expect(detail).not.toHaveProperty("author");
    expect(detail).not.toHaveProperty("body");
  });

  it("returns null when neither publication nor source PR data exists", () => {
    expect(
      mapPullRequestDetail({
        conversationId: "conversation-1",
        projectId: "project-1",
        mode: "edit",
        baseRefKind: "project_default",
        baseRef: "main",
        baseDisplayName: "main",
        baseCommit: null,
        branchName: "ralphx/test/no-pr",
        worktreePath: "/tmp/ralphx-worktree",
        linkedIdeationSessionId: null,
        linkedPlanBranchId: null,
        sourcePullRequest: null,
        modeSwitchLocked: false,
        modeSwitchLockReason: null,
        publicationPrNumber: null,
        publicationPrUrl: null,
        publicationPrStatus: null,
        publicationPushStatus: null,
        autoPublishEnabled: true,
        autoPublishInitialPrEnabled: false,
        autoPublishPausedPrAutofixEnabled: null,
        autoPublishPausedPrAutoMergeDesired: null,
        prAutofixEnabled: false,
        prAutoMergeDesired: false,
        prAutoMergeMethod: "squash",
        prAutoMergeCurrent: null,
        prSupervisionStatus: null,
        prSupervisionSummary: null,
        prSupervisionUpdatedAt: null,
        status: "active",
        createdAt: "2026-06-01T12:00:00Z",
        updatedAt: "2026-06-01T12:05:00Z",
      }),
    ).toBeNull();
  });
});

describe("usePullRequestDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("derives detail from the shared workspace query without live PR commands", async () => {
    vi.mocked(invoke).mockResolvedValue(
      makeRawWorkspace({
        source_pull_request: {
          number: 17,
          url: "https://github.com/acme/app/pull/17",
          title: "Source branch title",
          head_ref_name: "feature/source",
          base_ref_name: "main",
          head_ref_oid: null,
        },
        publication_pr_number: 42,
        publication_pr_url: "https://github.com/acme/app/pull/42",
        publication_pr_status: "merged",
        publication_push_status: "pushed",
        pr_supervision_status: "waiting",
        pr_supervision_summary: "Waiting for checks",
        pr_supervision_updated_at: "2026-06-01T12:07:00Z",
      }),
    );
    const queryClient = makeQueryClient();

    const { result } = renderHook(
      () => usePullRequestDetail("conversation-1"),
      { wrapper: makeWrapper(queryClient) },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toMatchObject({
      origin: "publication",
      number: 42,
      status: "Merged",
      url: "https://github.com/acme/app/pull/42",
      headRef: "feature/source",
      baseRef: "main",
      pushStatus: "pushed",
      supervisionStatus: "waiting",
      supervisionSummary: "Waiting for checks",
      supervisionUpdatedAt: "2026-06-01T12:07:00Z",
      title: "Source branch title",
    });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("get_agent_conversation_workspace", {
      conversationId: "conversation-1",
    });
    const invokedCommands = vi
      .mocked(invoke)
      .mock.calls.map(([command]) => command);
    expect(invokedCommands).not.toContain("get_agent_conversation_workspace_freshness");
    expect(invokedCommands).not.toContain(
      "reconcile_agent_conversation_workspace_publication",
    );
    expect(queryClient.getQueryCache().findAll()).toHaveLength(1);
    expect(queryClient.getQueryCache().findAll()[0]?.queryKey).toEqual(
      agentWorkspaceKeys.workspace("conversation-1"),
    );
  });

  it("returns null detail for a workspace with no PR", async () => {
    vi.mocked(invoke).mockResolvedValue(makeRawWorkspace());

    const { result } = renderHook(
      () => usePullRequestDetail("conversation-1"),
      { wrapper: makeWrapper(makeQueryClient()) },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toBeNull();
  });

  it("does not fetch without a conversation id", () => {
    const { result } = renderHook(() => usePullRequestDetail(null), {
      wrapper: makeWrapper(makeQueryClient()),
    });

    expect(result.current.fetchStatus).toBe("idle");
    expect(result.current.data).toBeUndefined();
    expect(invoke).not.toHaveBeenCalled();
  });
});
