import { useQuery } from "@tanstack/react-query";
import { ideationApi } from "@/api/ideation";
import type { VerificationStatusResponse } from "@/api/ideation";

export const verificationStatusKey = (sessionId: string) =>
  ["verification", sessionId] as const;

export function useVerificationStatus(sessionId: string | undefined) {
  return useQuery<VerificationStatusResponse, Error>({
    queryKey: sessionId ? verificationStatusKey(sessionId) : ["verification", "none"],
    queryFn: () => ideationApi.verification.getStatus(sessionId ?? ""),
    enabled: Boolean(sessionId),
    staleTime: 0,
    refetchOnMount: "always",
    refetchOnWindowFocus: false,
    retry: false,
  });
}
