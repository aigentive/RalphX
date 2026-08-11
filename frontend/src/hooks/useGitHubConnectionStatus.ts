import { useQuery } from "@tanstack/react-query";

import { githubApi, type GitHubConnectionStatus } from "@/api/github";

import { prKeys } from "./usePullRequestDetail";

interface QueryOptions {
  enabled?: boolean | undefined;
}

/**
 * Read-only typed reflection of the local GitHub CLI connection.
 * RalphX stores no GitHub token. Cheap status read with a short
 * staleTime; consumed by GitHub connection and auth repair surfaces. Keyed under
 * the shared `prKeys` GitHub namespace, separate from `ticketingKeys`.
 */
export function useGitHubConnectionStatus(options: QueryOptions = {}) {
  return useQuery<GitHubConnectionStatus>({
    queryKey: prKeys.connectionStatus(),
    queryFn: () => githubApi.getConnectionStatus(),
    enabled: options.enabled ?? true,
    staleTime: 60_000,
  });
}
