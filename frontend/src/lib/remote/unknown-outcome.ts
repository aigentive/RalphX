/**
 * P-20 client half: reconciling an UNKNOWN mutation outcome (PR 2.7-b).
 *
 * `REMOTE_TIMEOUT_UNKNOWN` and `REMOTE_REQUEST_IN_PROGRESS` are the SAME condition
 * from the client's side: the request reached the host, and the answer did not reach
 * us. The mutation may have applied, or not. Nothing the client knows can distinguish
 * those, so:
 *
 * - NEVER re-send. The host's dedup reservation is keyed on the client-minted
 *   `requestId`; a re-send with a fresh id is a second mutation, and a re-send with the
 *   same id races the reservation the first attempt is still holding.
 * - NEVER retry, and NEVER schedule a timer (A-5: the supervisor is the sole retry
 *   owner). A refetch that fails is just a refetch that failed.
 * - REFETCH the affected entity and let the host's state be the answer.
 *
 * Env scoping comes from the KEYED `QueryClient` (`getQueryClient(envId)`), never from
 * rewriting query keys (A-8). Passing the client in is what makes that structural: this
 * helper cannot invalidate another environment's cache because it never sees one.
 */

import type { QueryClient, QueryKey } from "@tanstack/react-query";

import { isRemoteTransportError } from "./transport-errors";

export interface UnknownOutcomeReconcileOptions {
  /** The environment-scoped client. A-8: scoping is the client, not the key. */
  readonly queryClient: QueryClient;
  /** The entity keys whose truth the failed mutation would have changed. */
  readonly queryKeys: readonly QueryKey[];
}

export type UnknownOutcomeResult =
  /** Not an unknown outcome — the caller keeps its existing error handling. */
  | { readonly kind: "not_unknown" }
  /** Reconciled: the listed entities were invalidated, nothing was re-sent. */
  | { readonly kind: "reconciled"; readonly message: string };

/**
 * The one sentence a user gets for an unknown outcome. Deliberately does NOT promise
 * the change was discarded — it may well have applied, and the refresh is what settles
 * it.
 */
export const UNKNOWN_OUTCOME_MESSAGE =
  "We couldn't confirm whether that change went through — refreshed from the host.";

export function reconcileUnknownOutcome(
  error: unknown,
  options: UnknownOutcomeReconcileOptions
): UnknownOutcomeResult {
  if (!isRemoteTransportError(error) || !error.isUnknownOutcome) {
    return { kind: "not_unknown" };
  }
  for (const queryKey of options.queryKeys) {
    void options.queryClient.invalidateQueries({ queryKey });
  }
  return { kind: "reconciled", message: UNKNOWN_OUTCOME_MESSAGE };
}
