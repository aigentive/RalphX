import { describe, expect, it } from "vitest";

import { mockAgentIssueReportApi } from "./agent-issue-report";

describe("mockAgentIssueReportApi", () => {
  it("returns a reviewable draft using the requested conversation context", async () => {
    const draft = await mockAgentIssueReportApi.build({
      conversationId: "conversation-12345678",
      projectId: "project-1",
      includeLogs: true,
    });

    expect(draft.conversationId).toBe("conversation-12345678");
    expect(draft.projectId).toBe("project-1");
    expect(draft.destination.repository).toBe("aigentive/ralphx.app");
    expect(draft.markdown).toContain("conversation-12345678");
  });

  it("returns the GitHub issue URL for the requested repository", async () => {
    const response = await mockAgentIssueReportApi.submit({
      conversationId: "conversation-12345678",
      repository: "aigentive/ralphx.app",
      title: "Support report",
      bodyMarkdown: "# Edited",
    });

    expect(response.repository).toBe("aigentive/ralphx.app");
    expect(response.issueUrl).toBe("https://github.com/aigentive/ralphx.app/issues/1");
  });
});
