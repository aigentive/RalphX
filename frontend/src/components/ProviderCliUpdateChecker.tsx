import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Download, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import {
  providerCliManagementApi,
  type ManagedProviderCliStatusResponse,
} from "@/api/provider-cli-management";
import { harnessProviderKeys } from "@/hooks/useHarnessProviders";
import { providerCliManagementKeys } from "@/hooks/useProviderCliManagement";
import { useUiStore } from "@/stores/uiStore";

const STARTUP_PROVIDER_CLI_CHECK_DELAY_MS = 7_000;
const PROVIDER_LABELS: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
};

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

function providerCliToastId(provider: string) {
  return `provider-cli-update:${provider}`;
}

export function ProviderCliUpdateChecker() {
  const queryClient = useQueryClient();
  const openModal = useUiStore((state) => state.openModal);
  const notifiedKeys = useRef(new Set<string>());

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
      const toastId = providerCliToastId(status.provider);
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
      const versionKey = status.latestVersion ?? status.action;
      const notificationKey = `${status.provider}:${versionKey}`;
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
            toast.dismiss(providerCliToastId(status.provider));
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
              onClick={() => toast.dismiss(providerCliToastId(status.provider))}
            >
              Dismiss
            </button>
          </div>
        </div>,
        {
          duration: Infinity,
          id: providerCliToastId(status.provider),
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

  return null;
}
