import { describe, expect, it } from "vitest";

import { getVisibleIdeationArtifactTabs } from "./agentArtifactTabs";

describe("getVisibleIdeationArtifactTabs", () => {
  const baseAvailability = {
    hasAttachedIdeationSession: true,
    hasPlanArtifact: true,
    hasProposals: false,
    hasVerificationEvidence: false,
    hasExecutionTasks: false,
    artifactMode: "plan",
  };

  it("returns no tabs before an attached ideation run has a plan", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        ...baseAvailability,
        hasPlanArtifact: false,
      }),
    ).toEqual([]);
  });

  it("returns the plan tab without an empty proposals tab once a plan exists", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        ...baseAvailability,
      }),
    ).toEqual(["plan"]);
  });

  it("adds proposals only when proposal content exists in plan or ideation mode", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        ...baseAvailability,
        hasProposals: true,
        artifactMode: "plan",
      }),
    ).toEqual(["plan", "proposal"]);

    expect(
      getVisibleIdeationArtifactTabs({
        ...baseAvailability,
        hasProposals: true,
        artifactMode: "ideation",
      }),
    ).toEqual(["plan", "proposal"]);
  });

  it("omits proposals outside plan and ideation modes even when proposals exist", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        ...baseAvailability,
        hasProposals: true,
        artifactMode: "edit",
      }),
    ).toEqual(["plan"]);
  });

  it("adds verification only when the session has verification evidence", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        ...baseAvailability,
        hasVerificationEvidence: true,
      }),
    ).toEqual(["plan", "verification"]);
  });

  it("adds tasks only after the plan has execution tasks", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        ...baseAvailability,
        hasProposals: true,
        hasVerificationEvidence: true,
        hasExecutionTasks: true,
      }),
    ).toEqual(["plan", "verification", "proposal", "tasks"]);
  });
});
