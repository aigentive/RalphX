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
} as const;
