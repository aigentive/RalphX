import { useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { Query } from "@tanstack/react-query";
import { ideationApi } from "@/api/ideation";
import type { VerificationStatusResponse } from "@/api/ideation";
import { useEventBus } from "@/providers/EventProvider";

export const verificationStatusKey = (sessionId: string) =>
  ["verification", sessionId] as const;

export const verificationRefetchInterval = (
  query: Query<VerificationStatusResponse, Error>,
) => (query.state.data?.inProgress ? 2_000 : false);

export function useVerificationStatus(sessionId: string | undefined) {
  const bus = useEventBus();
  const queryClient = useQueryClient();
  const query = useQuery<VerificationStatusResponse, Error>({
    queryKey: sessionId ? verificationStatusKey(sessionId) : ["verification", "none"],
    queryFn: () => ideationApi.verification.getStatus(sessionId ?? ""),
    enabled: Boolean(sessionId),
    staleTime: 0,
    refetchOnMount: "always",
    refetchOnWindowFocus: false,
    refetchInterval: verificationRefetchInterval,
    retry: false,
  });
  useEffect(() => {
    if (!sessionId) return;
    const invalidate = () => {
      void queryClient.invalidateQueries({
        queryKey: verificationStatusKey(sessionId),
      });
    };
    const invalidateMatchingStatus = (payload: unknown) => {
      if (
        payload &&
        typeof payload === "object" &&
        "session_id" in payload &&
        payload.session_id === sessionId
      ) {
        invalidate();
      }
    };
    const invalidateMatchingLifecycle = (payload: unknown) => {
      if (
        payload &&
        typeof payload === "object" &&
        "context_type" in payload &&
        payload.context_type === "ideation" &&
        "context_id" in payload &&
        payload.context_id === sessionId
      ) {
        invalidate();
      }
    };
    const unsubscribes = [
      bus.subscribe(
        "plan_verification:status_changed",
        invalidateMatchingStatus,
      ),
      bus.subscribe("agent:run_started", invalidateMatchingLifecycle),
      bus.subscribe("agent:turn_completed", invalidateMatchingLifecycle),
      bus.subscribe("agent:run_completed", invalidateMatchingLifecycle),
      bus.subscribe("agent:error", invalidateMatchingLifecycle),
    ];
    return () => unsubscribes.forEach((unsubscribe) => unsubscribe());
  }, [bus, queryClient, sessionId]);
  return query;
}
