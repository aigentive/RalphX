import { z } from "zod";

import { typedInvoke } from "@/lib/tauri";

/**
 * ClickUp ticketing-provider settings API.
 *
 * Mirrors `api/linear.ts` (single Personal API token, no OAuth) and adds a
 * Workspace (Team) selector. Command names and argument shapes match the
 * backend `clickup_commands.rs`: settings mutations are wrapped under `input`
 * and every field is camelCase (Tauri input structs use
 * `#[serde(rename_all = "camelCase")]`). The settings response struct also uses
 * camelCase, so the schemas parse camelCase directly.
 */

export const ClickUpValidationStatusSchema = z.enum([
  "not_configured",
  "pending",
  "valid",
  "invalid",
]);

export type ClickUpValidationStatus = z.infer<
  typeof ClickUpValidationStatusSchema
>;

export const ClickUpIntegrationSettingsSchema = z.object({
  enabled: z.boolean(),
  hasApiToken: z.boolean(),
  workspaceId: z.string().nullable().optional(),
  validationStatus: ClickUpValidationStatusSchema,
  taskSearchAvailable: z.boolean(),
  lastValidatedAt: z.string().nullable().optional(),
  lastError: z.string().nullable().optional(),
  updatedAt: z.string(),
});

export type ClickUpIntegrationSettings = z.infer<
  typeof ClickUpIntegrationSettingsSchema
>;

export const ClickUpWorkspaceSchema = z.object({
  id: z.string(),
  name: z.string(),
  color: z.string().nullable().optional(),
});

export type ClickUpWorkspace = z.infer<typeof ClickUpWorkspaceSchema>;

export const ClickUpTaskSummarySchema = z.object({
  id: z.string(),
  customId: z.string().nullable().optional(),
  name: z.string(),
  url: z.string().nullable().optional(),
  statusName: z.string().nullable().optional(),
  statusType: z.string().nullable().optional(),
  statusCategory: z.string().nullable().optional(),
  statusColor: z.string().nullable().optional(),
  assignees: z.array(z.string()).default([]),
  tags: z.array(z.string()).default([]),
  spaceId: z.string().nullable().optional(),
  listName: z.string().nullable().optional(),
  updatedAt: z.string().nullable().optional(),
});

export type ClickUpTaskSummary = z.infer<typeof ClickUpTaskSummarySchema>;

export const ListClickUpWorkspacesResponseSchema = z.object({
  workspaces: z.array(ClickUpWorkspaceSchema),
});

export const SearchClickUpTasksResponseSchema = z.object({
  tasks: z.array(ClickUpTaskSummarySchema),
});

export interface SaveClickUpIntegrationSettingsInput {
  /**
   * Tri-state: omit to leave the stored token untouched, `""` to clear it,
   * a value to replace it. The raw token never round-trips back to the UI.
   */
  apiToken?: string | null;
  /** Tri-state mirror for the selected Workspace (Team) id. */
  workspaceId?: string | null;
}

export interface SearchClickUpTasksInput {
  /** ClickUp Space ids to scope the search to within the selected workspace. */
  spaceIds?: string[];
}

export const clickupApi = {
  getSettings(): Promise<ClickUpIntegrationSettings> {
    return typedInvoke(
      "get_clickup_integration_settings",
      {},
      ClickUpIntegrationSettingsSchema,
    );
  },

  saveSettings(
    input: SaveClickUpIntegrationSettingsInput,
  ): Promise<ClickUpIntegrationSettings> {
    return typedInvoke(
      "save_clickup_integration_settings",
      { input },
      ClickUpIntegrationSettingsSchema,
    );
  },

  validate(): Promise<ClickUpIntegrationSettings> {
    return typedInvoke(
      "validate_clickup_integration",
      {},
      ClickUpIntegrationSettingsSchema,
    );
  },

  disconnect(): Promise<ClickUpIntegrationSettings> {
    return typedInvoke(
      "disconnect_clickup_integration",
      {},
      ClickUpIntegrationSettingsSchema,
    );
  },

  async listWorkspaces(): Promise<ClickUpWorkspace[]> {
    const response = await typedInvoke(
      "list_clickup_workspaces",
      {},
      ListClickUpWorkspacesResponseSchema,
    );
    return response.workspaces;
  },

  async searchTasks(
    input: SearchClickUpTasksInput = {},
  ): Promise<ClickUpTaskSummary[]> {
    const response = await typedInvoke(
      "search_clickup_tasks",
      { input: { spaceIds: input.spaceIds ?? [] } },
      SearchClickUpTasksResponseSchema,
    );
    return response.tasks;
  },
} as const;
