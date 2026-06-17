import { useState } from "react";
import { CheckCircle2, Loader2, Webhook, XCircle } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useLinearIntegration } from "@/hooks/useLinearIntegration";

import { ErrorBanner, SectionCard } from "./SettingsView.shared";

const LINEAR_WEBHOOK_PATH = "/api/integrations/linear/webhook";

export function LinearIntegrationSettingsPanel() {
  const {
    webhookConfig,
    isLoading,
    isError,
    error,
    saveWebhookSigningSecretAsync,
    isSavingWebhookSigningSecret,
    saveWebhookSigningSecretError,
  } = useLinearIntegration();
  const [signingSecret, setSigningSecret] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const displayedError =
    localError ??
    (isError && error instanceof Error ? error.message : null) ??
    (saveWebhookSigningSecretError instanceof Error
      ? saveWebhookSigningSecretError.message
      : null);
  const isConfigured = Boolean(webhookConfig?.enabled && webhookConfig.hasSigningSecret);

  const save = async () => {
    setLocalError(null);
    setSaved(false);
    const trimmed = signingSecret.trim();
    if (!trimmed) {
      setLocalError("Linear webhook signing secret cannot be empty");
      return;
    }

    try {
      await saveWebhookSigningSecretAsync({
        signingSecret: trimmed,
        enabled: true,
      });
      setSigningSecret("");
      setSaved(true);
    } catch (err) {
      setLocalError(
        err instanceof Error ? err.message : "Failed to save Linear webhook settings",
      );
    }
  };

  if (isLoading) {
    return (
      <SectionCard
        icon={<Webhook className="h-[18px] w-[18px]" />}
        title="Linear"
        description="Issue status reconciliation from Linear webhooks"
      >
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-2 text-sm text-[var(--text-muted)]">
          Loading Linear settings...
        </div>
      </SectionCard>
    );
  }

  return (
    <SectionCard
      icon={<Webhook className="h-[18px] w-[18px]" />}
      title="Linear"
      description="Issue status reconciliation from Linear webhooks"
    >
      {displayedError ? (
        <ErrorBanner error={displayedError} onDismiss={() => setLocalError(null)} />
      ) : null}

      <div className="space-y-4">
        <div className="rounded-md border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3 py-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <div className="text-sm font-medium text-[var(--text-primary)]">
                {isConfigured ? "Enabled" : "Not configured"}
              </div>
              <div className="mt-1 flex flex-wrap gap-2 text-xs text-[var(--text-muted)]">
                <span>
                  Signing secret {webhookConfig?.hasSigningSecret ? "stored" : "missing"}
                </span>
                <span>Webhook {webhookConfig?.enabled ? "enabled" : "disabled"}</span>
              </div>
            </div>
            {isConfigured ? (
              <CheckCircle2 className="h-5 w-5 text-[var(--status-success)]" />
            ) : (
              <XCircle className="h-5 w-5 text-[var(--text-muted)]" />
            )}
          </div>
        </div>

        <div className="space-y-1.5">
          <Label htmlFor="linear-webhook-url">Webhook path</Label>
          <Input id="linear-webhook-url" value={LINEAR_WEBHOOK_PATH} readOnly />
        </div>

        <div className="space-y-1.5">
          <Label htmlFor="linear-webhook-signing-secret">Signing secret</Label>
          <Input
            id="linear-webhook-signing-secret"
            type="password"
            value={signingSecret}
            onChange={(event) => setSigningSecret(event.target.value)}
            placeholder={
              webhookConfig?.hasSigningSecret ? "Stored secret unchanged" : "Paste signing secret"
            }
            disabled={isSavingWebhookSigningSecret}
          />
          <p className="text-xs leading-relaxed text-[var(--text-muted)]">
            Use the Linear webhook signing secret for this RalphX endpoint.
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            disabled={isSavingWebhookSigningSecret}
            onClick={() => void save()}
          >
            {isSavingWebhookSigningSecret ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : null}
            Save and enable
          </Button>
          {saved ? (
            <span className="text-xs text-[var(--status-success)]">Saved</span>
          ) : null}
        </div>
      </div>
    </SectionCard>
  );
}
