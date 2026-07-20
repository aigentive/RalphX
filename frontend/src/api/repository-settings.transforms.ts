import type { RepositorySettingsRaw } from "./repository-settings.schemas";
import type { RepositorySettings } from "./repository-settings.types";

export function transformRepositorySettings(
  raw: RepositorySettingsRaw,
): RepositorySettings {
  return {
    removeInheritedGithubCliTokens: raw.remove_inherited_github_cli_tokens,
  };
}
