import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { repositorySettingsApi } from "./repository-settings";

describe("repositorySettingsApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads and transforms the default token-removal preference", async () => {
    vi.mocked(invoke).mockResolvedValue({
      remove_inherited_github_cli_tokens: true,
    });

    await expect(repositorySettingsApi.get()).resolves.toEqual({
      removeInheritedGithubCliTokens: true,
    });
    expect(invoke).toHaveBeenCalledWith("get_repository_settings", {});
  });

  it("sends the explicit opt-out using the command input contract", async () => {
    vi.mocked(invoke).mockResolvedValue({
      remove_inherited_github_cli_tokens: false,
    });

    await expect(
      repositorySettingsApi.update({ removeInheritedGithubCliTokens: false }),
    ).resolves.toEqual({ removeInheritedGithubCliTokens: false });
    expect(invoke).toHaveBeenCalledWith("update_repository_settings", {
      input: { removeInheritedGithubCliTokens: false },
    });
  });
});
