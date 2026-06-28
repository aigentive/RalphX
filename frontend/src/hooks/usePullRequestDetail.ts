import { useQuery, type QueryClient } from "@tanstack/react-query";

import {
  githubApi,
  type PullRequestDetail,
  type PullRequestIssueComment,
} from "@/api/github";

interface QueryOptions {
  enabled?: boolean | undefined;
}

/**
 * "Like ticketing" React Query staleTime for the full PR-detail payload
 * (description + comments + review thread + RX conversations). Mirrors
 * `useTicketDetail`'s cached-detail behavior so re-opening the same PR within a
 * session is instant. Comments are part of the cached payload (Decision 6).
 */
export const PR_DETAIL_CACHE_MS = 5 * 60 * 1000;

/** Selector for a PR-detail query — provide `prNumber` or `branch`. */
export interface PullRequestDetailSelector {
  projectId: string;
  prNumber?: number | null | undefined;
  branch?: string | null | undefined;
}

/**
 * Query-key factory for the GitHub PR-visibility data layer. Intentionally a
 * SEPARATE root (`["github-pr"]`) from `ticketingKeys` (`["ticketing"]`) so the
 * ticketing store is never overloaded with PR state (Proof Obligation / plan
 * acceptance: `prKeys` namespace separate from `ticketingKeys`).
 */
export const prKeys = {
  all: ["github-pr"] as const,
  connectionStatus: () => [...prKeys.all, "connection-status"] as const,
  details: () => [...prKeys.all, "detail"] as const,
  detail: (selector: PullRequestDetailSelector) =>
    [
      ...prKeys.details(),
      selector.projectId,
      selector.prNumber ?? null,
      selector.branch ?? null,
    ] as const,
};

function hasResolvableSelector(
  selector: PullRequestDetailSelector | null,
): selector is PullRequestDetailSelector {
  return Boolean(
    selector
      && selector.projectId
      && (selector.prNumber != null
        || (selector.branch != null && selector.branch.length > 0)),
  );
}

/**
 * Cache the full PR-detail graph for a `(project, pr|branch)` selector.
 *
 * The query is deferred/enabled-gated (Constraint 6): it is not fired until the
 * caller passes `enabled` (e.g. after the PR-detail shell paints) AND the
 * selector resolves to a project + PR number/branch. The payload — including
 * comments — is cached with a `staleTime` so re-opens are instant.
 */
export function usePullRequestDetail(
  selector: PullRequestDetailSelector | null,
  options: QueryOptions = {},
) {
  return useQuery<PullRequestDetail>({
    queryKey: selector ? prKeys.detail(selector) : [...prKeys.details(), null],
    queryFn: () => {
      if (!selector) {
        throw new Error("Pull request selector is required");
      }
      return githubApi.getPullRequestDetail(selector);
    },
    enabled: (options.enabled ?? true) && hasResolvableSelector(selector),
    staleTime: PR_DETAIL_CACHE_MS,
    gcTime: PR_DETAIL_CACHE_MS,
    refetchOnMount: false,
    refetchOnWindowFocus: false,
  });
}

// ============================================================================
// Optimistic issue-comment dedupe (mirrors the `useTicketing` optimistic append
// + dedupe). PR detail is read-focused in v1, but a user-posted comment can be
// appended optimistically; dedupe-by-id keeps the cached `issueComments`
// consistent when the server payload later refreshes with the same comment.
// ============================================================================

export interface OptimisticPrCommentInput {
  clientId: string;
  body: string;
  author?: string | null | undefined;
}

/** Build a synthetic optimistic issue comment keyed by `optimistic:<clientId>`. */
export function buildOptimisticPrComment(
  input: OptimisticPrCommentInput,
): PullRequestIssueComment {
  const now = new Date().toISOString();
  return {
    id: `optimistic:${input.clientId}`,
    author: input.author ?? "You",
    body: input.body,
    url: null,
    createdAt: now,
    updatedAt: now,
    isBot: false,
    isCodecov: false,
    source: "live",
  };
}

/** Dedupe issue comments by id (last write wins, stable first-appearance order). */
export function dedupePrIssueComments(
  comments: PullRequestIssueComment[],
): PullRequestIssueComment[] {
  const byId = new Map<string, PullRequestIssueComment>();
  for (const comment of comments) {
    byId.set(comment.id, comment);
  }
  return [...byId.values()];
}

/**
 * Append an optimistic comment into the cached PR detail, idempotent by id so
 * repeated appends (or a later server refresh carrying the same id) never
 * duplicate the comment.
 */
export function appendOptimisticPrComment(
  queryClient: QueryClient,
  selector: PullRequestDetailSelector,
  comment: PullRequestIssueComment,
): void {
  queryClient.setQueryData<PullRequestDetail>(prKeys.detail(selector), (detail) => {
    if (!detail) {
      return detail;
    }
    return {
      ...detail,
      issueComments: dedupePrIssueComments([...detail.issueComments, comment]),
    };
  });
}
