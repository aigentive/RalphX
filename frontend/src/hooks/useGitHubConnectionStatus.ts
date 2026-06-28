import { useQuery } from "@tanstack/react-query";

import { githubApi, type GitHubConnectionStatus } from "@/api/github";

import { prKeys } from "./usePullRequestDetail";

interface QueryOptions {
  enabled?: boolean | undefined;
}

/**
 * Read-only reflection of the locally-authenticated `gh` CLI (`gh auth status`).
 * RalphX stores no GitHub token (Decision 1). Cheap status read with a short
 * staleTime; consumed by the GitHub connection settings panel (P7). Keyed under
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
