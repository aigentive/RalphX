import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

const RedactionEntrySchema = z.object({
  category: z.string(),
  count: z.number(),
});

const RedactionSummarySchema = z.object({
  replacements: z.array(RedactionEntrySchema),
});

const AgentIssueReportDestinationSchema = z.object({
  repository: z.string(),
  source: z.enum(["configured", "public_default"]),
  isDefault: z.boolean(),
});

const AgentIssueReportSourceSchema = z.object({
  label: z.string(),
  included: z.boolean(),
  truncated: z.boolean(),
  detail: z.string().nullable().optional(),
});

const AgentIssueReportDraftSchema = z.object({
  conversationId: z.string(),
  projectId: z.string(),
  generatedAt: z.string(),
  markdown: z.string(),
  destination: AgentIssueReportDestinationSchema,
  redactionSummary: RedactionSummarySchema,
  sources: z.array(AgentIssueReportSourceSchema),
  warnings: z.array(z.string()),
});

const AgentIssueReportSubmitResponseSchema = z.object({
  repository: z.string(),
  issueUrl: z.string(),
});

export type AgentIssueReportDestination = z.infer<
  typeof AgentIssueReportDestinationSchema
>;
export type AgentIssueReportDraft = z.infer<typeof AgentIssueReportDraftSchema>;
export type AgentIssueReportSubmitResponse = z.infer<
  typeof AgentIssueReportSubmitResponseSchema
>;

export interface BuildAgentIssueReportInput {
  conversationId: string;
  projectId?: string;
  includeLogs?: boolean;
  recentErrorsOnly?: boolean;
  maxLogBytes?: number;
}

export interface SubmitAgentIssueReportInput {
  conversationId: string;
  repository: string;
  title: string;
  bodyMarkdown: string;
}

export const agentIssueReportApi = {
  build: async (
    input: BuildAgentIssueReportInput,
  ): Promise<AgentIssueReportDraft> => {
    const response = await invoke<unknown>("build_agent_issue_report", {
      input,
    });
    return AgentIssueReportDraftSchema.parse(response);
  },

  submit: async (
    input: SubmitAgentIssueReportInput,
  ): Promise<AgentIssueReportSubmitResponse> => {
    const response = await invoke<unknown>("submit_agent_issue_report", {
      input,
    });
    return AgentIssueReportSubmitResponseSchema.parse(response);
  },
} as const;
