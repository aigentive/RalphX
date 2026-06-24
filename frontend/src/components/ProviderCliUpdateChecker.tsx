import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Download, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import {
  providerCliManagementApi,
  type ManagedProviderCliStatusResponse,
} from "@/api/provider-cli-management";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { harnessProviderKeys } from "@/hooks/useHarnessProviders";
import { providerCliManagementKeys } from "@/hooks/useProviderCliManagement";
import {
  providerCliUpdateNotificationKey,
  providerCliUpdateToastId,
} from "@/lib/provider-cli-update-toast";
import { useUiStore } from "@/stores/uiStore";

const STARTUP_PROVIDER_CLI_CHECK_DELAY_MS = 7_000;
export const PROVIDER_CLI_DISMISSED_UPDATES_STORAGE_KEY =
  "ralphx-provider-cli-dismissed-updates";

const PROVIDER_LABELS: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
};

interface PendingProviderCliDismiss {
  notificationKey: string;
  status: ManagedProviderCliStatusResponse;
}

function providerLabel(provider: string): string {
  return PROVIDER_LABELS[provider] ?? provider;
}

function isActionableManagedStatus(status: ManagedProviderCliStatusResponse) {
  return (
    status.cliManagementMode === "rx_managed" &&
    status.supported &&
    (status.action === "install" || status.action === "update")
  );
}

function isUserManagedOutdatedStatus(status: ManagedProviderCliStatusResponse) {
  return (
    status.cliManagementMode === "user_managed" &&
    status.supported &&
    status.updateAvailable
  );
}

function readDismissedProviderCliUpdateKeys(): Set<string> {
  try {
    const raw = globalThis.localStorage?.getItem(
      PROVIDER_CLI_DISMISSED_UPDATES_STORAGE_KEY,
    );
    if (!raw) {
      return new Set();
    }
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return new Set();
    }
    return new Set(parsed.filter((item): item is string => typeof item === "string"));
  } catch {
    return new Set();
  }
}

function isDismissedProviderCliUpdate(notificationKey: string): boolean {
  return readDismissedProviderCliUpdateKeys().has(notificationKey);
}

function rememberDismissedProviderCliUpdate(notificationKey: string): void {
  try {
    const keys = readDismissedProviderCliUpdateKeys();
    keys.add(notificationKey);
    globalThis.localStorage?.setItem(
      PROVIDER_CLI_DISMISSED_UPDATES_STORAGE_KEY,
      JSON.stringify([...keys].sort()),
    );
  } catch {
    // A blocked preference store should not prevent dismissing the current toast.
  }
}

