import type { TicketingProviderSummary } from "@/api/ticketing";

export function isValidTicketingProvider(
  provider: TicketingProviderSummary | null | undefined,
): provider is TicketingProviderSummary {
  return Boolean(provider?.enabled && provider.connectionStatus === "connected");
}

export function getValidTicketingProviders(
  providers: TicketingProviderSummary[] | null | undefined,
): TicketingProviderSummary[] {
  return (providers ?? []).filter(isValidTicketingProvider);
}

export function hasValidTicketingProvider(
  providers: TicketingProviderSummary[] | null | undefined,
): boolean {
  return getValidTicketingProviders(providers).length > 0;
}
