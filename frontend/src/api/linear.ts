import { z } from "zod";

import { typedInvoke } from "@/lib/tauri";

export const LinearWebhookConfigSchema = z.object({
  enabled: z.boolean(),
  hasSigningSecret: z.boolean(),
});

export type LinearWebhookConfig = z.infer<typeof LinearWebhookConfigSchema>;

export interface SaveLinearWebhookSigningSecretInput {
  signingSecret: string;
  enabled?: boolean;
}

export const linearApi = {
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
