import { describe, expect, it } from "vitest";

import type { PullRequestIssueComment } from "@/api/github";

import { partitionIssueComments } from "./pullRequestComments";

function comment(
  overrides: Partial<PullRequestIssueComment> = {},
): PullRequestIssueComment {
  return {
    id: "c",
    author: "octocat",
    body: "hello",
    url: null,
    createdAt: null,
    updatedAt: null,
    isBot: false,
    isCodecov: false,
    source: "live",
    ...overrides,
  };
}

describe("partitionIssueComments", () => {
  it("keeps human comments and counts hidden bot/Codecov comments", () => {
    const result = partitionIssueComments([
      comment({ id: "human-1", author: "alice" }),
      comment({ id: "codecov", author: "codecov[bot]", isCodecov: true }),
      comment({ id: "bot", author: "dependabot[bot]", isBot: true }),
      comment({ id: "human-2", author: "bob" }),
    ]);

    expect(result.human.map((c) => c.id)).toEqual(["human-1", "human-2"]);
    expect(result.hiddenBotCount).toBe(2);
  });

  it("returns empty human list when every comment is automated", () => {
    const result = partitionIssueComments([
      comment({ id: "codecov", isCodecov: true }),
      comment({ id: "bot", isBot: true }),
    ]);

    expect(result.human).toEqual([]);
    expect(result.hiddenBotCount).toBe(2);
  });

  it("handles no comments", () => {
    expect(partitionIssueComments([])).toEqual({ human: [], hiddenBotCount: 0 });
  });
});
