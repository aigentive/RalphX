import { describe, expect, it } from "vitest";

import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
} from "@/api/chat";
import {
  getAgentWorkspacePrConflictSummary,
  isAgentWorkspaceAutoMergeDeferred,
  isAgentWorkspaceAutoMergeRequestPending,
  shouldAutoRefreshCleanAgentWorkspaceFromBase,
  shouldShowAgentWorkspacePublishSurface,
} from "./agentWorkspacePublishState";

function workspace(
  overrides: Partial<AgentConversationWorkspace> = {},
): AgentConversationWorkspace {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    mode: "edit",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    baseCommit: null,
    branchName: "ralphx/ralphx/agent-abcdef12",
    worktreePath: "/tmp/ralphx/conversation-1",
    linkedIdeationSessionId: null,
    linkedPlanBranchId: null,
    publicationPrNumber: null,
    publicationPrUrl: null,
    publicationPrStatus: null,
    publicationPushStatus: null,
    autoPublishEnabled: true,
    autoPublishPausedPrAutofixEnabled: null,
    autoPublishPausedPrAutoMergeDesired: null,
    status: "active",
    createdAt: "2026-04-23T09:00:00Z",
    updatedAt: "2026-04-23T09:00:00Z",
    ...overrides,
  };
}

function freshness(
  overrides: Partial<AgentConversationWorkspaceFreshness> = {},
): AgentConversationWorkspaceFreshness {
  return {
    conversationId: "conversation-1",
    freshnessScope: "full",
    baseRef: "release/1.2",
    baseDisplayName: "release/1.2",
    targetRef: "origin/release/1.2",
    capturedBaseCommit: "old-base-sha",
    targetBaseCommit: "new-base-sha",
    isBaseAhead: true,
    hasUncommittedChanges: false,
    unpublishedCommitCount: 0,
    remoteRefreshed: true,
    worktreeStatusChecked: true,
    baseStatus: "valid",
    effectiveBaseRef: "release/1.2",
    effectiveBaseDisplayName: "release/1.2",
    baseBlockReason: null,
    ...overrides,
  };
}

const base = {
  autoMergeDesired: true,
  autoMergeCurrent: false as boolean | null,
  hasPublishedPr: true,
  publicationPushStatus: "pushed",
  terminalPublicationStatus: null as string | null,
};

describe("isAgentWorkspaceAutoMergeRequestPending", () => {
  it("returns true when supervision status is null (active publish in progress)", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        prSupervisionStatus: null,
      }),
    ).toBe(true);
  });

  it("returns false when supervision status is waiting (deferred/failed)", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        prSupervisionStatus: "waiting",
      }),
    ).toBe(false);
  });

  it("returns false when autoMergeCurrent is true", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        autoMergeCurrent: true,
        prSupervisionStatus: null,
      }),
    ).toBe(false);
  });

  it("returns false when autoMergeDesired is false", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        autoMergeDesired: false,
        prSupervisionStatus: null,
      }),
    ).toBe(false);
  });

  it("returns false for terminal publication status", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        prSupervisionStatus: null,
        terminalPublicationStatus: "merged",
      }),
    ).toBe(false);
  });

  it("returns false when supervision is monitoring", () => {
    expect(
      isAgentWorkspaceAutoMergeRequestPending({
        ...base,
        prSupervisionStatus: "monitoring",
      }),
    ).toBe(false);
  });
});

