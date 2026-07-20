import { describe, expect, it } from "vitest";

import { getVisibleIdeationArtifactTabs } from "./agentArtifactTabs";

describe("getVisibleIdeationArtifactTabs", () => {
  const baseAvailability = {
    hasAttachedIdeationSession: true,
    hasPlanArtifact: true,
    canStartPlan: false,
    hasVerificationEvidence: false,
    hasExecutionTasks: false,
  };

  it("returns the Plan tab for a plan-capable project conversation before a run is attached", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        ...baseAvailability,
        hasAttachedIdeationSession: false,
        hasPlanArtifact: false,
        canStartPlan: true,
      }),
    ).toEqual(["plan"]);
  });

  it("returns no tabs before an attached non-project ideation run has a plan", () => {
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

  it("does not add a dedicated verification tab", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        ...baseAvailability,
        hasVerificationEvidence: true,
      }),
    ).toEqual(["plan"]);
  });

  it("adds tasks only after the plan has execution tasks", () => {
    const tabs = getVisibleIdeationArtifactTabs({
      ...baseAvailability,
      hasVerificationEvidence: true,
      hasExecutionTasks: true,
    });

    expect(tabs).toEqual(["plan", "tasks"]);
    expect(tabs as readonly string[]).not.toContain("proposal");
  });
});
