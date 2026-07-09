import { describe, it, expect } from "vitest";
import type { z } from "zod";
import { IdeationSessionResponseSchema } from "./ideation.schemas";
import { transformNullableBool, transformSession } from "./ideation.transforms";

type RawSession = z.infer<typeof IdeationSessionResponseSchema>;

const baseRaw: RawSession = {
  id: "sess-1",
  project_id: "proj-1",
  title: null,
  status: "active",
  plan_artifact_id: null,
  parent_session_id: null,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  archived_at: null,
  converted_at: null,
};

describe("transformNullableBool", () => {
  it("returns null for null input", () => {
    expect(transformNullableBool(null)).toBeNull();
  });

  it("returns null for undefined input", () => {
    expect(transformNullableBool(undefined)).toBeNull();
  });

  it("returns false for 0", () => {
    expect(transformNullableBool(0)).toBe(false);
  });

  it("returns true for 1", () => {
    expect(transformNullableBool(1)).toBe(true);
  });

  it("returns true for any non-zero number", () => {
    expect(transformNullableBool(2)).toBe(true);
    expect(transformNullableBool(-1)).toBe(true);
  });
});

describe("transformSession — lastEffectiveModel", () => {
  it("maps last_effective_model string to lastEffectiveModel", () => {
    const result = transformSession({ ...baseRaw, last_effective_model: "claude-sonnet-4-6" });
    expect(result.lastEffectiveModel).toBe("claude-sonnet-4-6");
  });

  it("returns null when last_effective_model is absent", () => {
    const result = transformSession({ ...baseRaw });
    expect(result.lastEffectiveModel).toBeNull();
  });

  it("returns null when last_effective_model is null", () => {
    const result = transformSession({ ...baseRaw, last_effective_model: null });
    expect(result.lastEffectiveModel).toBeNull();
  });
});

describe("transformSession — proposalGenerationProgress", () => {
  it("defaults legacy sessions without progress payload to idle progress", () => {
    const result = transformSession({ ...baseRaw });

    expect(result.proposalGenerationProgress).toEqual({
      status: "idle",
      phase: null,
      expectedCount: null,
      createdCount: 0,
      dependencyCount: null,
      error: null,
      startedAt: null,
      updatedAt: null,
      completedAt: null,
    });
  });

  it("maps active proposal-generation progress from snake_case response fields", () => {
    const result = transformSession({
      ...baseRaw,
      proposal_generation_progress: {
        status: "running",
        phase: "creating_proposals",
        expected_count: 5,
        created_count: 2,
        dependency_count: null,
        error: null,
        started_at: "2026-01-01T00:01:00Z",
        updated_at: "2026-01-01T00:02:00Z",
        completed_at: null,
      },
    });

    expect(result.proposalGenerationProgress).toEqual({
      status: "running",
      phase: "creating_proposals",
      expectedCount: 5,
      createdCount: 2,
      dependencyCount: null,
      error: null,
      startedAt: "2026-01-01T00:01:00Z",
      updatedAt: "2026-01-01T00:02:00Z",
      completedAt: null,
    });
  });

  it("accepts each terminal and active progress status", () => {
    const statuses = [
      "idle",
      "queued",
      "running",
      "waiting_for_confirmation",
      "completed",
      "failed",
      "cancelled",
    ] as const;

    for (const status of statuses) {
      expect(() =>
        transformSession({
          ...baseRaw,
          proposal_generation_progress: {
            status,
            phase: null,
            expected_count: null,
            created_count: 0,
            dependency_count: null,
            error: null,
            started_at: null,
            updated_at: null,
            completed_at: null,
          },
        }),
      ).not.toThrow();
    }
  });

  it("accepts each active and terminal progress phase", () => {
    const phases = [
      "queued",
      "creating_proposals",
      "analyzing_dependencies",
      "finalizing_proposals",
      "waiting_for_confirmation",
      "completed",
      "failed",
      "cancelled",
    ] as const;

    for (const phase of phases) {
      expect(() =>
        transformSession({
          ...baseRaw,
          proposal_generation_progress: {
            status: phase === "queued" ? "queued" : "running",
            phase,
            expected_count: null,
            created_count: 0,
            dependency_count: null,
            error: null,
            started_at: null,
            updated_at: null,
            completed_at: null,
          },
        }),
      ).not.toThrow();
    }
  });
});
