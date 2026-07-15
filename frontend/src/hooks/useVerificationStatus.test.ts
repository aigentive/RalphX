import { describe, expect, it } from "vitest";
import type { Query } from "@tanstack/react-query";
import type { VerificationStatusResponse } from "@/api/ideation";
import { verificationRefetchInterval } from "./useVerificationStatus";

function queryWithStatus(
  status: VerificationStatusResponse | undefined,
): Query<VerificationStatusResponse, Error> {
  return {
    state: { data: status },
  } as Query<VerificationStatusResponse, Error>;
}

describe("verificationRefetchInterval", () => {
  it("polls only while verification is queued or running", () => {
    expect(
      verificationRefetchInterval(
        queryWithStatus({
          sessionId: "session-1",
          status: "queued",
          inProgress: true,
          planArtifactId: "plan-1",
          verifiedPlanArtifactId: null,
          agentRunId: null,
          startedAt: null,
          completedAt: null,
          error: null,
        }),
      ),
    ).toBe(2_000);

    expect(
      verificationRefetchInterval(
        queryWithStatus({
          sessionId: "session-1",
          status: "verified",
          inProgress: false,
          planArtifactId: "plan-1",
          verifiedPlanArtifactId: "plan-1",
          agentRunId: "run-1",
          startedAt: null,
          completedAt: null,
          error: null,
        }),
      ),
    ).toBe(false);
  });
});
