/**
 * WP3 Gap 5 — the workspace-review context is reachable over the remote transport.
 *
 * Before the fetch remount, `GET /api/agent-workspaces/:id/workspace-review-context` was not in
 * `REMOUNT_ALLOWLIST`, so a paired client got `REMOTE_COMMAND_UNAVAILABLE` and the review tab's
 * artifact ids stayed `null` — which left both already-registered `get_artifact` queries
 * DISABLED (`AgentsArtifactPane.tsx`: `enabled: Boolean(reviewArtifactId)`). The tab opened and
 * showed nothing.
 *
 * These tests drive the real `remote_fetch` proxy path and assert the two properties the pane
 * depends on: the read resolves, and it carries the persisted artifact-id PAIR.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { primitiveInvoke } = vi.hoisted(() => ({
  primitiveInvoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
}));

vi.mock("#tauri-core-primitive", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return { ...actual, invoke: primitiveInvoke };
});

import { getAgentWorkspaceReviewContext } from "@/api/chat";
import {
  resetTransportEnvironmentId,
  setTransportEnvironmentId,
} from "@/lib/remote/active-environment";

const REMOTE_ID = "env-remote";

const rawWorkspace = () => ({
  conversation_id: "conversation-1",
  project_id: "project-1",
  mode: "edit",
  base_ref_kind: "project_default",
  base_ref: "main",
  base_display_name: "Project default (main)",
  base_commit: "base-sha",
  branch_name: "ralphx/agent-conversation-1",
  worktree_path: "/tmp/ralphx/conversation-1",
  linked_ideation_session_id: null,
  linked_plan_branch_id: null,
  source_pull_request: null,
  publication_pr_number: null,
  publication_pr_url: null,
  publication_pr_status: null,
  publication_push_status: null,
  auto_publish_enabled: false,
  auto_publish_initial_pr_enabled: false,
  auto_publish_paused_pr_autofix_enabled: null,
  auto_publish_paused_pr_auto_merge_desired: null,
  pr_autofix_enabled: false,
  pr_auto_merge_desired: false,
  pr_auto_merge_method: "squash",
  pr_auto_merge_current: false,
  pr_supervision_status: null,
  pr_supervision_summary: null,
  pr_supervision_updated_at: null,
  status: "active",
  created_at: "2026-08-01T12:00:00Z",
  updated_at: "2026-08-01T12:00:00Z",
});

/** The persisted monitor snapshot the remounted handler serves — artifact ids included. */
const rawWorkspaceReviewMonitor = () => ({
  conversation_id: "conversation-1",
  project_id: "project-1",
  status: "ready",
  current_target_scope: "workspace_delta",
  reviewed_target_scope: "workspace_delta",
  review_conversation_id: "review-conversation-1",
  review_artifact_id: "review-artifact-1",
  review_artifact_version: 2,
  review_artifact_updated_at: "2026-08-01T12:05:00Z",
  review_requested_changes_artifact_id: "requested-changes-artifact-1",
  review_requested_changes_artifact_version: 2,
  review_requested_changes_artifact_updated_at: "2026-08-01T12:05:00Z",
  reviewed_head_sha: "head-sha",
  reviewed_diff_fingerprint: "fingerprint-1",
  selected_source_base_ref: null,
  selected_source_base_sha: null,
  selected_source_head_ref: null,
  selected_source_head_sha: null,
  selected_source_pull_request_number: null,
  workspace_base_ref: "main",
  workspace_base_sha: "base-sha",
  workspace_head_ref: "HEAD",
  workspace_head_sha: "head-sha",
  current_diff_fingerprint: "fingerprint-1",
  previous_version_id: null,
  review_requested_changes_previous_version_id: null,
  last_run_id: "run-1",
  last_error: null,
  created_at: "2026-08-01T12:00:00Z",
  updated_at: "2026-08-01T12:05:00Z",
});

const remountedContextBody = () => ({
  success: true,
  workspace: rawWorkspace(),
  events: [],
  target: {
    scope: "workspace_delta",
    base_ref: "main",
    base_sha: "base-sha",
    head_ref: "HEAD",
    head_sha: "head-sha",
    diff_fingerprint: "fingerprint-1",
    source_pull_request_number: null,
    review_packet: null,
  },
  monitor: rawWorkspaceReviewMonitor(),
  review_artifact_is_current: true,
  review_artifact_is_outdated: false,
  // The remounted handler pins runtime authority CLOSED: it reads no agent-run trust headers,
  // so a paired device can never present itself as the review's own run.
  can_mutate_review_state: false,
  review_runtime_state: "missing_runtime_identity",
  is_current: true,
  is_outdated: false,
  should_show_tab: true,
});

function remoteFetchAnswers(status: number, body: unknown): string[] {
  const paths: string[] = [];
  primitiveInvoke.mockImplementation(async (cmd: string, args?: unknown) => {
    if (cmd !== "remote_fetch") {
      throw new Error(`unexpected primitive invoke: ${cmd}`);
    }
    const request = args as { input?: { path?: string } } | undefined;
    paths.push(String(request?.input?.path ?? ""));
    return { status, headers: [], body: JSON.stringify(body) };
  });
  return paths;
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  resetTransportEnvironmentId();
});

describe("workspace review context over the remote transport", () => {
  it("resolves the artifact-id pair the review tab needs to enable both get_artifact reads", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    remoteFetchAnswers(200, remountedContextBody());

    const context = await getAgentWorkspaceReviewContext("conversation-1");

    // These two ids are the entire fix: `enabled: Boolean(reviewArtifactId)` on the Overview
    // query and `Boolean(workspaceReviewRequestedChangesArtifactId)` on its pair.
    expect(context.monitor.reviewArtifactId).toBe("review-artifact-1");
    expect(context.monitor.reviewRequestedChangesArtifactId).toBe(
      "requested-changes-artifact-1"
    );
    expect(context.shouldShowTab).toBe(true);
  });

  it("never claims review-mutating authority for a paired device", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    remoteFetchAnswers(200, remountedContextBody());

    const context = await getAgentWorkspaceReviewContext("conversation-1");

    expect(context.canMutateReviewState).toBe(false);
    expect(context.reviewRuntimeState).toBe("missing_runtime_identity");
  });

  it("asks for the plain read, with no recompute parameter, by default", async () => {
    setTransportEnvironmentId(REMOTE_ID);
    const paths = remoteFetchAnswers(200, remountedContextBody());

    await getAgentWorkspaceReviewContext("conversation-1");

    expect(paths).toHaveLength(1);
    expect(paths[0]).toContain(
      "agent-workspaces/conversation-1/workspace-review-context"
    );
    expect(paths[0]).not.toContain("refresh_target");
  });
});
