import { z } from "zod";

import { typedInvoke } from "@/lib/tauri";

export const AtlassianValidationStatusSchema = z.enum([
  "not_configured",
  "pending",
  "valid",
  "invalid",
]);

export const AtlassianIntegrationSettingsSchema = z.object({
  enabled: z.boolean(),
  authMethod: z.enum(["api_token", "oauth"]),
  siteUrl: z.string().nullable().optional(),
  email: z.string().nullable().optional(),
  hasApiToken: z.boolean(),
  oauthClientId: z.string().nullable().optional(),
  oauthRedirectUri: z.string().nullable().optional(),
  hasOauthClientSecret: z.boolean(),
  hasOauthToken: z.boolean(),
  oauthCloudId: z.string().nullable().optional(),
  oauthScopes: z.string().nullable().optional(),
  validationStatus: AtlassianValidationStatusSchema,
  jiraAvailable: z.boolean(),
  confluenceAvailable: z.boolean(),
  lastValidatedAt: z.string().nullable().optional(),
  lastError: z.string().nullable().optional(),
  updatedAt: z.string(),
});

export type AtlassianIntegrationSettings = z.infer<
  typeof AtlassianIntegrationSettingsSchema
>;

export const AtlassianResourceKindSchema = z.enum(["jira", "confluence"]);
export type AtlassianResourceKind = z.infer<typeof AtlassianResourceKindSchema>;

export const AtlassianResourceSummarySchema = z.object({
  kind: AtlassianResourceKindSchema,
  id: z.string(),
  key: z.string().nullable().optional(),
  title: z.string(),
  url: z.string().nullable().optional(),
  excerpt: z.string().nullable().optional(),
});

export type AtlassianResourceSummary = z.infer<typeof AtlassianResourceSummarySchema>;

export const SearchAtlassianResourcesResponseSchema = z.object({
  resources: z.array(AtlassianResourceSummarySchema),
});

export const AtlassianResourceUrlResolutionSchema = z.object({
  inputUrl: z.string(),
  resource: AtlassianResourceSummarySchema.nullable().optional(),
});

export type AtlassianResourceUrlResolution = z.infer<
  typeof AtlassianResourceUrlResolutionSchema
>;

export const ResolveAtlassianResourceUrlsResponseSchema = z.object({
  results: z.array(AtlassianResourceUrlResolutionSchema),
});

export const AtlassianJiraCommentSchema = z.object({
  id: z.string().nullable().optional(),
  author: z.string().nullable().optional(),
  bodyMarkdown: z.string(),
  bodyText: z.string(),
  createdAt: z.string().nullable().optional(),
  updatedAt: z.string().nullable().optional(),
});

export type AtlassianJiraComment = z.infer<typeof AtlassianJiraCommentSchema>;

export const AtlassianJiraAttachmentSchema = z.object({
  id: z.string().nullable().optional(),
  filename: z.string(),
  mimeType: z.string().nullable().optional(),
  size: z.number().nullable().optional(),
  author: z.string().nullable().optional(),
  contentUrl: z.string().nullable().optional(),
  thumbnailUrl: z.string().nullable().optional(),
  createdAt: z.string().nullable().optional(),
});

export type AtlassianJiraAttachment = z.infer<typeof AtlassianJiraAttachmentSchema>;