describe("isAgentWorkspaceAutoMergeDeferred", () => {
  it("returns true when supervision status is waiting", () => {
    expect(
      isAgentWorkspaceAutoMergeDeferred({
        ...base,
        prSupervisionStatus: "waiting",
      }),
    ).toBe(true);
  });

  it("returns false when supervision status is null", () => {
    expect(
      isAgentWorkspaceAutoMergeDeferred({
        ...base,
        prSupervisionStatus: null,
      }),
    ).toBe(false);
  });

  it("returns false when autoMergeCurrent is true", () => {
    expect(
      isAgentWorkspaceAutoMergeDeferred({
        ...base,
        autoMergeCurrent: true,
        prSupervisionStatus: "waiting",
      }),
    ).toBe(false);
  });

  it("returns false when autoMergeDesired is false", () => {
    expect(
      isAgentWorkspaceAutoMergeDeferred({
        ...base,
        autoMergeDesired: false,
        prSupervisionStatus: "waiting",
      }),
    ).toBe(false);
  });

  it("returns false when supervision is monitoring", () => {
    expect(
      isAgentWorkspaceAutoMergeDeferred({
        ...base,
        prSupervisionStatus: "monitoring",
      }),
    ).toBe(false);
  });
});

describe("shouldShowAgentWorkspacePublishSurface", () => {
  it("shows the publish surface for edit workspaces linked to a planning session", () => {
    expect(
      shouldShowAgentWorkspacePublishSurface(
        workspace({ linkedIdeationSessionId: "planning-session-1" }),
      ),
    ).toBe(true);
  });

  it("shows the publish surface for edit workspaces linked to a plan branch", () => {
    expect(
      shouldShowAgentWorkspacePublishSurface(
        workspace({ linkedPlanBranchId: "plan-branch-1" }),
      ),
    ).toBe(true);
  });

  it("keeps non-execution planning workspaces out of the publish surface", () => {
    expect(
      shouldShowAgentWorkspacePublishSurface(
        workspace({
          mode: "plan",
          linkedIdeationSessionId: "planning-session-1",
        }),
      ),
    ).toBe(false);
  });
});

describe("shouldAutoRefreshCleanAgentWorkspaceFromBase", () => {
  it("allows clean edit workspaces behind their configured base", () => {
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace({ baseRef: "release/1.2", baseDisplayName: "release/1.2" }),
        freshness(),
      ),
    ).toBe(true);
  });

  it("rejects workspaces with local changes or publishable commits", () => {
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ hasUncommittedChanges: true }),
      ),
    ).toBe(false);
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ unpublishedCommitCount: 1 }),
      ),
    ).toBe(false);
  });

  it("requires a full remote-refreshed freshness check", () => {
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ freshnessScope: "local" }),
      ),
    ).toBe(false);
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ remoteRefreshed: false }),
      ),
    ).toBe(false);
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ worktreeStatusChecked: false }),
      ),
    ).toBe(false);
  });

  it("rejects blocked, missing, and non-edit workspaces", () => {
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace(),
        freshness({ baseStatus: "blocked" }),
      ),
    ).toBe(false);
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace({ status: "missing" }),
        freshness(),
      ),
    ).toBe(false);
    expect(
      shouldAutoRefreshCleanAgentWorkspaceFromBase(
        workspace({ mode: "ideation" }),
        freshness(),
      ),
    ).toBe(false);
  });
});

describe("getAgentWorkspacePrConflictSummary", () => {
  it("returns blocked merge-conflict summaries", () => {
    expect(
      getAgentWorkspacePrConflictSummary(
        workspace({
          prSupervisionStatus: "blocked",
          prSupervisionSummary:
            "PR #470 has merge conflicts. GitHub reports: PR is reported as conflicting.",
        }),
      ),
    ).toBe(
      "PR #470 has merge conflicts. GitHub reports: PR is reported as conflicting.",
    );
  });

  it("ignores generic blocked supervision summaries", () => {
    expect(
      getAgentWorkspacePrConflictSummary(
        workspace({
          prSupervisionStatus: "blocked",
          prSupervisionSummary: "Required checks are still pending.",
        }),
      ),
    ).toBeNull();
    expect(
      getAgentWorkspacePrConflictSummary(
        workspace({
          prSupervisionStatus: "monitoring",
          prSupervisionSummary: "PR #470 has merge conflicts.",
        }),
      ),
    ).toBeNull();
  });
});
