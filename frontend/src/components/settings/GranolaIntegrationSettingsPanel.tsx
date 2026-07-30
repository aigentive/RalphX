import { useState } from "react";
import { ExternalLink, KeyRound, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useGranolaIntegration } from "@/hooks/useGranolaIntegration";

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

export function GranolaIntegrationSettingsPanel() {
  const {
    settings,
    isLoading,
    isError,
    error,
    connected,
    saveSettingsAsync,
    validateAsync,
    disconnectAsync,
    isSavingSettings,
    isValidating,
    isDisconnecting,
    saveSettingsError,
    validateError,
    disconnectError,
  } = useGranolaIntegration();
  const [apiToken, setApiToken] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const displayedError =
    localError ??
    (isError ? errorMessage(error, "Failed to load Granola settings") : null) ??
    (saveSettingsError
      ? errorMessage(saveSettingsError, "Failed to save Granola API token")
      : null) ??
    (validateError
      ? errorMessage(validateError, "Failed to validate Granola integration")
      : null) ??
    (disconnectError
      ? errorMessage(disconnectError, "Failed to disconnect Granola integration")
      : null);
  const hasConnection = Boolean(settings?.hasApiToken || settings?.enabled);
  const statusChips = [
    `API token ${settings?.hasApiToken ? "stored" : "missing"}`,
    `Status ${settings?.validationStatus ?? "not_configured"}`,
  ];

  const saveApiToken = async () => {
    setLocalError(null);
    setSaved(false);
    const trimmed = apiToken.trim();
    if (!trimmed) {
      setLocalError("Granola API token cannot be empty");
      return;
    }

    try {
      await saveSettingsAsync({ apiToken: trimmed });
      const validated = await validateAsync();
      if (!validated.enabled || validated.validationStatus !== "valid") {
        setLocalError(
          validated.lastError ??
            "Granola API token was saved, but note references are still disabled",
        );
        return;
      }
      setApiToken("");
      setSaved(true);
    } catch (err) {
      setLocalError(errorMessage(err, "Failed to save Granola API token"));
    }
  };

  const validate = async () => {
    setLocalError(null);
    setSaved(false);
    try {
      const validated = await validateAsync();
      if (!validated.enabled || validated.validationStatus !== "valid") {
        setLocalError(
          validated.lastError ?? "Failed to validate Granola integration",
        );
        return;
      }
      setSaved(true);
    } catch (err) {
      setLocalError(errorMessage(err, "Failed to validate Granola integration"));
    }
  };

  const disconnect = async () => {
    setLocalError(null);
    setSaved(false);
    setApiToken("");
    try {
      await disconnectAsync();
    } catch (err) {
      setLocalError(errorMessage(err, "Failed to disconnect Granola integration"));
    }
  };

  if (isLoading) {
    return (
      <SettingsSection>
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 text-sm text-[var(--text-muted)]">
          Loading Granola settings...
        </div>
      </SettingsSection>
    );
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
            connected
              ? "Note references enabled"
              : "Note references not ready"
          }
          chips={statusChips}
          lastError={settings?.lastError ?? null}
        />

        <div className="space-y-1.5">
          <Label htmlFor="granola-api-token">API token</Label>
          <Input
            id="granola-api-token"
            type="password"
            value={apiToken}
            onChange={(event) => setApiToken(event.target.value)}
            placeholder={
              settings?.hasApiToken
                ? "Stored token unchanged"
                : "Paste Granola API token"
            }
            disabled={isSavingSettings || isValidating}
          />
          <p className="text-xs leading-relaxed text-[var(--text-muted)]">
            Used for @granola note references and prompt context.
          </p>
        </div>

        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 text-xs leading-relaxed text-[var(--text-secondary)]">
          <div className="font-medium text-[var(--text-primary)]">
            Get a Granola API key
          </div>
          <div className="mt-1">
            {
              "Open the Granola desktop app, go to Settings -> Connectors -> API keys, create a new key, choose the note scopes, then paste it here."
            }
          </div>
          <a
            href="https://docs.granola.ai/introduction"
            target="_blank"
            rel="noreferrer"
            className="mt-1 inline-flex items-center gap-1 text-[var(--accent-primary)] hover:underline"
          >
            Granola API docs
            <ExternalLink className="h-3 w-3" aria-hidden="true" />
          </a>
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
