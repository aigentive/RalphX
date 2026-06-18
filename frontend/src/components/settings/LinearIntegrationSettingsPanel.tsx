import { useState } from "react";
import { CheckCircle2, KeyRound, Loader2, XCircle } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useLinearIntegration } from "@/hooks/useLinearIntegration";

import { ErrorBanner, SectionCard } from "./SettingsView.shared";

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
  const {
    settings,
    isLoading,
    isError,
    error,
    saveSettingsAsync,
    validateAsync,
    isSavingSettings,
    isValidating,
    saveSettingsError,
    validateError,
  } = useLinearIntegration();
  const [apiToken, setApiToken] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const displayedError =
    localError ??
    (isError && error instanceof Error ? error.message : null) ??
    (saveSettingsError instanceof Error ? saveSettingsError.message : null) ??
    (validateError instanceof Error ? validateError.message : null);
  const isApiConfigured = Boolean(
    settings?.enabled &&
    settings.hasApiToken &&
    settings.validationStatus === "valid" &&
    settings.issueSearchAvailable,
  );

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

  if (isLoading) {
    return (
      <SectionCard
        icon={<KeyRound className="h-[18px] w-[18px]" />}
        title="Linear"
        description="Linear issue references"
      >
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 text-sm text-[var(--text-muted)]">
          Loading Linear settings...
        </div>
      </SectionCard>
    );
  }

  return (
    <SectionCard
      icon={<KeyRound className="h-[18px] w-[18px]" />}
      title="Linear"
      description="Linear issue references"
    >
      {displayedError ? (
        <ErrorBanner
          error={displayedError}
          onDismiss={() => setLocalError(null)}
        />
      ) : null}

      <div className="space-y-4">
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <div className="text-sm font-medium text-[var(--text-primary)]">
                {isApiConfigured
                  ? "Issue references enabled"
                  : "Issue references not ready"}
              </div>
              <div className="mt-1 flex flex-wrap gap-2 text-xs text-[var(--text-muted)]">
                <span>
                  API token {settings?.hasApiToken ? "stored" : "missing"}
                </span>
                <span>
                  Status {settings?.validationStatus ?? "not_configured"}
                </span>
                <span>
                  Search{" "}
                  {settings?.issueSearchAvailable ? "available" : "disabled"}
                </span>
              </div>
              {settings?.lastError ? (
                <div className="mt-1 text-xs text-[var(--status-error)]">
                  {settings.lastError}
                </div>
              ) : null}
            </div>
            {isApiConfigured ? (
              <CheckCircle2 className="h-5 w-5 text-[var(--status-success)]" />
            ) : (
              <XCircle className="h-5 w-5 text-[var(--text-muted)]" />
            )}
          </div>
        </div>

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
          {saved ? (
            <span className="text-xs text-[var(--status-success)]">Saved</span>
          ) : null}
        </div>
      </div>
    </SectionCard>
  );
}
