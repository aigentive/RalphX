import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { agentIssueReportApi } from "./agent-issue-report";

const mockInvoke = invoke as ReturnType<typeof vi.fn>;

describe("agentIssueReportApi", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("builds an issue report draft through Tauri", async () => {
    mockInvoke.mockResolvedValue({
      conversationId: "conversation-1",
      projectId: "project-1",
      generatedAt: "2026-06-19T12:00:00Z",
      markdown: "# Report",
      destination: {
        repository: "aigentive/ralphx.app",
        source: "public_default",
        isDefault: true,
      },
      redactionSummary: {
        replacements: [{ category: "home_path", count: 1 }],
      },
      sources: [
        {
          label: "stream-debug/conversation.log",
          included: true,
          truncated: false,
          detail: null,
        },
      ],
      warnings: ["Using public default repository."],
    });

    const draft = await agentIssueReportApi.build({
      conversationId: "conversation-1",
      projectId: "project-1",
      includeLogs: true,
      recentErrorsOnly: true,
      maxLogBytes: 4096,
    });

    expect(mockInvoke).toHaveBeenCalledWith("build_agent_issue_report", {
      input: {
        conversationId: "conversation-1",
        projectId: "project-1",
        includeLogs: true,
        recentErrorsOnly: true,
        maxLogBytes: 4096,
      },
    });
    expect(draft.destination.source).toBe("public_default");
    expect(draft.redactionSummary.replacements[0]).toEqual({
      category: "home_path",
      count: 1,
    });
  });

  it("submits an issue report through Tauri", async () => {
    mockInvoke.mockResolvedValue({
      repository: "aigentive/ralphx.app",
      issueUrl: "https://github.com/aigentive/ralphx.app/issues/42",
    });

    const response = await agentIssueReportApi.submit({
      conversationId: "conversation-1",
      repository: "aigentive/ralphx.app",
      title: "Support report",
      bodyMarkdown: "# Edited",
    });

    expect(mockInvoke).toHaveBeenCalledWith("submit_agent_issue_report", {
      input: {
        conversationId: "conversation-1",
        repository: "aigentive/ralphx.app",
        title: "Support report",
        bodyMarkdown: "# Edited",
      },
    });
    expect(response.issueUrl).toBe("https://github.com/aigentive/ralphx.app/issues/42");
  });
});
