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

export const SearchLinearIssuesResponseSchema = z.object({
  issues: z.array(LinearIssueSummarySchema),
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
