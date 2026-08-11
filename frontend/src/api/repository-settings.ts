import { typedInvokeWithTransform } from "@/lib/tauri";

import { RepositorySettingsSchema } from "./repository-settings.schemas";
import { transformRepositorySettings } from "./repository-settings.transforms";
import type {
  RepositorySettings,
  UpdateRepositorySettingsInput,
} from "./repository-settings.types";

export const repositorySettingsApi = {
  get: (): Promise<RepositorySettings> =>
    typedInvokeWithTransform(
      "get_repository_settings",
      {},
      RepositorySettingsSchema,
      transformRepositorySettings,
    ),
  update: (input: UpdateRepositorySettingsInput): Promise<RepositorySettings> =>
    typedInvokeWithTransform(
      "update_repository_settings",
      { input },
      RepositorySettingsSchema,
      transformRepositorySettings,
    ),
} as const;

export type {
  RepositorySettings,
  UpdateRepositorySettingsInput,
} from "./repository-settings.types";
