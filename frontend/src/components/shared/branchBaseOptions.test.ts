import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  loadBranchBaseOptions,
  loadPullRequestBaseOptions,
  normalizeGitBranchName,
  ticketAssociationBranchBaseOption,
  ticketCanonicalBranchBaseOption,
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

  it("maps ticket pull request associations to PR base selections", () => {
    const option = ticketAssociationBranchBaseOption({
      id: "https://github.com/owner/repo/pull/42",
      title: "PR #42",
      subtitle: "feature/ticket-pr",
      status: "open",
      active: true,
      deepLink: { view: "agents", id: "conversation-1", projectId: "project-1" },
      branchName: "feature/ticket-pr",
      baseRef: "main",
      prNumber: 42,
      prUrl: "https://github.com/owner/repo/pull/42",
    });

    expect(option).toEqual({
      key: "pull_request:42:feature/ticket-pr",
      label: "PR #42",
      detail: "feature/ticket-pr -> main",
      source: "pull_request",
      selection: {
        kind: "local_branch",
        ref: "feature/ticket-pr",
        displayName: "PR #42",
        sourcePullRequest: {
          number: 42,
          url: "https://github.com/owner/repo/pull/42",
          title: "PR #42",
          headRefName: "feature/ticket-pr",
          baseRefName: "main",
          headRefOid: null,
        },
      },
    });
  });

  it("builds deterministic ticket branch base options from composer references", () => {
    const option = ticketCanonicalBranchBaseOption({
      provider: "atlassian",
      kind: "jira",
      id: "10001",
      key: "RX 24/Follow-up",
      title: "Ticket title",
    });

    expect(option).toEqual({
      key: "ticket_branch:ralphx/ticket/jira-rx-24-follow-up",
      label: "Ticket RX 24/Follow-up",
      detail: "ralphx/ticket/jira-rx-24-follow-up",
      source: "local",
      selection: {
        kind: "local_branch",
        ref: "ralphx/ticket/jira-rx-24-follow-up",
        displayName: "Ticket RX 24/Follow-up (ralphx/ticket/jira-rx-24-follow-up)",
      },
    });
  });
});
