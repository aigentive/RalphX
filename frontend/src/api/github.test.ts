import { beforeEach, describe, expect, it, vi } from "vitest";

import { typedInvoke } from "@/lib/tauri";

import {
  GitHubConnectionStatusSchema,
  PullRequestDetailSchema,
  githubApi,
  isTransientGitHubConnectionState,
  requiresGitHubCredentialRepair,
} from "./github";

vi.mock("@/lib/tauri", () => ({
  typedInvoke: vi.fn(),
}));

describe("githubApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("fetches connection status with no args", async () => {
    const status = {
      state: "authenticated",
      diagnostic: null,
      ghInstalled: true,
      authenticated: true,
      host: "github.com",
      account: "octocat",
    };
    vi.mocked(typedInvoke).mockResolvedValue(status);

    await expect(githubApi.getConnectionStatus()).resolves.toEqual(status);

    expect(typedInvoke).toHaveBeenCalledWith(
      "get_github_connection_status",
      {},
      expect.any(Object),
    );
  });

  it("wraps get_pull_request_detail args under input with prNumber", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({ state: "noPr" });

    await githubApi.getPullRequestDetail({ projectId: "proj-1", prNumber: 42 });

    expect(typedInvoke).toHaveBeenCalledWith(
      "get_pull_request_detail",
      { input: { projectId: "proj-1", prNumber: 42 } },
      expect.any(Object),
    );
  });

  it("passes branch under input when no prNumber is given", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({ state: "noPr" });

    await githubApi.getPullRequestDetail({ projectId: "proj-1", branch: "feat/x" });

    expect(typedInvoke).toHaveBeenCalledWith(
      "get_pull_request_detail",
      { input: { projectId: "proj-1", branch: "feat/x" } },
      expect.any(Object),
    );
  });

  it("omits absent/empty selector keys from the input wrapper", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({ state: "noPr" });

    await githubApi.getPullRequestDetail({
      projectId: "proj-1",
      prNumber: null,
      branch: "",
    });

    expect(typedInvoke).toHaveBeenCalledWith(
      "get_pull_request_detail",
      { input: { projectId: "proj-1" } },
      expect.any(Object),
    );
  });
});

describe("GitHub Zod schemas round-trip the emitted camelCase DTOs", () => {
  it("parses a connection status payload", () => {
    const parsed = GitHubConnectionStatusSchema.parse({
      ghInstalled: true,
      authenticated: false,
      state: "provider_unavailable",
      diagnostic: "http5xx",
      host: null,
      account: null,
    });

    expect(parsed.ghInstalled).toBe(true);
    expect(parsed.authenticated).toBe(false);
    expect(parsed.state).toBe("provider_unavailable");
    expect(parsed.diagnostic).toBe("http5xx");
    expect(parsed.host).toBeNull();
    expect(parsed.account).toBeNull();
  });

  it("classifies credential repair and transient GitHub states separately", () => {
    expect(
      requiresGitHubCredentialRepair({
        state: "credential_rejected",
        diagnostic: "credentials_rejected",
        ghInstalled: true,
        authenticated: false,
        host: "github.com",
        account: null,
      }),
    ).toBe(true);
    expect(
      requiresGitHubCredentialRepair({
        state: "provider_unavailable",
        diagnostic: "http5xx",
        ghInstalled: true,
        authenticated: false,
        host: "github.com",
        account: null,
      }),
    ).toBe(false);
    expect(
      isTransientGitHubConnectionState({
        state: "provider_unavailable",
        diagnostic: "timeout",
        ghInstalled: true,
        authenticated: false,
        host: "github.com",
        account: null,
      }),
    ).toBe(true);
  });

  it("parses a full loaded PR-detail graph and keeps camelCase fields", () => {
    const parsed = PullRequestDetailSchema.parse({
      state: "loaded",
      origin: "ownedOutbound",
      description: {
        number: 42,
        title: "Add GitHub PR visibility",
        body: "Body",
        author: "octocat",
        createdAt: "2026-06-24T08:00:00Z",
        url: "https://github.com/acme/repo/pull/42",
        state: "open",
        isDraft: false,
        headRefName: "feat/pr-visibility",
        baseRefName: "main",
      },
      checks: [
        {
          name: "ci",
          status: "completed",
          conclusion: "success",
          detailsUrl: "https://ci.example/run/1",
        },
      ],
      reviewSummary: {
        reviewDecision: "CHANGES_REQUESTED",
        latestChangesRequestedAuthor: "reviewer",
        latestChangesRequestedBody: "Please fix",
        latestChangesRequestedSubmittedAt: "2026-06-24T09:00:00Z",
        latestChangesRequestedComments: [
          { id: "c1", author: "reviewer", path: "src/x.ts", line: 10, body: "nit" },
        ],
      },
      issueComments: [
        {
          id: "ic1",
          author: "octocat",
          body: "hello",
          url: null,
          createdAt: "2026-06-24T08:30:00Z",
          updatedAt: null,
          isBot: false,
          isCodecov: false,
          source: "evidence",
        },
      ],
      reviewThread: [
        {
          id: "rt1",
          author: "reviewer",
          body: "inline",
          path: "src/x.ts",
          side: "RIGHT",
          line: 12,
          url: null,
          createdAt: null,
          inReplyToId: null,
          isOutdated: false,
        },
      ],
      rxConversations: [
        {
          conversationId: "conv-1",
          branchName: "feat/pr-visibility",
          linkedIdeationSessionId: "sess-1",
          publicationPrNumber: 42,
          publicationPrStatus: "open",
        },
      ],
      linkedTickets: [{ provider: "linear", issueKey: "RX-1", url: null }],
      sourcesUnavailable: ["reviewThread"],
    });

    expect(parsed.state).toBe("loaded");
    expect(parsed.origin).toBe("ownedOutbound");
    expect(parsed.description?.headRefName).toBe("feat/pr-visibility");
    expect(parsed.description?.isDraft).toBe(false);
    expect(parsed.checks[0]?.detailsUrl).toBe("https://ci.example/run/1");
    expect(parsed.reviewSummary?.latestChangesRequestedComments[0]?.line).toBe(10);
    expect(parsed.issueComments[0]?.isCodecov).toBe(false);
    expect(parsed.issueComments[0]?.source).toBe("evidence");
    expect(parsed.reviewThread[0]?.inReplyToId).toBeNull();
    expect(parsed.rxConversations[0]?.conversationId).toBe("conv-1");
    expect(parsed.linkedTickets[0]?.issueKey).toBe("RX-1");
    expect(parsed.sourcesUnavailable).toEqual(["reviewThread"]);
  });

  it("parses the emitted empty payload for a typed non-loaded state", () => {
    const parsed = PullRequestDetailSchema.parse({
      state: "cliUnavailable",
      origin: null,
      description: null,
      checks: [],
      reviewSummary: null,
      issueComments: [],
      reviewThread: [],
      rxConversations: [],
      linkedTickets: [],
      sourcesUnavailable: [],
    });

    expect(parsed.state).toBe("cliUnavailable");
    expect(parsed.origin).toBeNull();
    expect(parsed.description).toBeNull();
    expect(parsed.reviewSummary).toBeNull();
    expect(parsed.issueComments).toEqual([]);
    expect(parsed.reviewThread).toEqual([]);
    expect(parsed.rxConversations).toEqual([]);
  });
});
