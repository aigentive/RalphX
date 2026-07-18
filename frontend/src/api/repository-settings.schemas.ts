import { z } from "zod";

export const RepositorySettingsSchema = z.object({
  remove_inherited_github_cli_tokens: z.boolean().nullish().transform((value) => value ?? true),
});

export type RepositorySettingsRaw = z.infer<typeof RepositorySettingsSchema>;
