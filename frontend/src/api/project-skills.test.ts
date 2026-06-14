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

  it("returns null when a lifecycle endpoint finds no skill", async () => {
    fetchMock.mockResolvedValue(jsonResponse({ skill: null }));

    await expect(projectSkillsApi.reject("missing-skill")).resolves.toBeNull();
  });

  it("throws HTTP failures", async () => {
    fetchMock.mockResolvedValue(jsonResponse({}, { status: 500, statusText: "Server Error" }));

    await expect(projectSkillsApi.archive("skill-1")).rejects.toThrow(
      "Project skill request failed: 500 Server Error",
    );
  });
});
