import { GitBranch, Loader2, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useGitHubConnectionStatus } from "@/hooks/useGitHubConnectionStatus";

import {
  IntegrationStatusBanner,
  SectionCard,
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
  const { data, error, isError, isLoading, isFetching, refetch } =
    useGitHubConnectionStatus();
  const ghInstalled = data?.ghInstalled ?? false;
  const authenticated = data?.authenticated ?? false;
  const connected = ghInstalled && authenticated;
  const statusTitle = connected
    ? "GitHub CLI authenticated"
    : ghInstalled
      ? "GitHub CLI not authenticated"
      : "GitHub CLI unavailable";
  const statusChips = [
    ghInstalled ? "gh installed" : "gh missing",
    authenticated ? "Authenticated" : "Not authenticated",
    data?.host ? `Host ${data.host}` : "Host unknown",
    data?.account ? `Account ${data.account}` : "Account unknown",
  ];
  const guidance = ghInstalled
    ? "Run `gh auth login` in your project shell, then refresh this status."
    : "Install the GitHub CLI, run `gh auth login`, then refresh this status.";

  return (
    <SectionCard
      icon={<GitBranch className="h-[18px] w-[18px]" aria-hidden="true" />}
      title="GitHub"
      description="Local GitHub CLI connection"
    >
      <div className="space-y-4">
        <IntegrationStatusBanner
          connected={connected}
          title={isLoading ? "Reading GitHub CLI status" : statusTitle}
          chips={isLoading ? ["Checking gh"] : statusChips}
          lastError={isError ? describeError(error) : null}
        />

        {!connected && !isLoading ? (
          <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-3 text-sm text-[var(--text-secondary)]">
            <code className="rounded bg-[var(--bg-elevated)] px-1.5 py-0.5 text-[var(--text-primary)]">
              gh auth login
            </code>
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
    </SectionCard>
  );
}