export const AgentConversationJiraIssueSchema = z.object({
  conversationId: z.string(),
  projectId: z.string(),
  provider: z.string(),
  issueKey: z.string(),
  issueId: z.string().nullable().optional(),
  issueUrl: z.string().nullable().optional(),
  title: z.string().nullable().optional(),
  status: z.string().nullable().optional(),
  assignee: z.string().nullable().optional(),
  reporter: z.string().nullable().optional(),
  updatedAtRemote: z.string().nullable().optional(),
  descriptionMarkdown: z.string().nullable().optional(),
  descriptionText: z.string().nullable().optional(),
  acceptanceCriteriaMarkdown: z.string().nullable().optional(),
  acceptanceCriteriaText: z.string().nullable().optional(),
  comments: z.array(AtlassianJiraCommentSchema),
  attachments: z.array(AtlassianJiraAttachmentSchema),
  lastRefreshedAt: z.string().nullable().optional(),
  refreshStatus: z.enum(["not_loaded", "loaded", "error"]),
  refreshError: z.string().nullable().optional(),
  assignedAt: z.string(),
  assignedFromMessageId: z.string().nullable().optional(),
  manuallyAssigned: z.boolean(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

export type AgentConversationJiraIssue = z.infer<
  typeof AgentConversationJiraIssueSchema
>;

export const AgentConversationJiraIssueResponseSchema = z.object({
  issue: AgentConversationJiraIssueSchema.nullable().optional(),
});

export const AtlassianOAuthAuthorizationSchema = z.object({
  authorizationUrl: z.string(),
  state: z.string(),
  scopes: z.string(),
  redirectUri: z.string(),
});

export type AtlassianOAuthAuthorization = z.infer<
  typeof AtlassianOAuthAuthorizationSchema
>;

export interface SaveAtlassianIntegrationSettingsInput {
  authMethod?: "api_token" | "oauth";
  siteUrl?: string | null;
  email?: string | null;
  apiToken?: string | null;
  oauthClientId?: string | null;
  oauthClientSecret?: string | null;
  oauthRedirectUri?: string | null;
}

export interface ExchangeAtlassianOAuthCodeInput {
  authorizationCode: string;
}

export interface CompleteAtlassianOAuthLocalCallbackInput {
  state: string;
}

export interface SearchAtlassianResourcesInput {
  kind: AtlassianResourceKind;
  query: string;
  limit?: number;
}

export interface ResolveAtlassianResourceUrlsInput {
  urls: string[];
}

export interface AgentConversationJiraIssueConversationInput {
  conversationId: string;
}

export interface AssignAgentConversationJiraIssueInput {
  conversationId: string;
  projectId?: string | null;
  issueKey: string;
  issueId?: string | null;
  title?: string | null;
  issueUrl?: string | null;
  refresh?: boolean;
}

export const atlassianApi = {
  getSettings(): Promise<AtlassianIntegrationSettings> {
    return typedInvoke(
      "get_atlassian_integration_settings",
      {},
      AtlassianIntegrationSettingsSchema,
    );
  },

  saveSettings(
    input: SaveAtlassianIntegrationSettingsInput,
  ): Promise<AtlassianIntegrationSettings> {
    return typedInvoke(
      "save_atlassian_integration_settings",
      { input },
      AtlassianIntegrationSettingsSchema,
    );
  },

  validate(): Promise<AtlassianIntegrationSettings> {
    return typedInvoke(
      "validate_atlassian_integration",
      {},
      AtlassianIntegrationSettingsSchema,
    );
  },

  disconnect(): Promise<AtlassianIntegrationSettings> {
    return typedInvoke(
      "disconnect_atlassian_integration",
      {},
      AtlassianIntegrationSettingsSchema,
    );
  },

  buildOAuthAuthorization(): Promise<AtlassianOAuthAuthorization> {
    return typedInvoke(
      "build_atlassian_oauth_authorization_url",
      {},
      AtlassianOAuthAuthorizationSchema,
    );
  },

  startOAuthLocalCallback(): Promise<AtlassianOAuthAuthorization> {
    return typedInvoke(
      "start_atlassian_oauth_local_callback",
      {},
      AtlassianOAuthAuthorizationSchema,
    );
  },

  completeOAuthLocalCallback(
    input: CompleteAtlassianOAuthLocalCallbackInput,
  ): Promise<AtlassianIntegrationSettings> {
    return typedInvoke(
      "complete_atlassian_oauth_local_callback",
      { input },
      AtlassianIntegrationSettingsSchema,
    );
  },

  exchangeOAuthCode(
    input: ExchangeAtlassianOAuthCodeInput,
  ): Promise<AtlassianIntegrationSettings> {
    return typedInvoke(
      "exchange_atlassian_oauth_code",
      { input },
      AtlassianIntegrationSettingsSchema,
    );
  },

  async searchResources(
    input: SearchAtlassianResourcesInput,
  ): Promise<AtlassianResourceSummary[]> {
    const response = await typedInvoke(
      "search_atlassian_resources",
      { input },
      SearchAtlassianResourcesResponseSchema,
    );
    return response.resources;
  },

  async resolveResourceUrls(
    input: ResolveAtlassianResourceUrlsInput,
  ): Promise<AtlassianResourceUrlResolution[]> {
    const response = await typedInvoke(
      "resolve_atlassian_resource_urls",
      { input },
      ResolveAtlassianResourceUrlsResponseSchema,
    );
    return response.results;
  },

  async getAgentConversationJiraIssue(
    input: AgentConversationJiraIssueConversationInput,
  ): Promise<AgentConversationJiraIssue | null> {
    const response = await typedInvoke(
      "get_agent_conversation_jira_issue",
      { input },
      AgentConversationJiraIssueResponseSchema,
    );
    return response.issue ?? null;
  },

  async assignAgentConversationJiraIssue(
    input: AssignAgentConversationJiraIssueInput,
  ): Promise<AgentConversationJiraIssue | null> {
    const response = await typedInvoke(
      "assign_agent_conversation_jira_issue",
      { input },
      AgentConversationJiraIssueResponseSchema,
    );
    return response.issue ?? null;
  },

  async refreshAgentConversationJiraIssue(
    input: AgentConversationJiraIssueConversationInput,
  ): Promise<AgentConversationJiraIssue | null> {
    const response = await typedInvoke(
      "refresh_agent_conversation_jira_issue",
      { input },
      AgentConversationJiraIssueResponseSchema,
    );
    return response.issue ?? null;
  },

  async assignAgentConversationJiraIssueToMe(
    input: AgentConversationJiraIssueConversationInput,
  ): Promise<AgentConversationJiraIssue | null> {
    const response = await typedInvoke(
      "assign_agent_conversation_jira_issue_to_me",
      { input },
      AgentConversationJiraIssueResponseSchema,
    );
    return response.issue ?? null;
  },

  async clearAgentConversationJiraIssue(
    input: AgentConversationJiraIssueConversationInput,
  ): Promise<AgentConversationJiraIssue | null> {
    const response = await typedInvoke(
      "clear_agent_conversation_jira_issue",
      { input },
      AgentConversationJiraIssueResponseSchema,
    );
    return response.issue ?? null;
  },
} as const;
