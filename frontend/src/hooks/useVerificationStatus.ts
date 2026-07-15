import { useQuery } from "@tanstack/react-query";
import type { Query } from "@tanstack/react-query";
import { ideationApi } from "@/api/ideation";
import type { VerificationStatusResponse } from "@/api/ideation";

export const verificationStatusKey = (sessionId: string) =>
  ["verification", sessionId] as const;

export const verificationRefetchInterval = (
  query: Query<VerificationStatusResponse, Error>,
) => (query.state.data?.inProgress ? 2_000 : false);

export function useVerificationStatus(sessionId: string | undefined) {
  return useQuery<VerificationStatusResponse, Error>({
    queryKey: sessionId ? verificationStatusKey(sessionId) : ["verification", "none"],
    queryFn: () => ideationApi.verification.getStatus(sessionId ?? ""),
    enabled: Boolean(sessionId),
    staleTime: 0,
    refetchOnMount: "always",
    refetchOnWindowFocus: false,
    refetchInterval: verificationRefetchInterval,
    retry: false,
  });
}
