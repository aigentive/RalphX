import type { ManagedProviderCliStatusResponse } from "@/api/provider-cli-management";

export function providerCliUpdateToastId(provider: string): string {
  return `provider-cli-update:${provider}`;
}

export function providerCliUpdateNotificationKey(
  status: Pick<
    ManagedProviderCliStatusResponse,
    "action" | "latestVersion" | "provider"
  >,
): string {
  return `${status.provider}:${status.latestVersion ?? status.action}`;
}

export function providerCliUpdateToastMatchesInstalledStatus(
  notificationStatus: ManagedProviderCliStatusResponse | undefined,
  installedStatus: ManagedProviderCliStatusResponse,
): boolean {
  const advertisedVersion = notificationStatus?.latestVersion?.trim();
  const installedVersion = installedStatus.currentVersion?.trim();
  return Boolean(
    notificationStatus?.updateAvailable &&
      advertisedVersion &&
      installedVersion &&
      installedVersion === advertisedVersion,
  );
}
