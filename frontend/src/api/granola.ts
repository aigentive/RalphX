import { z } from "zod";

import { typedInvoke } from "@/lib/tauri";

export const GranolaValidationStatusSchema = z.enum([
  "not_configured",
  "pending",
  "valid",
  "invalid",
]);

export type GranolaValidationStatus = z.infer<
  typeof GranolaValidationStatusSchema
>;

export const GranolaIntegrationSettingsSchema = z.object({
  enabled: z.boolean(),
  hasApiToken: z.boolean(),
  validationStatus: GranolaValidationStatusSchema,
  lastValidatedAt: z.string().nullable().optional(),
  lastError: z.string().nullable().optional(),
  updatedAt: z.string(),
});

export type GranolaIntegrationSettings = z.infer<
  typeof GranolaIntegrationSettingsSchema
>;

export interface SaveGranolaIntegrationSettingsInput {
  apiToken?: string | null;
}

export const granolaApi = {
  getSettings(): Promise<GranolaIntegrationSettings> {
    return typedInvoke(
      "get_granola_integration_settings",
      {},
      GranolaIntegrationSettingsSchema,
    );
  },

  saveSettings(
    input: SaveGranolaIntegrationSettingsInput,
  ): Promise<GranolaIntegrationSettings> {
    return typedInvoke(
      "save_granola_integration_settings",
      { input },
      GranolaIntegrationSettingsSchema,
    );
  },

  validate(): Promise<GranolaIntegrationSettings> {
    return typedInvoke(
      "validate_granola_integration_settings",
      {},
      GranolaIntegrationSettingsSchema,
    );
  },

  disconnect(): Promise<GranolaIntegrationSettings> {
    return typedInvoke(
      "save_granola_integration_settings",
      { input: { apiToken: "" } },
      GranolaIntegrationSettingsSchema,
    );
  },
} as const;
