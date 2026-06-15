import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { projectSkillsApi } from "./project-skills";

const fetchMock = vi.fn();

function jsonResponse(body: unknown, init: ResponseInit = {}) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
    ...init,
  });
}

function projectSkill(overrides: Record<string, unknown> = {}) {
  return {
    id: "skill-1",
    project_id: "project-1",
    title: "Check merge validation",
    bucket: "merge",
    stage: "review",
    status: "staged",
    pinned: false,
    archived: false,
    scope_paths: ["src-tauri"],
    compact_guidance: "Check validation failures before approval.",
    body_markdown: "Detailed guidance",
    predicted_effect: "Prevents repeated validation loops.",
    provenance_json: { outcome_id: "outcome-1" },
    companion_of_skill_id: null,
    created_at: "2026-06-14T10:00:00Z",
    updated_at: "2026-06-14T10:00:00Z",
    ...overrides,
  };
}

function exportResponse(overrides: Record<string, unknown> = {}) {
  return {
    project_id: "project-1",
    target_root: "/repo/.claude/skills",
    count: 1,
    files: [
      {
        project_skill_id: "skill-1",
        title: "Check merge validation",
        relative_path: ".claude/skills/check-merge-validation/SKILL.md",
        pinned: true,
        status: "approved",
        will_write: true,
      },
    ],
    ...overrides,
  };
}

