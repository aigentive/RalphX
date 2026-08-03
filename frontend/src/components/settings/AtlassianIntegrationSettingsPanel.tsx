import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { RemoteHostOnlyNotice } from "@/components/remote/RemoteHostOnlyNotice";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import {
  isAtlassianConnected,
  useAtlassianIntegration,
} from "@/hooks/useAtlassianIntegration";

import {
  ErrorBanner,
  IntegrationDisconnectButton,
  IntegrationStatusBanner,
  SettingsSection,
} from "./SettingsView.shared";

const EMPTY_FORM = {
  authMethod: "api_token" as "api_token" | "oauth",
  siteUrl: "",
  email: "",
  apiToken: "",
  oauthClientId: "",
  oauthClientSecret: "",
  oauthRedirectUri: "http://127.0.0.1:8765/atlassian/oauth/callback",
  authorizationCode: "",
};

function statusLabel(status: string, enabled: boolean): string {
  if (enabled && status === "valid") return "Enabled";
  if (status === "valid") return "Validated";
  if (status === "invalid") return "Invalid";
  if (status === "pending") return "Validation required";
  return "Not configured";
}

export function AtlassianIntegrationSettingsPanel() {
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const {
    settings,
    isLoading,
    isError,
    error,
    saveAsync,
    validateAsync,
    disconnectAsync,
    buildOAuthAuthorizationAsync,
    startOAuthLocalCallbackAsync,
    completeOAuthLocalCallbackAsync,
    exchangeOAuthCodeAsync,
    isSaving,
    isValidating,
    isDisconnecting,
    isBuildingOAuthAuthorization,
    isStartingOAuthLocalCallback,
    isCompletingOAuthLocalCallback,
    isExchangingOAuthCode,
    saveError,
    validateError,
    disconnectError,
    buildOAuthAuthorizationError,
    startOAuthLocalCallbackError,
    completeOAuthLocalCallbackError,
    exchangeOAuthCodeError,
  } = useAtlassianIntegration();
  const { data: featureFlags } = useFeatureFlags();
  const oauthEnabled = featureFlags.atlassianOauth;
  const [form, setForm] = useState(EMPTY_FORM);
  const [localError, setLocalError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [oauthState, setOauthState] = useState<string | null>(null);

  useEffect(() => {
    if (!settings) return;
    setForm({
      authMethod: oauthEnabled ? settings.authMethod : "api_token",
      siteUrl: settings.siteUrl ?? "",
      email: settings.email ?? "",
      apiToken: "",
      oauthClientId: settings.oauthClientId ?? "",
      oauthClientSecret: "",
      oauthRedirectUri:
        settings.oauthRedirectUri ?? "http://127.0.0.1:8765/atlassian/oauth/callback",
      authorizationCode: "",
    });
  }, [oauthEnabled, settings]);

  const displayedError =
    localError ??
    (isError && error instanceof Error ? error.message : null) ??
    (saveError instanceof Error ? saveError.message : null) ??
    (validateError instanceof Error ? validateError.message : null) ??
    (buildOAuthAuthorizationError instanceof Error
      ? buildOAuthAuthorizationError.message
      : null) ??
    (startOAuthLocalCallbackError instanceof Error
      ? startOAuthLocalCallbackError.message
      : null) ??
    (completeOAuthLocalCallbackError instanceof Error
      ? completeOAuthLocalCallbackError.message
      : null) ??
    (exchangeOAuthCodeError instanceof Error ? exchangeOAuthCodeError.message : null) ??
    (disconnectError instanceof Error ? disconnectError.message : null);
  const busy =
    isSaving ||
    isValidating ||
    isDisconnecting ||
    isBuildingOAuthAuthorization ||
    isStartingOAuthLocalCallback ||
    isCompletingOAuthLocalCallback ||
    isExchangingOAuthCode;
  const usingOAuth = form.authMethod === "oauth";
  const connected = isAtlassianConnected(settings);
  const hasConnection = Boolean(
    settings?.enabled ||
      settings?.hasApiToken ||
      settings?.hasOauthToken ||
      settings?.hasOauthClientSecret,
  );
  const statusChips = [
    oauthEnabled && settings?.authMethod === "oauth" ? "OAuth 2.0" : "API token",
    `Jira ${settings?.jiraAvailable ? "ready" : "not validated"}`,
    `Confluence ${settings?.confluenceAvailable ? "ready" : "not validated"}`,
    ...(settings?.hasApiToken ? ["API token stored"] : []),
    ...(oauthEnabled && settings?.hasOauthToken ? ["OAuth token stored"] : []),
  ];
  const canValidate = usingOAuth
    ? Boolean(settings?.hasOauthToken)
    : Boolean(form.siteUrl.trim()) &&
      Boolean(form.email.trim()) &&
      (Boolean(form.apiToken.trim()) || Boolean(settings?.hasApiToken));
  const canStartOAuth =
    Boolean(form.siteUrl.trim()) &&
    Boolean(form.oauthClientId.trim()) &&
    Boolean(form.oauthRedirectUri.trim()) &&
    (Boolean(form.oauthClientSecret.trim()) || Boolean(settings?.hasOauthClientSecret));

  const save = async (validateAfterSave: boolean) => {
    setLocalError(null);
    setSaved(false);
    try {
      await saveAsync({
        authMethod: form.authMethod,
        siteUrl: form.siteUrl,
        email: form.email,
        ...(form.apiToken.trim() ? { apiToken: form.apiToken } : {}),
        ...(usingOAuth && oauthEnabled
          ? {
              oauthClientId: form.oauthClientId,
              oauthRedirectUri: form.oauthRedirectUri,
              ...(form.oauthClientSecret.trim()
                ? { oauthClientSecret: form.oauthClientSecret }
                : {}),
            }
          : {}),
      });
      setForm((current) => ({ ...current, apiToken: "", oauthClientSecret: "" }));
      if (validateAfterSave) {
        await validateAsync();
      }
      setSaved(true);
    } catch (err) {
      setLocalError(err instanceof Error ? err.message : "Failed to save Atlassian settings");
    }
  };

  const disconnect = async () => {
    setLocalError(null);
    setSaved(false);
    setOauthState(null);
    try {
      await disconnectAsync();
      setForm({ ...EMPTY_FORM });
    } catch (err) {
      setLocalError(
        err instanceof Error ? err.message : "Failed to disconnect Atlassian",
      );
    }
  };

  const startOAuth = async () => {
    if (!oauthEnabled) return;
    setLocalError(null);
    setSaved(false);
    try {
      await saveAsync({
        authMethod: "oauth",
        siteUrl: form.siteUrl,
        oauthClientId: form.oauthClientId,
        oauthRedirectUri: form.oauthRedirectUri,
        ...(form.oauthClientSecret.trim()
          ? { oauthClientSecret: form.oauthClientSecret }
          : {}),
      });
      const authorization = await startOAuthLocalCallbackAsync();
      setOauthState(authorization.state);
      await openUrl(authorization.authorizationUrl);
      setForm((current) => ({
        ...current,
        oauthClientSecret: "",
        oauthRedirectUri: authorization.redirectUri,
      }));
      await completeOAuthLocalCallbackAsync({ state: authorization.state });
      setOauthState(null);
      setSaved(true);
    } catch (err) {
      setOauthState(null);
      setLocalError(err instanceof Error ? err.message : "Failed to start Atlassian OAuth");
    }
  };

  const openOAuthAuthorizationOnly = async () => {
    if (!oauthEnabled) return;
    setLocalError(null);
    try {
      await saveAsync({
        authMethod: "oauth",
        siteUrl: form.siteUrl,
        oauthClientId: form.oauthClientId,
        oauthRedirectUri: form.oauthRedirectUri,
        ...(form.oauthClientSecret.trim()
          ? { oauthClientSecret: form.oauthClientSecret }
          : {}),
      });
      const authorization = await buildOAuthAuthorizationAsync();
      setOauthState(authorization.state);
      setForm((current) => ({
        ...current,
        oauthClientSecret: "",
        oauthRedirectUri: authorization.redirectUri,
      }));
      await openUrl(authorization.authorizationUrl);
    } catch (err) {
      setOauthState(null);
      setLocalError(
        err instanceof Error ? err.message : "Failed to open Atlassian OAuth URL",
      );
    }
  };

  const exchangeOAuthCode = async () => {
    if (!oauthEnabled) return;
    setLocalError(null);
    setSaved(false);
    try {
      await exchangeOAuthCodeAsync({ authorizationCode: form.authorizationCode });
      setForm((current) => ({ ...current, authorizationCode: "" }));
      setSaved(true);
    } catch (err) {
      setLocalError(err instanceof Error ? err.message : "Failed to connect Atlassian OAuth");
    }
  };

  if (isLoading) {
    return (
      <SettingsSection>
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 text-sm text-[var(--text-muted)]">
          Loading Atlassian settings...
        </div>
      </SettingsSection>
    );
  }

  if (isRemoteEnvironment) {
    return <RemoteHostOnlyNotice subject="Atlassian credentials" />;
  }

  return (
    <SettingsSection>
      {displayedError ? (
        <ErrorBanner error={displayedError} onDismiss={() => setLocalError(null)} />
      ) : null}

      <div className="space-y-4">
        <IntegrationStatusBanner
          connected={connected}
          title={statusLabel(
            settings?.validationStatus ?? "not_configured",
            Boolean(settings?.enabled),
          )}
          chips={statusChips}
          lastError={settings?.lastError ?? null}
        />

        {oauthEnabled ? (
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant={form.authMethod === "api_token" ? "default" : "outline"}
              disabled={busy}
              onClick={() =>
                setForm((current) => ({ ...current, authMethod: "api_token" }))
              }
            >
              API token
            </Button>
            <Button
              type="button"
              variant={form.authMethod === "oauth" ? "default" : "outline"}
              disabled={busy}
              onClick={() =>
                setForm((current) => ({ ...current, authMethod: "oauth" }))
              }
            >
              OAuth 2.0
            </Button>
          </div>
        ) : null}

        <div className="grid gap-3 md:grid-cols-2">
          <div className="space-y-1.5">
            <Label htmlFor="atlassian-site-url">Site URL</Label>
            <Input
              id="atlassian-site-url"
              value={form.siteUrl}
              onChange={(event) =>
                setForm((current) => ({ ...current, siteUrl: event.target.value }))
              }
              placeholder="https://example.atlassian.net"
              disabled={busy}
            />
          </div>
          {!usingOAuth ? (
            <div className="space-y-1.5">
              <Label htmlFor="atlassian-email">Account email</Label>
              <Input
                id="atlassian-email"
                type="email"
                value={form.email}
                onChange={(event) =>
                  setForm((current) => ({ ...current, email: event.target.value }))
                }
                placeholder="you@example.com"
                disabled={busy}
              />
            </div>
          ) : null}
        </div>

        {!usingOAuth ? (
          <div className="space-y-1.5">
            <Label htmlFor="atlassian-api-token">API token</Label>
            <Input
              id="atlassian-api-token"
              type="password"
              value={form.apiToken}
              onChange={(event) =>
                setForm((current) => ({ ...current, apiToken: event.target.value }))
              }
              placeholder={settings?.hasApiToken ? "Stored token unchanged" : "Paste API token"}
              disabled={busy}
            />
            <p className="text-xs leading-relaxed text-[var(--text-muted)]">
              Create an Atlassian API token from your Atlassian account, then use it
              with your site URL and account email.
            </p>
            <a
              href="https://id.atlassian.com/manage-profile/security/api-tokens"
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1 text-xs font-medium text-[var(--accent-primary)] hover:underline"
            >
              Open Atlassian API tokens
              <ExternalLink className="h-3 w-3" aria-hidden="true" />
            </a>
          </div>
        ) : (
          <div className="space-y-3 rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-3">
            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1.5">
                <Label htmlFor="atlassian-oauth-client-id">OAuth client ID</Label>
                <Input
                  id="atlassian-oauth-client-id"
                  value={form.oauthClientId}
                  onChange={(event) =>
                    setForm((current) => ({ ...current, oauthClientId: event.target.value }))
                  }
                  disabled={busy}
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="atlassian-oauth-client-secret">OAuth client secret</Label>
                <Input
                  id="atlassian-oauth-client-secret"
                  type="password"
                  value={form.oauthClientSecret}
                  onChange={(event) =>
                    setForm((current) => ({
                      ...current,
                      oauthClientSecret: event.target.value,
                    }))
                  }
                  placeholder={
                    settings?.hasOauthClientSecret ? "Stored secret unchanged" : "Paste secret"
                  }
                  disabled={busy}
                />
              </div>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="atlassian-oauth-redirect-uri">Redirect URI</Label>
              <Input
                id="atlassian-oauth-redirect-uri"
                value={form.oauthRedirectUri}
                onChange={(event) =>
                  setForm((current) => ({ ...current, oauthRedirectUri: event.target.value }))
                }
                disabled={busy}
              />
            </div>
            <p className="text-xs leading-relaxed text-[var(--text-muted)]">
              Create an OAuth 2.0 (3LO) app in the Atlassian Developer Console,
              add Jira and Confluence permissions, and configure this exact local redirect URI.
              RalphX listens on that address during consent and completes the connection
              automatically.
            </p>
            <div className="flex flex-wrap items-center gap-2">
              <Button
                type="button"
                disabled={busy || !canStartOAuth}
                onClick={() => void startOAuth()}
              >
                {isStartingOAuthLocalCallback || isCompletingOAuthLocalCallback ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <ExternalLink className="h-4 w-4" />
                )}
                Open consent and connect
              </Button>
              <Button
                type="button"
                variant="outline"
                disabled={busy || !canStartOAuth}
                onClick={() => void openOAuthAuthorizationOnly()}
              >
                {isBuildingOAuthAuthorization ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <ExternalLink className="h-4 w-4" />
                )}
                Open URL only
              </Button>
              {oauthState ? (
                <span className="text-xs text-[var(--text-muted)]">State {oauthState}</span>
              ) : null}
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="atlassian-oauth-code">Authorization code</Label>
              <Input
                id="atlassian-oauth-code"
                value={form.authorizationCode}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    authorizationCode: event.target.value,
                  }))
                }
                disabled={busy}
              />
            </div>
            <p className="text-xs leading-relaxed text-[var(--text-muted)]">
              Manual exchange is only needed if automatic return fails and you copied
              the code from the browser address bar.
            </p>
            <Button
              type="button"
              disabled={busy || !form.authorizationCode.trim()}
              onClick={() => void exchangeOAuthCode()}
            >
              {isExchangingOAuthCode ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
              Exchange code and validate
            </Button>
          </div>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <Button type="button" variant="outline" disabled={busy} onClick={() => void save(false)}>
            {isSaving && !isValidating ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
            Save
          </Button>
          <Button
            type="button"
            disabled={busy || !canValidate}
            onClick={() => void save(true)}
          >
            {isValidating ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
            Save and validate
          </Button>
          {hasConnection ? (
            <IntegrationDisconnectButton
              onDisconnect={disconnect}
              disabled={busy}
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
