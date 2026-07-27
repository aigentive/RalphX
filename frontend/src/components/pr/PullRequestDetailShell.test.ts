import { describe, expect, it } from "vitest";

import type { AgentConversationWorkspace } from "@/api/chat";

import {
  hasPullRequestShell,
  pullRequestSelectorFromShell,
  pullRequestShellFromTicket,
  pullRequestShellFromWorkspace,
} from "./PullRequestDetailShell";

describe("PullRequestDetailShell", () => {
  it("builds a shell and selector from ticket PR metadata", () => {
    const shell = pullRequestShellFromTicket({
      projectId: "project-1",
      prNumber: 42,
      prUrl: "https://github.com/acme/app/pull/42",
      prStatus: "merged",
    });

    expect(shell).toMatchObject({
      projectId: "project-1",
      prNumber: 42,
      title: "PR #42",
      status: "merged",
    });
    expect(hasPullRequestShell(shell)).toBe(true);
    expect(pullRequestSelectorFromShell(shell)).toEqual({
      projectId: "project-1",
      prNumber: 42,
    });
  });

  it("builds source and publication shells from workspaces", () => {
    const publication = workspace({
      publicationPrNumber: 7,
      publicationPrUrl: "https://github.com/acme/app/pull/7",
      publicationPrStatus: "open",
    });
    const source = workspace({
      publicationPrNumber: null,
      sourcePullRequest: {
        number: 99,
        url: null,
        title: null,
        headRefName: "review/source",
        baseRefName: "main",
        headRefOid: null,
      },
    });

    expect(pullRequestShellFromWorkspace(publication)).toMatchObject({
      projectId: "project-1",
      prNumber: 7,
      branch: "feature/x",
      conversationId: "conversation-1",
    });
    expect(pullRequestSelectorFromShell(pullRequestShellFromWorkspace(source))).toEqual({
      projectId: "project-1",
      prNumber: 99,
    });
    expect(pullRequestShellFromWorkspace(null)).toBeNull();
  });

  it("selects URL-only published workspaces by their workspace branch", () => {
    const published = workspace({
      publicationPrUrl: "https://github.com/acme/app/pull/7",
    });

    expect(pullRequestShellFromWorkspace(published)).toMatchObject({
      projectId: "project-1",
      url: "https://github.com/acme/app/pull/7",
      branch: "feature/x",
      conversationId: "conversation-1",
    });
    expect(
      pullRequestSelectorFromShell(pullRequestShellFromWorkspace(published)),
    ).toEqual({
      projectId: "project-1",
      branch: "feature/x",
    });
  });

  it("keeps publication PR numbers ahead of source pull requests", () => {
    const published = workspace({
      publicationPrNumber: 7,
      publicationPrUrl: "https://github.com/acme/app/pull/7",
      sourcePullRequest: {
        number: 99,
        url: "https://github.com/acme/app/pull/99",
        title: "Source PR",
        headRefName: "review/source",
        baseRefName: "main",
        headRefOid: null,
      },
    });

    expect(
      pullRequestSelectorFromShell(pullRequestShellFromWorkspace(published)),
    ).toEqual({
      projectId: "project-1",
      prNumber: 7,
    });
  });

  it("rejects incomplete shells and selects by branch when no PR number exists", () => {
    expect(hasPullRequestShell({ projectId: "project-1" })).toBe(false);
    expect(pullRequestSelectorFromShell(null)).toBeNull();
    expect(
      pullRequestSelectorFromShell({
        projectId: "project-1",
        branch: "feature/x",
      }),
    ).toEqual({
      projectId: "project-1",
      branch: "feature/x",
    });
  });
});

function workspace(
  overrides: Partial<AgentConversationWorkspace> = {},
): AgentConversationWorkspace {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    mode: "edit",
    baseRefKind: "projectDefault",
    baseRef: "main",
    baseDisplayName: null,
    baseCommit: null,
    branchName: "feature/x",
    worktreePath: "/tmp/worktree",
    linkedIdeationSessionId: null,
    linkedPlanBranchId: null,
    sourcePullRequest: null,
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
    createdAt: "2026-06-24T08:00:00Z",
    updatedAt: "2026-06-24T08:00:00Z",
    ...overrides,
  };
}
