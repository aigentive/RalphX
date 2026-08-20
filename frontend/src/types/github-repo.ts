// GitHub repository summary types and Zod schema, for the clone-time repo picker.
// Mirrors src-tauri/src/commands/project_clone_commands.rs::GitHubRepoSummary (already camelCase).

import { z } from "zod";

export const GitHubRepoSummarySchema = z.object({
  nameWithOwner: z.string(),
  description: z.string().nullable(),
  isPrivate: z.boolean(),
  updatedAt: z.string().nullable(),
});

export type GitHubRepoSummary = z.infer<typeof GitHubRepoSummarySchema>;