export function ProviderCliUpdateChecker() {
  const queryClient = useQueryClient();
  const openModal = useUiStore((state) => state.openModal);
  const notifiedKeys = useRef(new Set<string>());
  const [pendingDismiss, setPendingDismiss] =
    useState<PendingProviderCliDismiss | null>(null);

  const handleRemindAgain = useCallback(() => {
    if (!pendingDismiss) return;
    toast.dismiss(providerCliUpdateToastId(pendingDismiss.status.provider));
    setPendingDismiss(null);
  }, [pendingDismiss]);

  const handleDontAskAgain = useCallback(() => {
    if (!pendingDismiss) return;
    rememberDismissedProviderCliUpdate(pendingDismiss.notificationKey);
    toast.dismiss(providerCliUpdateToastId(pendingDismiss.status.provider));
    setPendingDismiss(null);
  }, [pendingDismiss]);

  useEffect(() => {
    let cancelled = false;

    const invalidateProviderCliQueries = async () => {
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: providerCliManagementKeys.all,
        }),
        queryClient.invalidateQueries({ queryKey: harnessProviderKeys.all }),
        queryClient.invalidateQueries({ queryKey: ["agent", "harness"] }),
      ]);
    };

    const installOrUpdate = async (status: ManagedProviderCliStatusResponse) => {
      const label = providerLabel(status.provider);
      const toastId = providerCliUpdateToastId(status.provider);
      const verb = status.action === "install" ? "Installing" : "Updating";
      toast.loading(`${verb} ${label} CLI...`, { id: toastId });
      try {
        await providerCliManagementApi.installOrUpdate({
          provider: status.provider,
        });
        toast.success(`${label} CLI is ready.`, { id: toastId });
        await invalidateProviderCliQueries();
      } catch (error) {
        toast.error(`Failed to update ${label} CLI.`, {
          id: toastId,
          description: error instanceof Error ? error.message : undefined,
        });
      }
    };

    const showManualToast = (status: ManagedProviderCliStatusResponse) => {
      const notificationKey = providerCliUpdateNotificationKey(status);
      if (status.updateAvailable && isDismissedProviderCliUpdate(notificationKey)) {
        return;
      }
      if (notifiedKeys.current.has(notificationKey)) {
        return;
      }
      notifiedKeys.current.add(notificationKey);

      const label = providerLabel(status.provider);
      const isInstall = status.action === "install";
      const isUserManaged = status.cliManagementMode === "user_managed";
      const primaryLabel = isUserManaged ? "Open Settings" : isInstall ? "Install" : "Update";
      const onPrimaryAction = isUserManaged
        ? () => {
            openModal("settings", { section: "providers" });
            toast.dismiss(providerCliUpdateToastId(status.provider));
          }
        : () => void installOrUpdate(status);
      toast(
        <div className="flex flex-col gap-2" data-testid="provider-cli-update-toast">
          <div className="flex items-center gap-2">
            {isInstall ? (
              <Download
                className="h-4 w-4"
                style={{ color: "var(--accent-primary)" }}
              />
            ) : (
              <RefreshCw
                className="h-4 w-4"
                style={{ color: "var(--accent-primary)" }}
              />
            )}
            <span className="font-medium">
              {isInstall ? `${label} CLI ready to install` : `${label} CLI update available`}
            </span>
          </div>
          <p
            className="text-xs"
            style={{ color: "var(--text-muted)", lineHeight: 1.4 }}
          >
            {status.status}
          </p>
          <div className="mt-1 flex gap-2">
            <button
              type="button"
              data-testid={
                isUserManaged
                  ? "provider-cli-open-settings-button"
                  : "provider-cli-update-now-button"
              }
              className="git-auth-startup-toast-action inline-flex h-7 items-center rounded-[6px] px-3 text-xs font-semibold"
              onClick={onPrimaryAction}
            >
              {primaryLabel}
            </button>
            <button
              type="button"
              data-testid="provider-cli-update-dismiss-button"
              className="inline-flex h-7 items-center rounded-[6px] px-3 text-xs font-medium"
              style={{ color: "var(--text-muted)" }}
              onClick={() => {
                if (status.updateAvailable) {
                  setPendingDismiss({ notificationKey, status });
                  return;
                }
                toast.dismiss(providerCliUpdateToastId(status.provider));
              }}
            >
              Dismiss
            </button>
          </div>
        </div>,
        {
          duration: Infinity,
          id: providerCliUpdateToastId(status.provider),
          className: "git-auth-startup-toast",
        },
      );
    };

    const checkProviderClis = async () => {
      try {
        const statuses = await providerCliManagementApi.status();
        if (cancelled) return;

        const autoUpdateCandidates = statuses.providers.filter(
          (status) => isActionableManagedStatus(status) && status.autoUpdateEnabled,
        );
        if (autoUpdateCandidates.length > 0) {
          const toastId = "provider-cli-auto-update";
          toast.loading("Updating managed CLI tools...", { id: toastId });
          try {
            const result = await providerCliManagementApi.autoUpdate();
            if (cancelled) return;
            if (result.updated.length > 0) {
              toast.success("Managed CLI tools are up to date.", { id: toastId });
              await invalidateProviderCliQueries();
            } else {
              toast.dismiss(toastId);
            }
          } catch (error) {
            if (cancelled) return;
            toast.error("Failed to update managed CLI tools.", {
              id: toastId,
              description: error instanceof Error ? error.message : undefined,
            });
          }
        }

        statuses.providers
          .filter(
            (status) =>
              (isActionableManagedStatus(status) && !status.autoUpdateEnabled) ||
              isUserManagedOutdatedStatus(status),
          )
          .forEach(showManualToast);
      } catch (error) {
        console.debug("Provider CLI update check failed:", error);
      }
    };

    const timeoutId = window.setTimeout(
      () => void checkProviderClis(),
      STARTUP_PROVIDER_CLI_CHECK_DELAY_MS,
    );
    return () => {
      cancelled = true;
      window.clearTimeout(timeoutId);
    };
  }, [openModal, queryClient]);

  return (
    <ProviderCliDismissPreferenceDialog
      pendingDismiss={pendingDismiss}
      onOpenChange={(open) => {
        if (!open) {
          setPendingDismiss(null);
        }
      }}
      onRemindAgain={handleRemindAgain}
      onDontAskAgain={handleDontAskAgain}
    />
  );
}

function ProviderCliDismissPreferenceDialog({
  pendingDismiss,
  onDontAskAgain,
  onOpenChange,
  onRemindAgain,
}: {
  pendingDismiss: PendingProviderCliDismiss | null;
  onDontAskAgain: () => void;
  onOpenChange: (open: boolean) => void;
  onRemindAgain: () => void;
}) {
  const status = pendingDismiss?.status ?? null;
  const label = status ? providerLabel(status.provider) : "Provider";
  const latestVersion = status?.latestVersion;

  return (
    <Dialog open={pendingDismiss !== null} onOpenChange={onOpenChange}>
      <DialogContent
        className="w-[min(440px,calc(100vw-2rem))] overflow-hidden p-0"
        data-testid="provider-cli-dismiss-preference-dialog"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-subtle)",
          borderStyle: "solid",
          borderWidth: "1px",
        }}
      >
        <DialogHeader className="block border-b-0 px-5 pb-3 pt-5">
          <DialogTitle className="pr-8 text-base leading-6 tracking-normal">
            {`Dismiss ${label} CLI update?`}
          </DialogTitle>
          <DialogDescription className="mt-1.5 text-sm leading-5 text-[var(--text-secondary)]">
            {latestVersion
              ? `Remind me again later, or stop showing this ${label} CLI ${latestVersion} update.`
              : `Remind me again later, or stop showing this ${label} CLI update.`}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter className="gap-2 border-t-0 px-5 pb-5 pt-0 sm:gap-2">
          <Button type="button" variant="ghost" onClick={onRemindAgain}>
            Remind me again
          </Button>
          <Button type="button" onClick={onDontAskAgain}>
            Don't ask again
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
