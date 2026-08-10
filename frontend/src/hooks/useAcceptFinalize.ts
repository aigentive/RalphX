/**
 * useAcceptFinalize — TanStack mutations for the acceptance gate.
 *
 * Provides accept and reject mutations for agent-initiated plan finalization
 * confirmation. After accept, invalidates relevant query keys so the UI
 * refreshes to show the accepted session state.
 */

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { ideationApi, RemoteFinalizeIntentError } from "@/api/ideation";
import { ideationKeys } from "./useIdeation";
import { taskKeys } from "./useTasks";
import { proposalKeys } from "./useProposals";

// ============================================================================
// Accept mutation
// ============================================================================

export function useAcceptFinalize(sessionId: string) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => ideationApi.acceptance.accept(sessionId),
    onSuccess: () => {
      // Mirror useApplyProposals.onSuccess + ideation:session_accepted event handler
      queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
      queryClient.invalidateQueries({ queryKey: ideationKeys.sessionWithData(sessionId) });
      queryClient.invalidateQueries({ queryKey: taskKeys.all });
      queryClient.invalidateQueries({ queryKey: proposalKeys.list(sessionId) });
      queryClient.invalidateQueries({ queryKey: ["plan-branch"] });
    },
    onError: (err: Error) => {
      if (
        err instanceof RemoteFinalizeIntentError &&
        err.errorCode === "REMOTE_FINALIZE_NOT_PENDING"
      ) {
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
        queryClient.invalidateQueries({
          queryKey: ideationKeys.sessionWithData(sessionId),
        });
      }
      toast.error("Failed to accept plan", { description: err.message });
    },
  });
}

// ============================================================================
// Reject mutation
// ============================================================================

export function useRejectFinalize(sessionId: string) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => ideationApi.acceptance.reject(sessionId),
    onSuccess: () => {
      // Refresh session so acceptance_status clears
      queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
      queryClient.invalidateQueries({ queryKey: ideationKeys.sessionWithData(sessionId) });
    },
    onError: (err: Error) => {
      if (
        err instanceof RemoteFinalizeIntentError &&
        err.errorCode === "REMOTE_FINALIZE_NOT_PENDING"
      ) {
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
        queryClient.invalidateQueries({
          queryKey: ideationKeys.sessionWithData(sessionId),
        });
      }
      toast.error("Failed to reject plan", { description: err.message });
    },
  });
}