describe("projectSkillsApi", () => {
  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("lists project skills and transforms backend fields", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({
        skills: [projectSkill()],
        count: 1,
      }),
    );

    await expect(
      projectSkillsApi.list({
        projectId: "project-1",
        status: "staged",
        includeArchived: true,
        stage: "review",
        bucket: "merge",
        scopePath: "src-tauri",
      }),
    ).resolves.toEqual([
      {
        id: "skill-1",
        projectId: "project-1",
        title: "Check merge validation",
        bucket: "merge",
        stage: "review",
        status: "staged",
        pinned: false,
        archived: false,
        scopePaths: ["src-tauri"],
        compactGuidance: "Check validation failures before approval.",
        bodyMarkdown: "Detailed guidance",
        predictedEffect: "Prevents repeated validation loops.",
        provenance: { outcome_id: "outcome-1" },
        companionOfSkillId: null,
        createdAt: "2026-06-14T10:00:00Z",
        updatedAt: "2026-06-14T10:00:00Z",
      },
    ]);

    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:3847/api/project_skills/list",
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          project_id: "project-1",
          status: "staged",
          include_archived: true,
          stage: "review",
          bucket: "merge",
          scope_path: "src-tauri",
        }),
      },
    );
  });

  it("approves project skills through the lifecycle endpoint", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({
        skill: projectSkill({ status: "approved" }),
      }),
    );

    const approved = await projectSkillsApi.approve("skill-1");

    expect(approved?.status).toBe("approved");
    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:3847/api/project_skills/approve",
      expect.objectContaining({
        body: JSON.stringify({ project_skill_id: "skill-1" }),
      }),
    );
  });

  it("pins and unpins project skills through lifecycle endpoints", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        skill: projectSkill({ status: "approved", pinned: true }),
      }),
    );
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        skill: projectSkill({ status: "approved", pinned: false }),
      }),
    );

    await expect(projectSkillsApi.pin("skill-1")).resolves.toMatchObject({
      pinned: true,
    });
    await expect(projectSkillsApi.unpin("skill-1")).resolves.toMatchObject({
      pinned: false,
    });

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://localhost:3847/api/project_skills/pin",
      expect.objectContaining({
        body: JSON.stringify({ project_skill_id: "skill-1" }),
      }),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "http://localhost:3847/api/project_skills/unpin",
      expect.objectContaining({
        body: JSON.stringify({ project_skill_id: "skill-1" }),
      }),
    );
  });

  it("returns null when a lifecycle endpoint finds no skill", async () => {
    fetchMock.mockResolvedValue(jsonResponse({ skill: null }));

    await expect(projectSkillsApi.reject("missing-skill")).resolves.toBeNull();
  });

  it("distills eligible outcomes into staged skills", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({
        staged_skills: [projectSkill({ id: "skill-2", status: "staged" })],
        skipped_existing: 1,
      }),
    );

    await expect(
      projectSkillsApi.distill({
        projectId: "project-1",
        source: "review",
        limit: 5,
      }),
    ).resolves.toMatchObject({
      stagedSkills: [{ id: "skill-2", status: "staged" }],
      skippedExisting: 1,
    });
    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:3847/api/project_skills/distill",
      expect.objectContaining({
        body: JSON.stringify({
          project_id: "project-1",
          source: "review",
          limit: 5,
        }),
      }),
    );
  });

  it("lists conservative project skill report cards", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({
        cards: [
          {
            project_skill_id: "skill-1",
            title: "Check merge validation",
            bucket: "merge",
            stage: "review",
            pinned: false,
            usage_count: 3,
            linked_outcome_count: 2,
            succeeded_outcome_count: 1,
            failed_outcome_count: 1,
            unknown_outcome_count: 0,
            last_used_at: "2026-06-15T10:00:00Z",
            age_days: 1,
            aging_status: "active",
            evidence_level: "insufficient_data",
          },
        ],
        count: 1,
      }),
    );

    await expect(
      projectSkillsApi.listReportCards({
        projectId: "project-1",
        minLinkedOutcomes: 5,
        staleAfterDays: 30,
      }),
    ).resolves.toEqual({
      count: 1,
      cards: [
        {
          projectSkillId: "skill-1",
          title: "Check merge validation",
          bucket: "merge",
          stage: "review",
          pinned: false,
          usageCount: 3,
          linkedOutcomeCount: 2,
          succeededOutcomeCount: 1,
          failedOutcomeCount: 1,
          unknownOutcomeCount: 0,
          lastUsedAt: "2026-06-15T10:00:00Z",
          ageDays: 1,
          agingStatus: "active",
          evidenceLevel: "insufficient_data",
        },
      ],
    });
    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:3847/api/project_skills/report_cards",
      expect.objectContaining({
        body: JSON.stringify({
          project_id: "project-1",
          min_linked_outcomes: 5,
          stale_after_days: 30,
        }),
      }),
    );
  });

  it("previews project skill imports with fail-closed scanner decisions", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse({
        rows: [
          {
            index: 0,
            external_id: "manifest-skill-1",
            title: "Check imported guidance",
            decision: "invalid",
            reasons: ["source snapshot is required before import"],
            duplicate_project_skill_id: null,
          },
        ],
        eligible_count: 0,
        invalid_count: 1,
        duplicate_count: 0,
      }),
    );

    await expect(
      projectSkillsApi.previewImport({
        projectId: "project-1",
        candidates: [
          {
            externalId: "manifest-skill-1",
            title: "Check imported guidance",
            bucket: "review",
            stage: "review",
            scopePaths: ["src-tauri"],
            compactGuidance: "Check imported guidance before reviews.",
            bodyMarkdown: "Detailed guidance",
            predictedEffect: "Reduces repeated review misses.",
            provenance: { source: "manifest" },
            sourceSnapshot: null,
          },
        ],
      }),
    ).resolves.toEqual({
      rows: [
        {
          index: 0,
          externalId: "manifest-skill-1",
          title: "Check imported guidance",
          decision: "invalid",
          reasons: ["source snapshot is required before import"],
          duplicateProjectSkillId: null,
        },
      ],
      eligibleCount: 0,
      invalidCount: 1,
      duplicateCount: 0,
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:3847/api/project_skills/import/preview",
      expect.objectContaining({
        body: JSON.stringify({
          project_id: "project-1",
          candidates: [
            {
              external_id: "manifest-skill-1",
              title: "Check imported guidance",
              bucket: "review",
              stage: "review",
              scope_paths: ["src-tauri"],
              compact_guidance: "Check imported guidance before reviews.",
              body_markdown: "Detailed guidance",
              predicted_effect: "Reduces repeated review misses.",
              provenance_json: { source: "manifest" },
              source_snapshot_json: null,
            },
          ],
        }),
      }),
    );
  });

  it("previews project skill export files", async () => {
    fetchMock.mockResolvedValue(jsonResponse(exportResponse()));

    await expect(projectSkillsApi.previewExport("project-1")).resolves.toEqual({
      projectId: "project-1",
      targetRoot: "/repo/.claude/skills",
      count: 1,
      files: [
        {
          projectSkillId: "skill-1",
          title: "Check merge validation",
          relativePath: ".claude/skills/check-merge-validation/SKILL.md",
          pinned: true,
          status: "approved",
          willWrite: true,
        },
      ],
    });
    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:3847/api/project_skills/export/preview",
      expect.objectContaining({
        body: JSON.stringify({ project_id: "project-1" }),
      }),
    );
  });

  it("applies project skill export with explicit confirmation", async () => {
    fetchMock.mockResolvedValue(jsonResponse(exportResponse()));

    await expect(projectSkillsApi.applyExport("project-1")).resolves.toMatchObject({
      count: 1,
    });
    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:3847/api/project_skills/export/apply",
      expect.objectContaining({
        body: JSON.stringify({
          project_id: "project-1",
          confirm_export: true,
        }),
      }),
    );
  });

  it("reads and updates project skill settings", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        project_id: "project-1",
        export_enabled: false,
      }),
    );
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        project_id: "project-1",
        export_enabled: true,
      }),
    );

    await expect(projectSkillsApi.getSettings("project-1")).resolves.toEqual({
      projectId: "project-1",
      exportEnabled: false,
    });
    await expect(
      projectSkillsApi.updateSettings("project-1", { exportEnabled: true }),
    ).resolves.toEqual({
      projectId: "project-1",
      exportEnabled: true,
    });

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://localhost:3847/api/project_skills/settings/get",
      expect.objectContaining({
        body: JSON.stringify({ project_id: "project-1" }),
      }),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "http://localhost:3847/api/project_skills/settings/update",
      expect.objectContaining({
        body: JSON.stringify({
          project_id: "project-1",
          export_enabled: true,
        }),
      }),
    );
  });

  it("throws HTTP failures", async () => {
    fetchMock.mockResolvedValue(jsonResponse({}, { status: 500, statusText: "Server Error" }));

    await expect(projectSkillsApi.archive("skill-1")).rejects.toThrow(
      "Project skill request failed: 500 Server Error",
    );
  });
});
