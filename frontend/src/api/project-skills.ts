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

const DistillProjectSkillsResponseSchema = z.object({
  staged_skills: z.array(ProjectSkillResponseSchema),
  skipped_existing: z.number(),
});

const ProjectSkillExportFileResponseSchema = z.object({
  project_skill_id: z.string(),
  title: z.string(),
  relative_path: z.string(),
  pinned: z.boolean(),
  status: ProjectSkillStatusSchema,
  will_write: z.boolean(),
});

const ProjectSkillExportResponseSchema = z.object({
  project_id: z.string(),
  target_root: z.string(),
  files: z.array(ProjectSkillExportFileResponseSchema),
  count: z.number(),
});

const ProjectSkillSettingsResponseSchema = z.object({
  project_id: z.string(),
  export_enabled: z.boolean(),
});

type RawProjectSkill = z.infer<typeof ProjectSkillResponseSchema>;
type RawProjectSkillExport = z.infer<typeof ProjectSkillExportResponseSchema>;
type RawProjectSkillSettings = z.infer<typeof ProjectSkillSettingsResponseSchema>;

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

export interface DistillProjectSkillsInput {
  projectId: string;
  source?: string | null;
  limit?: number | null;
}

export interface DistillProjectSkillsResult {
  stagedSkills: ProjectSkill[];
  skippedExisting: number;
}

export interface ProjectSkillExportFile {
  projectSkillId: string;
  title: string;
  relativePath: string;
  pinned: boolean;
  status: ProjectSkillStatus;
  willWrite: boolean;
}

export interface ProjectSkillExportResult {
  projectId: string;
  targetRoot: string;
  files: ProjectSkillExportFile[];
  count: number;
}

export interface ProjectSkillSettings {
  projectId: string;
  exportEnabled: boolean;
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

function transformProjectSkillExport(raw: RawProjectSkillExport): ProjectSkillExportResult {
  return {
    projectId: raw.project_id,
    targetRoot: raw.target_root,
    count: raw.count,
    files: raw.files.map((file) => ({
      projectSkillId: file.project_skill_id,
      title: file.title,
      relativePath: file.relative_path,
      pinned: file.pinned,
      status: file.status,
      willWrite: file.will_write,
    })),
  };
}

function transformProjectSkillSettings(
  raw: RawProjectSkillSettings,
): ProjectSkillSettings {
  return {
    projectId: raw.project_id,
    exportEnabled: raw.export_enabled,
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
  endpoint:
    | "project_skills/approve"
    | "project_skills/reject"
    | "project_skills/archive"
    | "project_skills/pin"
    | "project_skills/unpin",
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

  pin(projectSkillId: string): Promise<ProjectSkill | null> {
    return lifecycleAction("project_skills/pin", projectSkillId);
  },

  unpin(projectSkillId: string): Promise<ProjectSkill | null> {
    return lifecycleAction("project_skills/unpin", projectSkillId);
  },

  async distill(input: DistillProjectSkillsInput): Promise<DistillProjectSkillsResult> {
    const raw = await postJson<unknown>("project_skills/distill", {
      project_id: input.projectId,
      ...(input.source ? { source: input.source } : {}),
      ...(input.limit != null ? { limit: input.limit } : {}),
    });
    const parsed = DistillProjectSkillsResponseSchema.parse(raw);
    return {
      stagedSkills: parsed.staged_skills.map(transformProjectSkill),
      skippedExisting: parsed.skipped_existing,
    };
  },

  async previewExport(projectId: string): Promise<ProjectSkillExportResult> {
    const raw = await postJson<unknown>("project_skills/export/preview", {
      project_id: projectId,
    });
    return transformProjectSkillExport(ProjectSkillExportResponseSchema.parse(raw));
  },

  async applyExport(projectId: string): Promise<ProjectSkillExportResult> {
    const raw = await postJson<unknown>("project_skills/export/apply", {
      project_id: projectId,
      confirm_export: true,
    });
    return transformProjectSkillExport(ProjectSkillExportResponseSchema.parse(raw));
  },

  async getSettings(projectId: string): Promise<ProjectSkillSettings> {
    const raw = await postJson<unknown>("project_skills/settings/get", {
      project_id: projectId,
    });
    return transformProjectSkillSettings(
      ProjectSkillSettingsResponseSchema.parse(raw),
    );
  },

  async updateSettings(
    projectId: string,
    settings: Pick<ProjectSkillSettings, "exportEnabled">,
  ): Promise<ProjectSkillSettings> {
    const raw = await postJson<unknown>("project_skills/settings/update", {
      project_id: projectId,
      export_enabled: settings.exportEnabled,
    });
    return transformProjectSkillSettings(
      ProjectSkillSettingsResponseSchema.parse(raw),
    );
  },
} as const;
