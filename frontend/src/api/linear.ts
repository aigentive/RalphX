import { z } from "zod";

import { typedInvoke } from "@/lib/tauri";

export const LinearWebhookConfigSchema = z.object({
  enabled: z.boolean(),
  hasSigningSecret: z.boolean(),
});

export type LinearWebhookConfig = z.infer<typeof LinearWebhookConfigSchema>;

export const LinearValidationStatusSchema = z.enum([
  "not_configured",
  "pending",
  "valid",
  "invalid",
]);

export const LinearIntegrationSettingsSchema = z.object({
  enabled: z.boolean(),
  hasApiToken: z.boolean(),
  validationStatus: LinearValidationStatusSchema,
  issueSearchAvailable: z.boolean(),
  lastValidatedAt: z.string().nullable().optional(),
  lastError: z.string().nullable().optional(),
  updatedAt: z.string(),
});

export type LinearIntegrationSettings = z.infer<
  typeof LinearIntegrationSettingsSchema
>;

export const LinearIssueSummarySchema = z.object({
  id: z.string(),
  key: z.string().nullable().optional(),
  title: z.string(),
  url: z.string().nullable().optional(),
  excerpt: z.string().nullable().optional(),
  stateName: z.string().nullable().optional(),
});

export type LinearIssueSummary = z.infer<typeof LinearIssueSummarySchema>;

export const AgentConversationLinearIssueSchema = z.object({
  conversationId: z.string(),
  projectId: z.string(),
  provider: z.literal("linear"),
  issueId: z.string(),
  issueKey: z.string().nullable().optional(),
  issueUrl: z.string().nullable().optional(),
  title: z.string().nullable().optional(),
  status: z.string().nullable().optional(),
  assignee: z.string().nullable().optional(),
  reporter: z.string().nullable().optional(),
  updatedAtRemote: z.string().nullable().optional(),
  descriptionMarkdown: z.string().nullable().optional(),
  descriptionText: z.string().nullable().optional(),
  comments: z.array(z.unknown()).default([]),
  attachments: z.array(z.unknown()).default([]),
  lastRefreshedAt: z.string().nullable().optional(),
  refreshStatus: z.enum(["not_loaded", "loaded", "error"]),
  refreshError: z.string().nullable().optional(),
  assignedAt: z.string(),
  assignedFromMessageId: z.string().nullable().optional(),
  manuallyAssigned: z.boolean(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

export type AgentConversationLinearIssue = z.infer<
  typeof AgentConversationLinearIssueSchema
>;

export const SearchLinearIssuesResponseSchema = z.object({
  issues: z.array(LinearIssueSummarySchema),
});

export const AgentConversationLinearIssueResponseSchema = z.object({
  issue: AgentConversationLinearIssueSchema.nullable().optional(),
});

export interface SaveLinearWebhookSigningSecretInput {
  signingSecret: string;
  enabled?: boolean;
}

export interface SaveLinearIntegrationSettingsInput {
  apiToken?: string | null;
}

export interface SearchLinearIssuesInput {
  query: string;
  limit?: number;
}

export interface AgentConversationLinearIssueConversationInput {
  conversationId: string;
}

export interface AssignAgentConversationLinearIssueInput {
  conversationId: string;
  projectId?: string | null;
  issueId: string;
  issueKey?: string | null;
  title?: string | null;
  issueUrl?: string | null;
  refresh?: boolean;
}

export const linearApi = {
  getSettings(): Promise<LinearIntegrationSettings> {
    return typedInvoke(
      "get_linear_integration_settings",
      {},
      LinearIntegrationSettingsSchema,
    );
  },

  saveSettings(
    input: SaveLinearIntegrationSettingsInput,
  ): Promise<LinearIntegrationSettings> {
    return typedInvoke(
      "save_linear_integration_settings",
      { input },
      LinearIntegrationSettingsSchema,
    );
  },

  validate(): Promise<LinearIntegrationSettings> {
    return typedInvoke(
      "validate_linear_integration",
      {},
      LinearIntegrationSettingsSchema,
    );
  },

  disconnect(): Promise<LinearIntegrationSettings> {
    return typedInvoke(
      "disconnect_linear_integration",
      {},
      LinearIntegrationSettingsSchema,
    );
  },

  async searchIssues(
    input: SearchLinearIssuesInput,
  ): Promise<LinearIssueSummary[]> {
    const response = await typedInvoke(
      "search_linear_issues",
      { input },
      SearchLinearIssuesResponseSchema,
    );
    return response.issues;
  },

  async getAgentConversationLinearIssue(
    input: AgentConversationLinearIssueConversationInput,
  ): Promise<AgentConversationLinearIssue | null> {
    const response = await typedInvoke(
      "get_agent_conversation_linear_issue",
      { input },
      AgentConversationLinearIssueResponseSchema,
    );
    return response.issue ?? null;
  },

  async assignAgentConversationLinearIssue(
    input: AssignAgentConversationLinearIssueInput,
  ): Promise<AgentConversationLinearIssue | null> {
    const response = await typedInvoke(
      "assign_agent_conversation_linear_issue",
      { input },
      AgentConversationLinearIssueResponseSchema,
    );
    return response.issue ?? null;
  },

  async refreshAgentConversationLinearIssue(
    input: AgentConversationLinearIssueConversationInput,
  ): Promise<AgentConversationLinearIssue | null> {
    const response = await typedInvoke(
      "refresh_agent_conversation_linear_issue",
      { input },
      AgentConversationLinearIssueResponseSchema,
    );
    return response.issue ?? null;
  },

  async clearAgentConversationLinearIssue(
    input: AgentConversationLinearIssueConversationInput,
  ): Promise<AgentConversationLinearIssue | null> {
    const response = await typedInvoke(
      "clear_agent_conversation_linear_issue",
      { input },
      AgentConversationLinearIssueResponseSchema,
    );
    return response.issue ?? null;
  },

  getWebhookConfig(): Promise<LinearWebhookConfig> {
    return typedInvoke(
      "get_linear_webhook_config",
      {},
      LinearWebhookConfigSchema,
    );
  },

  saveWebhookSigningSecret(
    input: SaveLinearWebhookSigningSecretInput,
  ): Promise<LinearWebhookConfig> {
    return typedInvoke(
      "save_linear_webhook_signing_secret",
      { input },
      LinearWebhookConfigSchema,
    );
  },
} as const;
