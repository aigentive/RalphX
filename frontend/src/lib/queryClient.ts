/**
 * TanStack Query client configuration
 *
 * Configures the QueryClient with sensible defaults for the Tauri app.
 */

import { QueryClient } from "@tanstack/react-query";

import { getTransportEnvironmentId } from "@/lib/remote/active-environment";

/**
 * Default stale time for queries (5 minutes)
 * Data is considered fresh for this duration.
 */
const DEFAULT_STALE_TIME = 5 * 60 * 1000;

/**
 * Default retry count for failed queries
 * Backend errors typically don't benefit from retries.
 */
const DEFAULT_RETRY = 1;

/**
 * Create and configure the QueryClient
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // How long data is considered fresh (5 minutes)
        staleTime: DEFAULT_STALE_TIME,

        // Only retry once for transient errors
        retry: DEFAULT_RETRY,

        // Don't refetch on window focus for desktop app
        refetchOnWindowFocus: false,

        // Refetch when reconnecting (for remote backends)
        refetchOnReconnect: true,

        // Keep failed data visible while refetching
        placeholderData: (previousData: unknown) => previousData,
      },
      mutations: {
        // Don't retry mutations by default
        retry: false,
      },
    },
  });
}

/**
 * Environment-keyed QueryClient instances for the app.
 * Created lazily and retained so switching back can reuse a warm cache.
 */
const queryClients = new Map<string, QueryClient>();

export function getQueryClient(
  environmentId: string = getTransportEnvironmentId(),
): QueryClient {
  let queryClient = queryClients.get(environmentId);
  if (!queryClient) {
    queryClient = createQueryClient();
    queryClients.set(environmentId, queryClient);

    // Preserve the original first-creation Playwright exposure. The mounted
    // EnvironmentScopedProviders is the single writer for subsequent switches.
    if (
      queryClients.size === 1 &&
      typeof window !== "undefined" &&
      !window.__TAURI_INTERNALS__
    ) {
      window.__queryClient = queryClient;
    }
  }
  return queryClient;
}

/** Clears and forgets an environment cache after its registry row is removed. */
export function removeQueryClient(environmentId: string): void {
  const queryClient = queryClients.get(environmentId);
  if (queryClient === undefined) {
    return;
  }
  queryClient.clear();
  queryClients.delete(environmentId);
}

/**
 * Reset the query client (for testing)
 */
export function resetQueryClient(): void {
  queryClients.clear();
}
