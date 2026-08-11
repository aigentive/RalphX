import { beforeEach, describe, expect, it, vi } from "vitest";

import { typedInvoke } from "@/lib/tauri";

import { atlassianApi, type AgentConversationJiraIssue } from "./atlassian";

vi.mock("@/lib/tauri", () => ({
  typedInvoke: vi.fn(),
}));

const jiraIssue: AgentConversationJiraIssue = {
  conversationId: "11111111-1111-1111-1111-111111111111",
  projectId: "project-1",
  provider: "atlassian",
  issueKey: "RX-42",
  issueId: "10042",
  issueUrl: "https://jira.test/browse/RX-42",
  title: "Fix Jira tab",
  status: "In Review",
  assignee: "Ada Lovelace",
  reporter: "Grace Hopper",
  updatedAtRemote: "2026-06-17T12:00:00Z",
  descriptionMarkdown: "Description",
  descriptionText: "Description",
  acceptanceCriteriaMarkdown: "- Visible",
  acceptanceCriteriaText: "Visible",
  comments: [
    {
      id: "c1",
      author: "Reviewer",
      bodyMarkdown: "Looks good",
      bodyText: "Looks good",
      createdAt: "2026-06-17T12:01:00Z",
      updatedAt: null,
    },
  ],
  attachments: [
    {
      id: "a1",
      filename: "design.png",
      mimeType: "image/png",
      size: 42,
      author: "Designer",
      contentUrl: "https://jira.test/attachment/a1",
      thumbnailUrl: null,
      createdAt: "2026-06-17T12:02:00Z",
    },
  ],
  lastRefreshedAt: "2026-06-17T12:03:00Z",
  refreshStatus: "loaded",
  refreshError: null,
  assignedAt: "2026-06-17T12:00:00Z",
  assignedFromMessageId: "msg-1",
  manuallyAssigned: true,
  createdAt: "2026-06-17T12:00:00Z",
  updatedAt: "2026-06-17T12:03:00Z",
};

describe("atlassianApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("returns resource arrays from the search response wrapper", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      resources: [
        {
          kind: "jira",
          id: "RX-42",
          key: "RX-42",
          title: "Fix Jira tab",
          url: "https://jira.test/browse/RX-42",
          excerpt: null,
        },
      ],
    });

    await expect(
      atlassianApi.searchResources({
        kind: "jira",
        query: "RX-42",
        limit: 5,
      }),
    ).resolves.toEqual([
      {
        kind: "jira",
        id: "RX-42",
        key: "RX-42",
        title: "Fix Jira tab",
        url: "https://jira.test/browse/RX-42",
        excerpt: null,
      },
    ]);
    expect(typedInvoke).toHaveBeenCalledWith(
      "search_atlassian_resources",
      { input: { kind: "jira", query: "RX-42", limit: 5 } },
      expect.any(Object),
    );
  });

  it("returns per-url resource resolutions from the URL resolver wrapper", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      results: [
        {
          inputUrl: "https://example.atlassian.net/browse/RX-42",
          resource: {
            kind: "jira",
            id: "RX-42",
            key: "RX-42",
            title: "Fix Jira tab",
            url: "https://example.atlassian.net/browse/RX-42",
            excerpt: null,
          },
        },
        {
          inputUrl: "https://other.atlassian.net/browse/RX-99",
          resource: null,
        },
      ],
    });

    await expect(
      atlassianApi.resolveResourceUrls({
        urls: [
          "https://example.atlassian.net/browse/RX-42",
          "https://other.atlassian.net/browse/RX-99",
        ],
      }),
    ).resolves.toEqual([
      {
        inputUrl: "https://example.atlassian.net/browse/RX-42",
        resource: {
          kind: "jira",
          id: "RX-42",
          key: "RX-42",
          title: "Fix Jira tab",
          url: "https://example.atlassian.net/browse/RX-42",
          excerpt: null,
        },
      },
      {
        inputUrl: "https://other.atlassian.net/browse/RX-99",
        resource: null,
      },
    ]);
    expect(typedInvoke).toHaveBeenCalledWith(
      "resolve_atlassian_resource_urls",
      {
        input: {
          urls: [
            "https://example.atlassian.net/browse/RX-42",
            "https://other.atlassian.net/browse/RX-99",
          ],
        },
      },
      expect.any(Object),
    );
  });

  it("normalizes nullable Jira assignment command responses", async () => {
    const input = {
      conversationId: "11111111-1111-1111-1111-111111111111",
    };
    vi.mocked(typedInvoke)
      .mockResolvedValueOnce({ issue: null })
      .mockResolvedValueOnce({ issue: jiraIssue })
      .mockResolvedValueOnce({ issue: jiraIssue })
      .mockResolvedValueOnce({ issue: jiraIssue })
      .mockResolvedValueOnce({});

    await expect(atlassianApi.getAgentConversationJiraIssue(input)).resolves.toBeNull();
    await expect(
      atlassianApi.assignAgentConversationJiraIssue({
        ...input,
        issueKey: "RX-42",
      }),
    ).resolves.toEqual(jiraIssue);
    await expect(atlassianApi.refreshAgentConversationJiraIssue(input)).resolves.toEqual(
      jiraIssue,
    );
    await expect(atlassianApi.assignAgentConversationJiraIssueToMe(input)).resolves.toEqual(
      jiraIssue,
    );
    await expect(atlassianApi.clearAgentConversationJiraIssue(input)).resolves.toBeNull();

    expect(typedInvoke).toHaveBeenNthCalledWith(
      1,
      "get_agent_conversation_jira_issue",
      { input },
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      2,
      "assign_agent_conversation_jira_issue",
      { input: { ...input, issueKey: "RX-42" } },
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      3,
      "refresh_agent_conversation_jira_issue",
      { input },
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      4,
      "assign_agent_conversation_jira_issue_to_me",
      { input },
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      5,
      "clear_agent_conversation_jira_issue",
      { input },
      expect.any(Object),
    );
  });
});
