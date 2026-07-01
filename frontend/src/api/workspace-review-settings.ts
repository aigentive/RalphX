import { typedInvoke } from "@/lib/tauri";
import { z } from "zod";

import { HarnessSchema, type KnownHarness } from "./ideation-harness";

export const WorkspaceReviewRuntimeSettingsResponseSchema = z.object({
  projectId: z.string().nullable().optional(),
  provider: HarnessSchema,
  model: z.string().nullable().optional(),
  effort: z.string().nullable().optional(),
  updatedAt: z.string(),
});

export type WorkspaceReviewRuntimeSettingsResponse = z.infer<
  typeof WorkspaceReviewRuntimeSettingsResponseSchema
>;

export interface UpdateWorkspaceReviewRuntimeSettingsInput {
  projectId: string | null;
  provider: KnownHarness;
  model?: string | null;
  effort?: string | null;
}

export interface WorkspaceReviewUtilityDefaults {
  model: string;
  effort: string;
}

export function workspaceReviewUtilityDefaultsForProvider(
  provider: KnownHarness,
): WorkspaceReviewUtilityDefaults {
  if (provider === "codex") {
    return { model: "gpt-5.4-mini", effort: "medium" };
  }
  return { model: "haiku", effort: "medium" };
}

export const workspaceReviewSettingsApi = {
  list(projectId: string | null): Promise<WorkspaceReviewRuntimeSettingsResponse[]> {
    return typedInvoke(
      "get_workspace_review_runtime_settings",
      { projectId },
      z.array(WorkspaceReviewRuntimeSettingsResponseSchema),
    );
  },

  update(
    input: UpdateWorkspaceReviewRuntimeSettingsInput,
  ): Promise<WorkspaceReviewRuntimeSettingsResponse> {
    return typedInvoke(
      "update_workspace_review_runtime_settings",
      { input },
      WorkspaceReviewRuntimeSettingsResponseSchema,
    );
  },
} as const;
