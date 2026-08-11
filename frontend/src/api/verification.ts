/**
 * verification.ts — HTTP API wrappers for verification confirmation flow.
 *
 * Endpoints use the configured local backend. Follows same fetch pattern as
 * ideation.ts acceptance section.
 */

import { backendApiUrl } from "@/api/backend";

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
   * Queue a visible Verify Plan turn in the active Plan conversation.
   */
  confirm: async (sessionId: string): Promise<{ status: string }> => {
    return verificationFetch(
      backendApiUrl("verification/confirm"),
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ session_id: sessionId }),
      },
      "Confirm verification failed"
    );
  },
} as const;
