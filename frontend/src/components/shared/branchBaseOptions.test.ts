import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  loadBranchBaseOptions,
  loadPullRequestBaseOptions,
  normalizeGitBranchName,
  synthesizeLocalBranchOption,
} from "./branchBaseOptions";

const {
  getGitBranchesMock,
  getGitCurrentBranchMock,
  getGitDefaultBranchMock,
  searchGithubPullRequestsMock,
  getPlanBranchesMock,
  listIdeationSessionsMock,
  listConversationsMock,
  listAgentConversationWorkspacesByProjectMock,
} = vi.hoisted(() => ({
  getGitBranchesMock: vi.fn(),
  getGitCurrentBranchMock: vi.fn(),
  getGitDefaultBranchMock: vi.fn(),
  searchGithubPullRequestsMock: vi.fn(),
  getPlanBranchesMock: vi.fn(),
  listIdeationSessionsMock: vi.fn(),
  listConversationsMock: vi.fn(),
  listAgentConversationWorkspacesByProjectMock: vi.fn(),
}));

vi.mock("@/api/projects", () => ({
  getGitBranches: (...args: unknown[]) => getGitBranchesMock(...args),
  getGitCurrentBranch: (...args: unknown[]) => getGitCurrentBranchMock(...args),
  getGitDefaultBranch: (...args: unknown[]) => getGitDefaultBranchMock(...args),
  searchGithubPullRequests: (...args: unknown[]) =>
    searchGithubPullRequestsMock(...args),
}));

vi.mock("@/api/plan-branch", () => ({
  planBranchApi: {
    getByProject: (...args: unknown[]) => getPlanBranchesMock(...args),
  },
}));

vi.mock("@/api/ideation", () => ({
  ideationApi: {
    sessions: {
      list: (...args: unknown[]) => listIdeationSessionsMock(...args),
    },
  },
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    listConversations: (...args: unknown[]) => listConversationsMock(...args),
    listAgentConversationWorkspacesByProject: (...args: unknown[]) =>
      listAgentConversationWorkspacesByProjectMock(...args),
  },
}));

