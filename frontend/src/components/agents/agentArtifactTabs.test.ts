import { describe, expect, it } from "vitest";

import { getVisibleIdeationArtifactTabs } from "./agentArtifactTabs";

describe("getVisibleIdeationArtifactTabs", () => {
  it("returns no tabs before an attached ideation run has a plan", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        hasAttachedIdeationSession: true,
        hasPlanArtifact: false,
        hasVerificationEvidence: false,
        hasExecutionTasks: false,
      }),
    ).toEqual([]);
  });

  it("returns plan-derived tabs once a plan exists", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        hasAttachedIdeationSession: true,
        hasPlanArtifact: true,
        hasVerificationEvidence: false,
        hasExecutionTasks: false,
      }),
    ).toEqual(["plan", "proposal"]);
  });

  it("adds verification only when the session has verification evidence", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        hasAttachedIdeationSession: true,
        hasPlanArtifact: true,
        hasVerificationEvidence: true,
        hasExecutionTasks: false,
      }),
    ).toEqual(["plan", "verification", "proposal"]);
  });

  it("adds tasks only after the plan has execution tasks", () => {
    expect(
      getVisibleIdeationArtifactTabs({
        hasAttachedIdeationSession: true,
        hasPlanArtifact: true,
        hasVerificationEvidence: true,
        hasExecutionTasks: true,
      }),
    ).toEqual(["plan", "verification", "proposal", "tasks"]);
  });
});
