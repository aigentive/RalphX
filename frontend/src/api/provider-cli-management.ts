import { typedInvoke } from "@/lib/tauri";
import { z } from "zod";

import { HarnessSchema } from "./ideation-harness";

export const ManagedProviderCliActionSchema = z.enum([
  "none",
  "install",
  "update",
  "unsupported",
]);

export const ManagedProviderCliStatusResponseSchema = z.object({
  provider: HarnessSchema,
  cliManagementMode: z.enum(["user_managed", "rx_managed"]),
  autoUpdateEnabled: z.boolean(),
  supported: z.boolean(),
  installed: z.boolean(),
  binaryPath: z.string().nullable().optional(),
  currentVersion: z.string().nullable().optional(),
  latestVersion: z.string().nullable().optional(),
  updateAvailable: z.boolean(),
  action: ManagedProviderCliActionSchema,
  status: z.string(),
  error: z.string().nullable().optional(),
});

export type ManagedProviderCliStatusResponse = z.infer<
  typeof ManagedProviderCliStatusResponseSchema
>;

export const ManagedProviderCliStatusesResponseSchema = z.object({
  providers: z.array(ManagedProviderCliStatusResponseSchema),
});

export type ManagedProviderCliStatusesResponse = z.infer<
  typeof ManagedProviderCliStatusesResponseSchema
>;

export const ManagedProviderCliActionResponseSchema = z.object({
  provider: HarnessSchema,
  success: z.boolean(),
  status: ManagedProviderCliStatusResponseSchema,
  stdout: z.string().nullable().optional(),
  stderr: z.string().nullable().optional(),
});

export type ManagedProviderCliActionResponse = z.infer<
  typeof ManagedProviderCliActionResponseSchema
>;

export const ManagedProviderCliAutoUpdateResponseSchema = z.object({
  updated: z.array(ManagedProviderCliActionResponseSchema),
  skipped: z.array(ManagedProviderCliStatusResponseSchema),
});

export type ManagedProviderCliAutoUpdateResponse = z.infer<
  typeof ManagedProviderCliAutoUpdateResponseSchema
>;

export interface ManagedProviderCliActionInput {
  provider: string;
}

export const providerCliManagementApi = {
  status(): Promise<ManagedProviderCliStatusesResponse> {
    return typedInvoke(
      "get_managed_provider_cli_status",
      {},
      ManagedProviderCliStatusesResponseSchema,
    );
  },

  installOrUpdate(
    input: ManagedProviderCliActionInput,
  ): Promise<ManagedProviderCliActionResponse> {
    return typedInvoke(
      "install_or_update_managed_provider_cli",
      { input },
      ManagedProviderCliActionResponseSchema,
    );
  },

  autoUpdate(): Promise<ManagedProviderCliAutoUpdateResponse> {
    return typedInvoke(
      "auto_update_managed_provider_clis",
      {},
      ManagedProviderCliAutoUpdateResponseSchema,
    );
  },
} as const;
