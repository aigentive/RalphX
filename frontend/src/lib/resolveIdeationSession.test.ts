import { describe, expect, it } from "vitest";
import type { IdeationSession } from "@/types/ideation";
import { resolveIdeationSession } from "./resolveIdeationSession";

function makeSession(overrides: Partial<IdeationSession> = {}): IdeationSession {
  return {
    id: "session-1",
    projectId: "project-1",
    title: "Session",
    titleSource: "user",
    status: "active",
    planArtifactId: null,
    inheritedPlanArtifactId: null,
    seedTaskId: null,
    parentSessionId: null,
    createdAt: "2026-04-18T10:00:00.000Z",
    updatedAt: "2026-04-18T10:00:00.000Z",
    archivedAt: null,
    convertedAt: null,
    verificationStatus: "unverified",
    verificationInProgress: false,
    gapScore: null,
    verificationUpdateSeq: 0,
    planUpdateSeq: 0,
    sourceProjectId: null,
    sourceSessionId: null,
    sourceTaskId: null,
    sourceContextType: null,
    sourceContextId: null,
    spawnReason: null,
    blockerFingerprint: null,
    sessionPurpose: "general",
    acceptanceStatus: null,
    lastEffectiveModel: null,
    ...overrides,
  };
}

describe("resolveIdeationSession", () => {
  it("uses fresher store plan fields when planUpdateSeq is greater than zero", () => {
    const fetchedSession = makeSession({
      planArtifactId: null,
      inheritedPlanArtifactId: null,
      planUpdateSeq: 0,
    });
    const activeSession = makeSession({
      planArtifactId: "plan-2",
      inheritedPlanArtifactId: null,
      planUpdateSeq: 1,
    });

    const resolved = resolveIdeationSession(fetchedSession, activeSession);

    expect(resolved?.planArtifactId).toBe("plan-2");
    expect(resolved?.inheritedPlanArtifactId).toBeNull();
  });

  it("preserves inherited plan links from fresher store follow-up sessions", () => {
    const fetchedSession = makeSession({
      planArtifactId: null,
      inheritedPlanArtifactId: "plan-1",
      planUpdateSeq: 0,
    });
    const activeSession = makeSession({
      planArtifactId: null,
      inheritedPlanArtifactId: "plan-2",
      planUpdateSeq: 2,
    });

    const resolved = resolveIdeationSession(fetchedSession, activeSession);

    expect(resolved?.planArtifactId).toBeNull();
    expect(resolved?.inheritedPlanArtifactId).toBe("plan-2");
  });

  it("keeps React Query plan fields when no plan event has made the store fresher", () => {
    const fetchedSession = makeSession({
      planArtifactId: "plan-query",
      inheritedPlanArtifactId: null,
      planUpdateSeq: 0,
    });
    const activeSession = makeSession({
      planArtifactId: null,
      inheritedPlanArtifactId: null,
      planUpdateSeq: 0,
    });

    const resolved = resolveIdeationSession(fetchedSession, activeSession);

    expect(resolved?.planArtifactId).toBe("plan-query");
  });
});
