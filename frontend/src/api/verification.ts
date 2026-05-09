/**
 * verification.ts — HTTP API wrappers for verification confirmation flow.
 *
 * Endpoints use the configured local backend. Follows same fetch pattern as
 * ideation.ts acceptance section.
 */

import { backendApiUrl } from "@/api/backend";
import { SpecialistsResponseSchema, PendingVerificationConfirmationsResponseSchema } from "@/types/verification-config";
import type { SpecialistsResponse, PendingVerificationConfirmationsResponse } from "@/types/verification-config";

// ============================================================================
// Internal helper
// ============================================================================

async function verificationFetch<T>(url: string, init: RequestInit, label: string): Promise<T> {
  const res = await fetch(url, init);
  if (!res.ok) {
    const body = await res.json().catch(() => ({})) as Record<string, unknown>;
    throw new Error((body as { error?: string }).error ?? `${label}: ${res.status}`);
  }
  return await res.json() as T;
}

export const verificationApi = {
  /**
   * Confirm verification — triggers verify with specialist config.
   * Handles both: confirming an existing PendingVerification entry (agent-triggered path)
   * and initiating fresh verification when no pending entry exists (user-initiated path).
   */
  confirm: async (sessionId: string, disabledSpecialists: string[]): Promise<{ status: string }> => {
    return verificationFetch(
      backendApiUrl("verification/confirm"),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ session_id: sessionId, disabled_specialists: disabledSpecialists }),
      },
      "Confirm verification failed"
    );
  },

  /**
   * Dismiss verification — removes pending entry, session stays Unverified.
   * No-op if no pending entry exists.
   */
  dismiss: async (sessionId: string): Promise<{ status: string }> => {
    return verificationFetch(
      backendApiUrl("verification/dismiss"),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ session_id: sessionId }),
      },
      "Dismiss verification failed"
    );
  },

  /**
   * Toggle auto-accept per-session on the backend (in-memory).
   * Also tracked in uiStore for frontend-only auto-accept logic.
   */
  setAutoAccept: async (sessionId: string, enabled: boolean): Promise<{ status: string }> => {
    return verificationFetch(
      backendApiUrl("verification/auto-accept"),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ session_id: sessionId, enabled }),
      },
      "Set auto-accept failed"
    );
  },

  /**
   * Get specialist list from registry.
   * Returns specialists configured in ralphx.yaml verification.specialists.
   */
  getSpecialists: async (): Promise<SpecialistsResponse> => {
    const res = await fetch(backendApiUrl("verification/specialists"));
    if (!res.ok) {
      throw new Error(`Failed to get specialists: ${res.status}`);
    }
    return SpecialistsResponseSchema.parse(await res.json());
  },

  /**
   * Get pending verification confirmations for a project.
   * Returns sessions with verification_confirmation_status = 'pending'.
   * Used by useVerificationBootstrap to hydrate the queue on startup and project switch.
   */
  getPendingVerificationConfirmations: async (projectId: string): Promise<PendingVerificationConfirmationsResponse> => {
    const data = await verificationFetch<unknown>(
      backendApiUrl(
        `verification/pending-confirmations?project_id=${encodeURIComponent(projectId)}`
      ),
      {},
      "Failed to get pending confirmations"
    );
    return PendingVerificationConfirmationsResponseSchema.parse(data);
  },
} as const;
