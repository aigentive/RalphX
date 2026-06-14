import { z } from "zod";

import { backendApiUrl } from "@/api/backend";

const ProjectSkillStatusSchema = z.enum([
  "staged",
  "approved",
  "rejected",
  "archived",
  "retired",
]);

const ProjectSkillResponseSchema = z.object({
  id: z.string(),
  project_id: z.string(),
  title: z.string(),
  bucket: z.string(),
  stage: z.string(),
  status: ProjectSkillStatusSchema,
  pinned: z.boolean(),
  archived: z.boolean(),
  scope_paths: z.array(z.string()),
  compact_guidance: z.string(),
  body_markdown: z.string(),
  predicted_effect: z.string().nullable().optional(),
  provenance_json: z.unknown(),
  companion_of_skill_id: z.string().nullable().optional(),
  created_at: z.string(),
  updated_at: z.string(),
});

const ListProjectSkillsResponseSchema = z.object({
  skills: z.array(ProjectSkillResponseSchema),
  count: z.number(),
});

const ProjectSkillLifecycleResponseSchema = z.object({
  skill: ProjectSkillResponseSchema.nullable().optional(),
});

type RawProjectSkill = z.infer<typeof ProjectSkillResponseSchema>;

export type ProjectSkillStatus = z.infer<typeof ProjectSkillStatusSchema>;

export interface ProjectSkill {
  id: string;
  projectId: string;
  title: string;
  bucket: string;
  stage: string;
  status: ProjectSkillStatus;
  pinned: boolean;
  archived: boolean;
  scopePaths: string[];
  compactGuidance: string;
  bodyMarkdown: string;
  predictedEffect: string | null;
  provenance: unknown;
  companionOfSkillId: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ListProjectSkillsInput {
  projectId: string;
  status?: ProjectSkillStatus | null;
  includeArchived?: boolean;
  stage?: string | null;
  bucket?: string | null;
  scopePath?: string | null;
}

function transformProjectSkill(raw: RawProjectSkill): ProjectSkill {
  return {
    id: raw.id,
    projectId: raw.project_id,
    title: raw.title,
    bucket: raw.bucket,
    stage: raw.stage,
    status: raw.status,
    pinned: raw.pinned,
    archived: raw.archived,
    scopePaths: raw.scope_paths,
    compactGuidance: raw.compact_guidance,
    bodyMarkdown: raw.body_markdown,
    predictedEffect: raw.predicted_effect ?? null,
    provenance: raw.provenance_json,
    companionOfSkillId: raw.companion_of_skill_id ?? null,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

async function postJson<T>(endpoint: string, body: Record<string, unknown>): Promise<T> {
  const response = await fetch(backendApiUrl(endpoint), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) {
    throw new Error(
      `Project skill request failed: ${response.status} ${response.statusText}`,
    );
  }
  return (await response.json()) as T;
}

async function lifecycleAction(
  endpoint: "project_skills/approve" | "project_skills/reject" | "project_skills/archive",
  projectSkillId: string,
): Promise<ProjectSkill | null> {
  const raw = await postJson<unknown>(endpoint, {
    project_skill_id: projectSkillId,
  });
  const parsed = ProjectSkillLifecycleResponseSchema.parse(raw);
  return parsed.skill ? transformProjectSkill(parsed.skill) : null;
}

export const projectSkillsApi = {
  async list(input: ListProjectSkillsInput): Promise<ProjectSkill[]> {
    const raw = await postJson<unknown>("project_skills/list", {
      project_id: input.projectId,
      ...(input.status ? { status: input.status } : {}),
      include_archived: input.includeArchived ?? false,
      ...(input.stage ? { stage: input.stage } : {}),
      ...(input.bucket ? { bucket: input.bucket } : {}),
      ...(input.scopePath ? { scope_path: input.scopePath } : {}),
    });
    const parsed = ListProjectSkillsResponseSchema.parse(raw);
    return parsed.skills.map(transformProjectSkill);
  },

  approve(projectSkillId: string): Promise<ProjectSkill | null> {
    return lifecycleAction("project_skills/approve", projectSkillId);
  },

  reject(projectSkillId: string): Promise<ProjectSkill | null> {
    return lifecycleAction("project_skills/reject", projectSkillId);
  },

  archive(projectSkillId: string): Promise<ProjectSkill | null> {
    return lifecycleAction("project_skills/archive", projectSkillId);
  },
} as const;
