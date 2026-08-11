import { toast } from "sonner";

import {
  useRepositorySettings,
  useUpdateRepositorySettings,
} from "@/hooks/useRepositorySettings";

import { ToggleSettingRow } from "./SettingsView.shared";

const DEFAULT_REMOVE_INHERITED_GITHUB_CLI_TOKENS = true;

function describeError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "Failed to update repository environment settings";
}

export function RepositoryEnvironmentSettings() {
  const { data, isLoading } = useRepositorySettings();
  const updateSettings = useUpdateRepositorySettings();
  const enabled =
    data?.removeInheritedGithubCliTokens ??
    DEFAULT_REMOVE_INHERITED_GITHUB_CLI_TOKENS;

  const handleChange = async (removeInheritedGithubCliTokens: boolean) => {
    try {
      await updateSettings.mutateAsync({ removeInheritedGithubCliTokens });
      toast.success(
        removeInheritedGithubCliTokens
          ? "Inherited GitHub tokens will be removed from new processes"
          : "Inherited GitHub tokens will be passed to new processes",
      );
    } catch (error) {
      toast.error(describeError(error));
    }
  };

  return (
    <>
      <div className="settings-section__head">
        <span className="settings-section__label">Environment</span>
      </div>
      <ToggleSettingRow
        id="remove-inherited-github-cli-tokens"
        label="Remove inherited GitHub tokens"
        description="Prevent shell GH_TOKEN and GITHUB_TOKEN values from overriding credentials saved by gh auth login; disable only to pass those variables to new RalphX processes."
        checked={enabled}
        disabled={isLoading || updateSettings.isPending}
        onChange={(next) => void handleChange(next)}
      />
    </>
  );
}