describe("branchBaseOptions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getGitDefaultBranchMock.mockResolvedValue("main");
    getGitCurrentBranchMock.mockResolvedValue("feature/current");
    getGitBranchesMock.mockResolvedValue([
      "  main",
      "* feature/current",
      "  feature/shared",
      "+ ralphx/ralphx/task-raw",
      "  ralphx/ralphx/plan-456",
      "  ralphx/ralphx/agent-789",
    ]);
    searchGithubPullRequestsMock.mockResolvedValue([]);
    getPlanBranchesMock.mockResolvedValue([
      {
        id: "plan-branch-1",
        planArtifactId: "plan-artifact-1",
        sessionId: "session-plan",
        projectId: "project-1",
        branchName: "ralphx/ralphx/plan-456",
        sourceBranch: "main",
        status: "active",
        mergeTaskId: null,
        createdAt: "2026-04-24T00:00:00Z",
        mergedAt: null,
        prNumber: null,
        prUrl: null,
        prDraft: null,
        prPushStatus: null,
        prStatus: null,
        prPollingActive: false,
        prEligible: false,
        baseBranchOverride: null,
      },
    ]);
    listIdeationSessionsMock.mockResolvedValue([
      {
        id: "session-plan",
        projectId: "project-1",
        title: "Plan Branch Selector",
      },
    ]);
    listConversationsMock.mockResolvedValue([
      {
        id: "conversation-agent",
        contextType: "project",
        contextId: "project-1",
        title: "Agent Branch Conversation",
        providerSessionId: null,
        providerHarness: null,
        messageCount: 1,
        lastMessageAt: null,
        createdAt: "2026-04-24T00:00:00Z",
        updatedAt: "2026-04-24T00:00:00Z",
      },
    ]);
    listAgentConversationWorkspacesByProjectMock.mockResolvedValue([
      {
        conversationId: "conversation-agent",
        projectId: "project-1",
        mode: "edit",
        baseRefKind: "project_default",
        baseRef: "main",
        baseDisplayName: "Project default (main)",
        baseCommit: null,
        branchName: "ralphx/ralphx/agent-789",
        worktreePath: "/tmp/ralphx/conversation-agent",
        linkedIdeationSessionId: null,
        linkedPlanBranchId: null,
        publicationPrNumber: null,
        publicationPrUrl: null,
        publicationPrStatus: null,
        publicationPushStatus: null,
        status: "active",
        createdAt: "2026-04-24T00:00:00Z",
        updatedAt: "2026-04-24T00:00:00Z",
      },
    ]);
  });

  it("strips Git worktree markers from branch names", () => {
    expect(normalizeGitBranchName("+ ralphx/ralphx/task-raw")).toBe(
      "ralphx/ralphx/task-raw"
    );
    expect(normalizeGitBranchName("* feature/current")).toBe("feature/current");
  });

  it("hides raw RalphX branches but keeps titled plan and agent workspace branches", async () => {
    const result = await loadBranchBaseOptions({
      projectId: "project-1",
      workingDirectory: "/tmp/ralphx",
      projectBaseBranch: "main",
    });

    expect(result.selectedKey).toBe("project_default:main");
    expect(result.options.map((option) => option.label)).toEqual([
      "Project default (main)",
      "Current branch (feature/current)",
      "feature/shared",
      "Plan Branch Selector",
      "Agent Branch Conversation",
    ]);
    expect(result.options).not.toEqual(
      expect.arrayContaining([
        expect.objectContaining({ label: "ralphx/ralphx/task-raw" }),
      ])
    );
    expect(result.options.find((option) => option.source === "plan")).toEqual(
      expect.objectContaining({
        label: "Plan Branch Selector",
        detail: "ralphx/ralphx/plan-456",
      })
    );
    expect(result.options.find((option) => option.source === "agent")).toEqual(
      expect.objectContaining({
        label: "Agent Branch Conversation",
        detail: "ralphx/ralphx/agent-789",
      })
    );
    expect(
      result.options.some(
        (option) =>
          option.label.startsWith("+") || option.detail?.startsWith("+")
      )
    ).toBe(false);
  });

  it("reports agent-branch loader failure as degraded", async () => {
    listAgentConversationWorkspacesByProjectMock.mockRejectedValueOnce(
      new Error("unavailable"),
    );

    const result = await loadBranchBaseOptions({
      projectId: "project-1",
      workingDirectory: "/tmp/ralphx",
      projectBaseBranch: "main",
    });

    expect(result.degraded).toEqual({
      planBranches: false,
      agentBranches: true,
    });
    expect(result.options.some((option) => option.source === "agent")).toBe(
      false,
    );
  });

  it("reports plan-branch loader failure as degraded", async () => {
    getPlanBranchesMock.mockRejectedValueOnce(new Error("unavailable"));

    const result = await loadBranchBaseOptions({
      projectId: "project-1",
      workingDirectory: "/tmp/ralphx",
      projectBaseBranch: "main",
    });

    expect(result.degraded).toEqual({
      planBranches: true,
      agentBranches: false,
    });
    expect(result.options.some((option) => option.source === "plan")).toBe(
      false,
    );
  });

  it("keeps RalphX internal branches in known refs when the agent loader fails", async () => {
    listAgentConversationWorkspacesByProjectMock.mockRejectedValueOnce(
      new Error("unavailable"),
    );

    const result = await loadBranchBaseOptions({
      projectId: "project-1",
      workingDirectory: "/tmp/ralphx",
      projectBaseBranch: "main",
    });

    expect(result.knownBranchRefs).toContain("ralphx/ralphx/agent-789");
    expect(result.options.some((option) => option.source === "agent")).toBe(
      false,
    );
  });

  it("synthesizes a local branch option with a default label", () => {
    expect(synthesizeLocalBranchOption("feature/recover")).toEqual({
      key: "local_branch:feature/recover",
      label: "feature/recover",
      detail: "Local branch",
      source: "local",
      selection: {
        kind: "local_branch",
        ref: "feature/recover",
        displayName: "feature/recover",
      },
    });
    expect(
      synthesizeLocalBranchOption("feature/recover", "Recovered branch").label,
    ).toBe("Recovered branch");
  });

  it("reports no degradation for a clean load", async () => {
    const result = await loadBranchBaseOptions({
      projectId: "project-1",
      workingDirectory: "/tmp/ralphx",
      projectBaseBranch: "main",
    });

    expect(result.degraded).toEqual({
      planBranches: false,
      agentBranches: false,
    });
  });

  it("uses the configured project base before Git's detected default", async () => {
    const result = await loadBranchBaseOptions({
      projectId: "project-1",
      workingDirectory: "/tmp/ralphx",
      projectBaseBranch: "develop",
      includePlanBranches: false,
      includeAgentBranches: false,
    });

    expect(getGitDefaultBranchMock).toHaveBeenCalledWith("/tmp/ralphx");
    expect(result.options[0]).toEqual(
      expect.objectContaining({
        key: "project_default:develop",
        label: "Project default (develop)",
        source: "project",
      })
    );
    expect(result.selectedKey).toBe("project_default:develop");
  });

  it("falls back to Git's detected default when configured base is blank", async () => {
    const result = await loadBranchBaseOptions({
      projectId: "project-1",
      workingDirectory: "/tmp/ralphx",
      projectBaseBranch: "   ",
      includePlanBranches: false,
      includeAgentBranches: false,
    });

    expect(result.options[0]).toEqual(
      expect.objectContaining({
        key: "project_default:main",
        label: "Project default (main)",
        source: "project",
      })
    );
  });

  it("maps same-repo pull requests to local branch base selections", async () => {
    searchGithubPullRequestsMock.mockResolvedValue([
      {
        number: 42,
        title: "Add PR picker",
        url: "https://github.com/owner/repo/pull/42",
        headRefName: "feature/pr-picker",
        headRefOid: "abc123",
        baseRefName: "main",
        isDraft: false,
        updatedAt: "2026-05-20T10:00:00Z",
        authorLogin: "dev",
        isCrossRepository: false,
      },
      {
        number: 43,
        title: "Forked contribution",
        url: "https://github.com/owner/repo/pull/43",
        headRefName: "fork-feature",
        headRefOid: "def456",
        baseRefName: "main",
        isDraft: false,
        updatedAt: "2026-05-20T11:00:00Z",
        authorLogin: "external",
        isCrossRepository: true,
      },
    ]);

    const options = await loadPullRequestBaseOptions({
      projectId: "project-1",
      query: "picker",
    });

    expect(searchGithubPullRequestsMock).toHaveBeenCalledWith({
      projectId: "project-1",
      query: "picker",
      limit: 30,
    });
    expect(options).toEqual([
      {
        key: "pull_request:42:feature/pr-picker",
        label: "#42 Add PR picker",
        detail: "feature/pr-picker -> main",
        source: "pull_request",
        selection: {
          kind: "local_branch",
          ref: "feature/pr-picker",
          displayName: "PR #42: Add PR picker",
          sourcePullRequest: {
            number: 42,
            url: "https://github.com/owner/repo/pull/42",
            title: "Add PR picker",
            headRefName: "feature/pr-picker",
            baseRefName: "main",
            headRefOid: "abc123",
          },
        },
      },
    ]);
  });

  it("retargets merged pull-request selections to their merge target", async () => {
    searchGithubPullRequestsMock.mockResolvedValue([
      {
        number: 52,
        title: "Completed picker work",
        url: "https://github.com/owner/repo/pull/52",
        headRefName: "feature/deleted-after-merge",
        headRefOid: "abc123",
        baseRefName: "release/next",
        isDraft: false,
        isCrossRepository: false,
        state: "merged",
        mergedAt: "2026-08-01T10:00:00Z",
      },
    ]);

    await expect(
      loadPullRequestBaseOptions({ projectId: "project-1" }),
    ).resolves.toEqual([
      expect.objectContaining({
        label: "#52 Completed picker work",
        detail: "Merged → release/next",
        selection: {
          kind: "local_branch",
          ref: "release/next",
          displayName: "release/next (PR #52 merged)",
          retargetedFromPullRequest: 52,
        },
      }),
    ]);
  });
});
