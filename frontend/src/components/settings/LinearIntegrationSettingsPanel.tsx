import { useState } from "react";
import { KeyRound, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { RemoteHostOnlyNotice } from "@/components/remote/RemoteHostOnlyNotice";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import {
  isLinearConnected,
  useLinearIntegration,
} from "@/hooks/useLinearIntegration";

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

export function LinearIntegrationSettingsPanel() {
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const {
    settings,
    isLoading,
    isError,
    error,
    saveSettingsAsync,
    validateAsync,
    disconnectAsync,
    isSavingSettings,
    isValidating,
    isDisconnecting,
    saveSettingsError,
    validateError,
    disconnectError,
  } = useLinearIntegration();
  const [apiToken, setApiToken] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const displayedError =
    localError ??
    (isError && error instanceof Error ? error.message : null) ??
    (saveSettingsError instanceof Error ? saveSettingsError.message : null) ??
    (validateError instanceof Error ? validateError.message : null) ??
    (disconnectError instanceof Error ? disconnectError.message : null);
  const isApiConfigured = isLinearConnected(settings);
  const hasConnection = Boolean(settings?.hasApiToken || settings?.enabled);
  const statusChips = [
    `API token ${settings?.hasApiToken ? "stored" : "missing"}`,
    `Status ${settings?.validationStatus ?? "not_configured"}`,
    `Search ${settings?.issueSearchAvailable ? "available" : "disabled"}`,
  ];

  const saveApiToken = async () => {
    setLocalError(null);
    setSaved(false);
    const trimmed = apiToken.trim();
    if (!trimmed) {
      setLocalError("Linear API token cannot be empty");
      return;
    }

    try {
      await saveSettingsAsync({ apiToken: trimmed });
      const validated = await validateAsync();
      if (
        !validated.enabled ||
        validated.validationStatus !== "valid" ||
        !validated.issueSearchAvailable
      ) {
        setLocalError(
          validated.lastError ??
            "Linear API token was saved, but issue references are still disabled",
        );
        return;
      }
      setApiToken("");
      setSaved(true);
    } catch (err) {
      setLocalError(errorMessage(err, "Failed to save Linear API token"));
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
        !validated.issueSearchAvailable
      ) {
        setLocalError(
          validated.lastError ?? "Failed to validate Linear integration",
        );
        return;
      }
      setSaved(true);
    } catch (err) {
      setLocalError(errorMessage(err, "Failed to validate Linear integration"));
    }
  };

  const disconnect = async () => {
    setLocalError(null);
    setSaved(false);
    setApiToken("");
    try {
      await disconnectAsync();
    } catch (err) {
      setLocalError(errorMessage(err, "Failed to disconnect Linear integration"));
    }
  };

  if (isLoading) {
    return (
      <SettingsSection>
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 text-sm text-[var(--text-muted)]">
          Loading Linear settings...
        </div>
      </SettingsSection>
    );
  }

  if (isRemoteEnvironment) {
    return <RemoteHostOnlyNotice subject="Linear credentials" />;
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
          connected={isApiConfigured}
          title={
            isApiConfigured
              ? "Issue references enabled"
              : "Issue references not ready"
          }
          chips={statusChips}
          lastError={settings?.lastError ?? null}
        />

        <div className="space-y-1.5">
          <Label htmlFor="linear-api-token">API token</Label>
          <Input
            id="linear-api-token"
            type="password"
            value={apiToken}
            onChange={(event) => setApiToken(event.target.value)}
            placeholder={
              settings?.hasApiToken
                ? "Stored token unchanged"
                : "Paste Linear API token"
            }
            disabled={isSavingSettings || isValidating}
          />
          <p className="text-xs leading-relaxed text-[var(--text-muted)]">
            Used for @linear issue search and prompt context.
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
      </div>
    </SettingsSection>
  );
}
