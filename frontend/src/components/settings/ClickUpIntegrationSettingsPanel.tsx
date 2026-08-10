import { useState } from "react";
import { KeyRound, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { RemoteHostOnlyNotice } from "@/components/remote/RemoteHostOnlyNotice";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { useClickUpIntegration } from "@/hooks/useClickUpIntegration";

import {
  ErrorBanner,
  IntegrationDisconnectButton,
  IntegrationStatusBanner,
  SettingsSection,
} from "./SettingsView.shared";

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  return fallback;
}

export function ClickUpIntegrationSettingsPanel() {
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const {
    settings,
    isLoading,
    isError,
    error,
    connected,
    workspaces,
    isLoadingWorkspaces,
    workspacesError,
    saveSettingsAsync,
    validateAsync,
    disconnectAsync,
    isSavingSettings,
    isValidating,
    isDisconnecting,
    saveSettingsError,
    validateError,
    disconnectError,
  } = useClickUpIntegration();
  const [apiToken, setApiToken] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const displayedError =
    localError ??
    (isError && error instanceof Error ? error.message : null) ??
    (saveSettingsError instanceof Error ? saveSettingsError.message : null) ??
    (validateError instanceof Error ? validateError.message : null) ??
    (disconnectError instanceof Error ? disconnectError.message : null) ??
    (workspacesError instanceof Error ? workspacesError.message : null);
  const hasConnection = Boolean(settings?.hasApiToken || settings?.enabled);
  const statusChips = [
    `API token ${settings?.hasApiToken ? "stored" : "missing"}`,
    `Status ${settings?.validationStatus ?? "not_configured"}`,
    `Search ${settings?.taskSearchAvailable ? "available" : "disabled"}`,
    `Workspace ${settings?.workspaceId ? settings.workspaceId : "none"}`,
  ];

  const saveApiToken = async () => {
    setLocalError(null);
    setSaved(false);
    const trimmed = apiToken.trim();
    if (!trimmed) {
      setLocalError("ClickUp API token cannot be empty");
      return;
    }

    try {
      await saveSettingsAsync({ apiToken: trimmed });
      const validated = await validateAsync();
      if (
        !validated.enabled ||
        validated.validationStatus !== "valid" ||
        !validated.taskSearchAvailable
      ) {
        setLocalError(
          validated.lastError ??
            "ClickUp API token was saved, but task references are still disabled",
        );
        return;
      }
      setApiToken("");
      setSaved(true);
    } catch (err) {
      setLocalError(errorMessage(err, "Failed to save ClickUp API token"));
    }
  };

  const validate = async () => {
    setLocalError(null);
    setSaved(false);
    try {
      const validated = await validateAsync();
      if (
        !validated.enabled ||
        validated.validationStatus !== "valid" ||
        !validated.taskSearchAvailable
      ) {
        setLocalError(
          validated.lastError ?? "Failed to validate ClickUp integration",
        );
        return;
      }
      setSaved(true);
    } catch (err) {
      setLocalError(errorMessage(err, "Failed to validate ClickUp integration"));
    }
  };

  const disconnect = async () => {
    setLocalError(null);
    setSaved(false);
    setApiToken("");
    try {
      await disconnectAsync();
    } catch (err) {
      setLocalError(errorMessage(err, "Failed to disconnect ClickUp integration"));
    }
  };

  const selectWorkspace = async (workspaceId: string) => {
    setLocalError(null);
    setSaved(false);
    try {
      await saveSettingsAsync({ workspaceId });
      setSaved(true);
    } catch (err) {
      setLocalError(errorMessage(err, "Failed to select ClickUp workspace"));
    }
  };

  if (isLoading) {
    return (
      <SettingsSection>
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 text-sm text-[var(--text-muted)]">
          Loading ClickUp settings...
        </div>
      </SettingsSection>
    );
  }

  if (isRemoteEnvironment) {
    return <RemoteHostOnlyNotice subject="ClickUp credentials" />;
  }

  return (
    <SettingsSection>
      {displayedError ? (
        <ErrorBanner
          error={displayedError}
          onDismiss={() => setLocalError(null)}
        />
      ) : null}

      <div className="space-y-4">
        <IntegrationStatusBanner
          connected={connected}
          title={
            connected ? "Task references enabled" : "Task references not ready"
          }
          chips={statusChips}
          lastError={settings?.lastError ?? null}
        />

        <div className="space-y-1.5">
          <Label htmlFor="clickup-api-token">API token</Label>
          <Input
            id="clickup-api-token"
            type="password"
            value={apiToken}
            onChange={(event) => setApiToken(event.target.value)}
            placeholder={
              settings?.hasApiToken
                ? "Stored token unchanged"
                : "Paste ClickUp personal API token"
            }
            disabled={isSavingSettings || isValidating}
          />
          <p className="text-xs leading-relaxed text-[var(--text-muted)]">
            Used for ClickUp ticket search and the unified ticketing dashboard.
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            disabled={isSavingSettings || isValidating}
            onClick={() => void saveApiToken()}
          >
            {isSavingSettings ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <KeyRound className="h-4 w-4" />
            )}
            Save API token
          </Button>
          <Button
            type="button"
            variant="secondary"
            disabled={
              isSavingSettings || isValidating || !settings?.hasApiToken
            }
            onClick={() => void validate()}
          >
            {isValidating ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
            Validate
          </Button>
          {hasConnection ? (
            <IntegrationDisconnectButton
              onDisconnect={disconnect}
              disabled={isSavingSettings || isValidating}
              isDisconnecting={isDisconnecting}
            />
          ) : null}
          {saved ? (
            <span className="text-xs text-[var(--status-success)]">Saved</span>
          ) : null}
        </div>

        {connected ? (
          <div className="space-y-1.5">
            <Label htmlFor="clickup-workspace">Workspace</Label>
            <select
              id="clickup-workspace"
              value={settings?.workspaceId ?? ""}
              disabled={
                isLoadingWorkspaces || isSavingSettings || workspaces.length === 0
              }
              onChange={(event) => void selectWorkspace(event.target.value)}
              className="h-9 w-full rounded-md border border-[var(--border-default)] bg-[var(--bg-surface)] px-3 text-sm text-[var(--text-primary)] outline-none focus:border-[var(--accent-primary)] disabled:opacity-50"
            >
              <option value="" disabled>
                {isLoadingWorkspaces
                  ? "Loading workspaces..."
                  : "Select a workspace"}
              </option>
              {workspaces.map((workspace) => (
                <option key={workspace.id} value={workspace.id}>
                  {workspace.name}
                </option>
              ))}
            </select>
            <p className="text-xs leading-relaxed text-[var(--text-muted)]">
              Tasks load from the Spaces in the selected ClickUp Workspace.
            </p>
          </div>
        ) : null}
      </div>
    </SettingsSection>
  );
}
