import type {
  AgentIssueReportDraft,
  AgentIssueReportSubmitResponse,
  BuildAgentIssueReportInput,
  SubmitAgentIssueReportInput,
} from "@/api/agent-issue-report";

export const mockAgentIssueReportApi = {
  build: async (
    input: BuildAgentIssueReportInput,
  ): Promise<AgentIssueReportDraft> => ({
    conversationId: input.conversationId,
    projectId: input.projectId ?? "project-demo",
    generatedAt: new Date().toISOString(),
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
        label: "stream-debug/conversation-demo.log",
        included: true,
        truncated: false,
        detail: null,
      },
    ],
    warnings: [],
    markdown: `# RalphX Issue Report

Review and edit this report before submitting. The submitted issue body is exactly the Markdown shown here.

## User Notes

_Add a short description of what went wrong, what you expected, and steps to reproduce._

## Submission Target

- Repository: \`aigentive/ralphx.app\` (public default destination)

## Agent Conversation

- Conversation ID: \`${input.conversationId}\`
- Project ID: \`${input.projectId ?? "project-demo"}\`

## Logs

~~~text
Example redacted log from $HOME/.artifacts/logs.
~~~
`,
  }),

  submit: async (
    input: SubmitAgentIssueReportInput,
  ): Promise<AgentIssueReportSubmitResponse> => ({
    repository: input.repository,
    issueUrl: `https://github.com/${input.repository}/issues/1`,
  }),
} as const;
