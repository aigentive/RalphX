// GitHub PR-visibility data layer — typed wrappers over the read-only
// `get_github_connection_status` and `get_pull_request_detail` Tauri commands.
//
// The backend DTOs use `#[serde(rename_all = "camelCase")]` (plan Constraint 5),
// so these Zod schemas mirror the emitted camelCase directly — no transform
// layer, matching how `api/ticketing.ts` / `api/clickup.ts` model their
// camelCase provider responses.

import { z } from "zod";

import { typedInvoke } from "@/lib/tauri";

// ============================================================================
// Connection status (get_github_connection_status)
// ============================================================================

/** Read-only reflection of `gh auth status` (Decision 1 — no token storage). */
export const GitHubConnectionStatusSchema = z.object({
  ghInstalled: z.boolean(),
  authenticated: z.boolean(),
  host: z.string().nullable().optional(),
  account: z.string().nullable().optional(),
});
export type GitHubConnectionStatus = z.infer<typeof GitHubConnectionStatusSchema>;

// ============================================================================
// PR-detail read model (get_pull_request_detail) — full graph (Decision 4)
// ============================================================================

/** Distinct, non-throwing view states for the PR-detail surface. */
export const PullRequestDetailStateSchema = z.enum([
  "loaded",
  "noPr",
  "ghUnauthenticated",
  "repoUnresolvable",
  "fetchTimeout",
  "rateLimited",
]);
export type PullRequestDetailState = z.infer<typeof PullRequestDetailStateSchema>;

/** How the PR was associated with the requested branch/number. */
export const PullRequestOriginSchema = z.enum([
  "ownedOutbound",
  "ownedInbound",
  "planBranch",
  "external",
]);
export type PullRequestOrigin = z.infer<typeof PullRequestOriginSchema>;

/** PR description section (title/body/author/state/refs). */
export const PullRequestDescriptionSchema = z.object({
  number: z.number(),
  title: z.string(),
  body: z.string().nullable().optional(),
  author: z.string().nullable().optional(),
  createdAt: z.string().nullable().optional(),
  url: z.string().nullable().optional(),
  state: z.string(),
  isDraft: z.boolean(),
  headRefName: z.string(),
  baseRefName: z.string(),
});
export type PullRequestDescription = z.infer<typeof PullRequestDescriptionSchema>;

/** A single status check / check-run summary. */
export const PullRequestCheckSchema = z.object({
  name: z.string(),
  status: z.string().nullable().optional(),
  conclusion: z.string().nullable().optional(),
  detailsUrl: z.string().nullable().optional(),
});
export type PullRequestCheck = z.infer<typeof PullRequestCheckSchema>;

/** One inline comment inside the latest outstanding requested-changes review. */
export const PullRequestReviewFeedbackCommentSchema = z.object({
  id: z.string(),
  author: z.string(),
  path: z.string().nullable().optional(),
  line: z.number().nullable().optional(),
  body: z.string(),
});
export type PullRequestReviewFeedbackComment = z.infer<
  typeof PullRequestReviewFeedbackCommentSchema
>;

/** Review summary: GitHub decision + the latest CHANGES_REQUESTED review. */
export const PullRequestReviewSummarySchema = z.object({
  reviewDecision: z.string().nullable().optional(),
  latestChangesRequestedAuthor: z.string().nullable().optional(),
  latestChangesRequestedBody: z.string().nullable().optional(),
  latestChangesRequestedSubmittedAt: z.string().nullable().optional(),
  latestChangesRequestedComments: z
    .array(PullRequestReviewFeedbackCommentSchema)
    .default([]),
});
export type PullRequestReviewSummary = z.infer<typeof PullRequestReviewSummarySchema>;

/**
 * An issue comment on the PR. `source` distinguishes the DB-cached evidence
 * path (owned published PRs) from the live `gh` path (inbound/external PRs).
 */
export const PullRequestIssueCommentSchema = z.object({
  id: z.string(),
  author: z.string().nullable().optional(),
  body: z.string(),
  url: z.string().nullable().optional(),
  createdAt: z.string().nullable().optional(),
  updatedAt: z.string().nullable().optional(),
  isBot: z.boolean(),
  isCodecov: z.boolean(),
  source: z.string(),
});
export type PullRequestIssueComment = z.infer<typeof PullRequestIssueCommentSchema>;

/** An inline review-thread comment (live, transient). */
export const PullRequestReviewThreadCommentSchema = z.object({
  id: z.string(),
  author: z.string().nullable().optional(),
  body: z.string(),
  path: z.string().nullable().optional(),
  side: z.string().nullable().optional(),
  line: z.number().nullable().optional(),
  url: z.string().nullable().optional(),
  createdAt: z.string().nullable().optional(),
  inReplyToId: z.string().nullable().optional(),
  isOutdated: z.boolean(),
});
export type PullRequestReviewThreadComment = z.infer<
  typeof PullRequestReviewThreadCommentSchema
>;

/** An attached RalphX conversation (workspace whose branch == the PR head ref). */
export const PullRequestRxConversationSchema = z.object({
  conversationId: z.string(),
  branchName: z.string(),
  linkedIdeationSessionId: z.string().nullable().optional(),
  publicationPrNumber: z.number().nullable().optional(),
  publicationPrStatus: z.string().nullable().optional(),
});
export type PullRequestRxConversation = z.infer<typeof PullRequestRxConversationSchema>;

/** A ticket linked to the PR (forward ticket→PR path ships separately). */
export const PullRequestLinkedTicketSchema = z.object({
  provider: z.string(),
  issueKey: z.string(),
  url: z.string().nullable().optional(),
});
export type PullRequestLinkedTicket = z.infer<typeof PullRequestLinkedTicketSchema>;

/** The full PR-detail graph returned by `get_pull_request_detail`. */
export const PullRequestDetailSchema = z.object({
  state: PullRequestDetailStateSchema,
  origin: PullRequestOriginSchema.nullable().optional(),
  description: PullRequestDescriptionSchema.nullable().optional(),
  checks: z.array(PullRequestCheckSchema).default([]),
  reviewSummary: PullRequestReviewSummarySchema.nullable().optional(),
  issueComments: z.array(PullRequestIssueCommentSchema).default([]),
  reviewThread: z.array(PullRequestReviewThreadCommentSchema).default([]),
  rxConversations: z.array(PullRequestRxConversationSchema).default([]),
  linkedTickets: z.array(PullRequestLinkedTicketSchema).default([]),
  sourcesUnavailable: z.array(z.string()).default([]),
});
export type PullRequestDetail = z.infer<typeof PullRequestDetailSchema>;

// ============================================================================
// Command inputs + API object
// ============================================================================

/** Selector for `get_pull_request_detail` (provide `prNumber` or `branch`). */
export interface GetPullRequestDetailInput {
  projectId: string;
  prNumber?: number | null | undefined;
  branch?: string | null | undefined;
}

export const githubApi = {
  getConnectionStatus(): Promise<GitHubConnectionStatus> {
    return typedInvoke(
      "get_github_connection_status",
      {},
      GitHubConnectionStatusSchema,
    );
  },

  getPullRequestDetail(input: GetPullRequestDetailInput): Promise<PullRequestDetail> {
    return typedInvoke(
      "get_pull_request_detail",
      {
        input: {
          projectId: input.projectId,
          ...(input.prNumber != null && { prNumber: input.prNumber }),
          ...(input.branch != null && input.branch.length > 0 && { branch: input.branch }),
        },
      },
      PullRequestDetailSchema,
    );
  },
} as const;
