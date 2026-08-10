/**
 * The remote environment's answer to "is a provider configured?".
 *
 * Locally the shell reads the full provider settings and negates `requiresOnboarding`. That
 * command is `Denied` on the facade (it probes provider CLIs and carries provider identities,
 * models, and CLI paths), so under a remote environment the shell asks the projection
 * instead — the same boolean, without the surface.
 *
 * Disabled on the local environment by construction: there is no local answer to fetch here,
 * and `useHarnessProviders` remains the local gate's source.
 */

import { useQuery } from "@tanstack/react-query";

import { projectsApi, type RemoteProviderReadiness } from "@/api/projects";

export const remoteProviderReadinessKeys = {
  all: ["remote-provider-readiness"] as const,
};

export function useRemoteProviderReadiness(enabled: boolean) {
  return useQuery<RemoteProviderReadiness, Error>({
    queryKey: remoteProviderReadinessKeys.all,
    queryFn: () => projectsApi.remoteProviderReadiness(),
    enabled,
    staleTime: 1000 * 30,
  });
}
