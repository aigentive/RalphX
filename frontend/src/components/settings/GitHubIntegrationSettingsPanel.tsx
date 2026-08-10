import { Loader2, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { RemoteHostOnlyNotice } from "@/components/remote/RemoteHostOnlyNotice";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { useGitHubConnectionStatus } from "@/hooks/useGitHubConnectionStatus";

import {
  IntegrationStatusBanner,
  SettingsSection,
} from "./SettingsView.shared";

function describeError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  return "Unable to read GitHub CLI status";
}

export function GitHubIntegrationSettingsPanel() {
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const { data, error, isError, isLoading, isFetching, refetch } =
    useGitHubConnectionStatus();
  if (isRemoteEnvironment) {
    return <RemoteHostOnlyNotice subject="GitHub CLI status" detail="GitHub authentication is checked on the host and is not available remotely." />;
  }
  const ghInstalled = data?.ghInstalled ?? false;
  const state = data?.state;
  const connected = state === "authenticated";
  const statusTitle =
    state === "authenticated"
      ? "GitHub CLI authenticated"
      : state === "provider_unavailable"
        ? "GitHub temporarily unavailable"
        : state === "probe_failed"
          ? "GitHub access could not be verified"
          : state === "cli_unavailable" || !ghInstalled
            ? "GitHub CLI unavailable"
            : state === "credential_rejected"
              ? "GitHub CLI credential rejected"
              : "GitHub CLI not authenticated";
  const statusChips = [
    ghInstalled ? "gh installed" : "gh missing",
    connected ? "Authenticated" : "Not authenticated",
    state ? `State ${state}` : "State unknown",
    data?.host ? `Host ${data.host}` : "Host unknown",
    data?.account ? `Account ${data.account}` : "Account unknown",
  ];
  const guidance =
    state === "provider_unavailable"
      ? "GitHub returned a temporary failure. Refresh this status before signing in again."
      : state === "probe_failed"
        ? "RalphX could not verify the saved CLI credential. Refresh this status before signing in again."
        : ghInstalled
          ? "Run `gh auth login` in your project shell, then refresh this status."
          : "Install the GitHub CLI, run `gh auth login`, then refresh this status.";
  const showCredentialGuidance =
    state === "unauthenticated" ||
    state === "credential_rejected" ||
    state === "cli_unavailable" ||
    state === undefined;
  const showTransientGuidance =
    state === "provider_unavailable" || state === "probe_failed";

  return (
    <SettingsSection>
      <div className="space-y-4">
        <IntegrationStatusBanner
          connected={connected}
          title={isLoading ? "Reading GitHub CLI status" : statusTitle}
          chips={isLoading ? ["Checking gh"] : statusChips}
          lastError={isError ? describeError(error) : null}
        />

        {!connected && !isLoading && (showCredentialGuidance || showTransientGuidance) ? (
          <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-3 text-sm text-[var(--text-secondary)]">
            {showCredentialGuidance ? (
              <code className="rounded bg-[var(--bg-elevated)] px-1.5 py-0.5 text-[var(--text-primary)]">
                gh auth login
              </code>
            ) : null}
            <span className="ml-2">{guidance}</span>
          </div>
        ) : null}

        <div className="flex items-center justify-between gap-3">
          <p className="text-xs text-[var(--text-muted)]">
            RalphX reads the local CLI status and does not store a GitHub token.
            After signing in again, restart terminals that were already open; new
            terminals use the updated GitHub CLI credentials.
          </p>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => void refetch()}
            disabled={isFetching}
            className="shrink-0 gap-2"
          >
            {isFetching ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            ) : (
              <RefreshCw className="h-3.5 w-3.5" aria-hidden="true" />
            )}
            Refresh
          </Button>
        </div>
      </div>
    </SettingsSection>
  );
}
