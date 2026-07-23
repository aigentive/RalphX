import { z } from "zod";

import { backendApiUrl } from "@/api/backend";

const ProjectSkillStatusSchema = z.enum([
  "staged",
  "approved",
  "rejected",
  "stale",
  "archived",
  "retired",
]);

export const PROJECT_SKILL_CATEGORY_VALUES = [
  "planning",
  "verification",
  "review",
  "execution",
  "merge",
] as const;

const ProjectSkillCategorySchema = z.enum(PROJECT_SKILL_CATEGORY_VALUES);

const ProjectSkillResponseSchema = z.object({
  id: z.string(),
  project_id: z.string(),
  title: z.string(),
  bucket: ProjectSkillCategorySchema,
  stage: ProjectSkillCategorySchema,
  status: ProjectSkillStatusSchema,
  pinned: z.boolean(),
  archived: z.boolean(),
  scope_paths: z.array(z.string()),
  compact_guidance: z.string(),
  body_markdown: z.string(),
  predicted_effect: z.string().nullable().optional(),
  provenance_json: z.unknown(),
  companion_of_skill_id: z.string().nullable().optional(),
  content_hash: z.string().regex(/^[0-9a-f]{64}$/),
  evidence_hash: z.string().regex(/^[0-9a-f]{64}$/),
  created_by: z.enum(["user", "agent", "imported"]),
  pipeline_role: z.string().trim().min(1).nullable().optional(),
  created_at: z.string(),
  updated_at: z.string(),
});

const ListProjectSkillsResponseSchema = z.object({
  skills: z.array(ProjectSkillResponseSchema),
  count: z.number(),
});

const ConversationProjectSkillResponseSchema = z.object({
  skill: ProjectSkillResponseSchema,
  generated_by_conversation: z.boolean(),
  used_by_conversation: z.boolean(),
  usage_count: z.number(),
});

const ListConversationProjectSkillsResponseSchema = z.object({
  skills: z.array(ConversationProjectSkillResponseSchema),
  count: z.number(),
});

const ProcessConversationProjectSkillsResponseSchema = z.object({
  staged_skills: z.array(ProjectSkillResponseSchema),
  skipped_existing: z.number(),
  message_count: z.number(),
});

const ProjectSkillLifecycleResponseSchema = z.object({
  skill: ProjectSkillResponseSchema.nullable().optional(),
});

const DistillProjectSkillsResponseSchema = z.object({
  staged_skills: z.array(ProjectSkillResponseSchema),
  skipped_existing: z.number(),
  updated_existing: z.number().optional().default(0),
  ingested_outcomes: z.number().optional().default(0),
  scanned_git_commits: z.number().optional().default(0),
  scanned_github_prs: z.number().optional().default(0),
});

const ProjectSkillPullRequestCandidateResponseSchema = z.object({
  number: z.number(),
  title: z.string(),
  state: z.string().nullable().optional(),
  url: z.string().nullable().optional(),
  merged_at: z.string().nullable().optional(),
  closed_at: z.string().nullable().optional(),
  updated_at: z.string().nullable().optional(),
  head_ref_name: z.string().nullable().optional(),
  base_ref_name: z.string().nullable().optional(),
});

const ListProjectSkillPullRequestCandidatesResponseSchema = z.object({
  candidates: z.array(ProjectSkillPullRequestCandidateResponseSchema),
  count: z.number(),
  limit: z.number(),
});

const StageProjectSkillFromPullRequestResponseSchema = z.object({
  skill: ProjectSkillResponseSchema.nullable().optional(),
  skipped_existing: z.boolean(),
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
  enabled: z.boolean(),
  auto_inject: z.boolean(),
  auto_distill: z.boolean(),
  injection_max_skills: z.number().int().positive(),
  injection_max_chars: z.number().int().positive(),
  injection_guidance_max_chars: z.number().int().positive(),
  report_min_outcomes: z.number().int().positive(),
  verification_corpus_gate: z.number().int().nonnegative(),
  export_enabled: z.boolean(),
});

const ProjectSkillReportCardResponseSchema = z.object({
  project_skill_id: z.string(),
  title: z.string(),
  bucket: ProjectSkillCategorySchema,
  stage: ProjectSkillCategorySchema,
  pinned: z.boolean(),
  usage_count: z.number(),
  linked_outcome_count: z.number(),
  succeeded_outcome_count: z.number(),
  failed_outcome_count: z.number(),
  unknown_outcome_count: z.number(),
  last_used_at: z.string().nullable().optional(),
  age_days: z.number(),
  aging_status: z.enum(["active", "stale", "unused"]),
  evidence_level: z.enum(["insufficient_data", "descriptive"]),
});

const ProjectSkillReportCardsResponseSchema = z.object({
  cards: z.array(ProjectSkillReportCardResponseSchema),
  count: z.number(),
});

const ProjectSkillImportPreviewRowResponseSchema = z.object({
  index: z.number(),
  external_id: z.string().nullable().optional(),
  title: z.string(),
  decision: z.enum(["eligible", "invalid", "duplicate"]),
  reasons: z.array(z.string()),
  duplicate_project_skill_id: z.string().nullable().optional(),
});

const ProjectSkillImportPreviewResponseSchema = z.object({
  rows: z.array(ProjectSkillImportPreviewRowResponseSchema),
  eligible_count: z.number(),
  invalid_count: z.number(),
  duplicate_count: z.number(),
});

const ProjectSkillImportApplyResponseSchema = z.object({
  preview: ProjectSkillImportPreviewResponseSchema,
  imported_skills: z.array(ProjectSkillResponseSchema),
  imported_count: z.number(),
  synced_count: z.number().optional().default(0),
});

const PromoteMemoryToProjectSkillResponseSchema = z.object({
  skill: ProjectSkillResponseSchema,
});

type RawProjectSkill = z.infer<typeof ProjectSkillResponseSchema>;
type RawConversationProjectSkill = z.infer<
  typeof ConversationProjectSkillResponseSchema
>;
type RawProjectSkillExport = z.infer<typeof ProjectSkillExportResponseSchema>;
type RawProjectSkillSettings = z.infer<typeof ProjectSkillSettingsResponseSchema>;
type RawProjectSkillReportCard = z.infer<typeof ProjectSkillReportCardResponseSchema>;
type RawProjectSkillImportPreviewRow = z.infer<
  typeof ProjectSkillImportPreviewRowResponseSchema
>;

export type ProjectSkillStatus = z.infer<typeof ProjectSkillStatusSchema>;
export type ProjectSkillCategory = z.infer<typeof ProjectSkillCategorySchema>;

export interface ProjectSkill {
  id: string;
  projectId: string;
  title: string;
  bucket: ProjectSkillCategory;
  stage: ProjectSkillCategory;
  status: ProjectSkillStatus;
  pinned: boolean;
  archived: boolean;
  scopePaths: string[];
  compactGuidance: string;
  bodyMarkdown: string;
  predictedEffect: string | null;
  provenance: unknown;
  sourcePath: string | null;
  sourceRoot: string | null;
  sourceSyncEnabled: boolean;
  companionOfSkillId: string | null;
  contentHash: string;
  evidenceHash: string;
  createdBy: "user" | "agent" | "imported";
  pipelineRole: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface ListProjectSkillsInput {
  projectId: string;
  status?: ProjectSkillStatus | null;
  includeArchived?: boolean;
  stage?: ProjectSkillCategory | null;
  bucket?: ProjectSkillCategory | null;
  scopePath?: string | null;
}

export interface ListConversationProjectSkillsInput {
  projectId: string;
  conversationId: string;
}

export interface ConversationProjectSkill {
  skill: ProjectSkill;
  generatedByConversation: boolean;
  usedByConversation: boolean;
  usageCount: number;
}

export interface ProcessConversationProjectSkillsResult {
  stagedSkills: ProjectSkill[];
  skippedExisting: number;
  messageCount: number;
}

export interface DistillProjectSkillsInput {
  projectId: string;
  source?: string | null;
  limit?: number | null;
  includeGitHistory?: boolean | null;
  includeGithubPrHistory?: boolean | null;
}

export interface DistillProjectSkillsResult {
  stagedSkills: ProjectSkill[];
  skippedExisting: number;
  updatedExisting: number;
  ingestedOutcomes: number;
  scannedGitCommits: number;
  scannedGithubPrs: number;
}

export interface ProjectSkillPullRequestCandidate {
  number: number;
  title: string;
  state: string | null;
  url: string | null;
  mergedAt: string | null;
  closedAt: string | null;
  updatedAt: string | null;
  headRefName: string | null;
  baseRefName: string | null;
}

export interface ListProjectSkillPullRequestCandidatesResult {
  candidates: ProjectSkillPullRequestCandidate[];
  count: number;
  limit: number;
}

export interface StageProjectSkillFromPullRequestResult {
  skill: ProjectSkill | null;
  skippedExisting: boolean;
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
  enabled: boolean;
  autoInject: boolean;
  autoDistill: boolean;
  injectionMaxSkills: number;
  injectionMaxChars: number;
  injectionGuidanceMaxChars: number;
  reportMinOutcomes: number;
  verificationCorpusGate: number;
  exportEnabled: boolean;
}

export type ProjectSkillSettingsPatch = Partial<
  Omit<ProjectSkillSettings, "projectId">
>;

export interface ProjectSkillReportCard {
  projectSkillId: string;
  title: string;
  bucket: ProjectSkillCategory;
  stage: ProjectSkillCategory;
  pinned: boolean;
  usageCount: number;
  linkedOutcomeCount: number;
  succeededOutcomeCount: number;
  failedOutcomeCount: number;
  unknownOutcomeCount: number;
  lastUsedAt: string | null;
  ageDays: number;
  agingStatus: "active" | "stale" | "unused";
  evidenceLevel: "insufficient_data" | "descriptive";
}

export interface ListProjectSkillReportCardsInput {
  projectId: string;
  minLinkedOutcomes?: number | null;
  staleAfterDays?: number | null;
}

export interface ProjectSkillReportCardsResult {
  cards: ProjectSkillReportCard[];
  count: number;
}

export interface ProjectSkillImportCandidate {
  externalId?: string | null;
  title: string;
  bucket: ProjectSkillCategory;
  stage: ProjectSkillCategory;
  scopePaths?: string[];
  compactGuidance: string;
  bodyMarkdown: string;
  predictedEffect: string;
  provenance: unknown;
  sourceSnapshot: unknown;
}

export interface PreviewProjectSkillImportInput {
  projectId: string;
  candidates: ProjectSkillImportCandidate[];
}

export interface ProjectSkillImportPreviewRow {
  index: number;
  externalId: string | null;
  title: string;
  decision: "eligible" | "invalid" | "duplicate";
  reasons: string[];
  duplicateProjectSkillId: string | null;
}

export interface ProjectSkillImportPreviewResult {
  rows: ProjectSkillImportPreviewRow[];
  eligibleCount: number;
  invalidCount: number;
  duplicateCount: number;
}

export interface ApplyProjectSkillImportInput extends PreviewProjectSkillImportInput {
  confirmImport: boolean;
}

export interface ProjectSkillImportApplyResult {
  preview: ProjectSkillImportPreviewResult;
  importedSkills: ProjectSkill[];
  importedCount: number;
  syncedCount: number;
}

export interface ProjectSkillDirectoryImportInput {
  projectId: string;
  sourceRoots?: string[];
  sourceSyncEnabled?: boolean;
}

export interface PromoteMemoryToProjectSkillInput {
  projectId: string;
  memoryId: string;
  title?: string | null;
  bucket: ProjectSkillCategory;
  stage: ProjectSkillCategory;
  compactGuidance: string;
  bodyMarkdown: string;
  predictedEffect: string;
}

export interface UpdateProjectSkillInput {
  projectSkillId: string;
  title: string;
  bucket: ProjectSkillCategory;
  stage: ProjectSkillCategory;
  scopePaths: string[];
  compactGuidance: string;
  bodyMarkdown: string;
  predictedEffect: string;
  sourceSyncEnabled?: boolean | null;
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value != null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

function booleanValue(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function sourceSnapshotValue(
  provenance: unknown,
): Record<string, unknown> | null {
  const object = objectValue(provenance);
  return objectValue(object?.source_snapshot);
}

function transformProjectSkill(raw: RawProjectSkill): ProjectSkill {
  const provenance = raw.provenance_json;
  const provenanceObject = objectValue(provenance);
  const sourceSnapshot = sourceSnapshotValue(provenance);
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
    provenance,
    sourcePath:
      stringValue(provenanceObject?.external_id) ??
      stringValue(sourceSnapshot?.relative_path),
    sourceRoot: stringValue(sourceSnapshot?.source_root),
    sourceSyncEnabled:
      booleanValue(provenanceObject?.source_sync_enabled) ??
      booleanValue(sourceSnapshot?.source_sync_enabled) ??
      false,
    companionOfSkillId: raw.companion_of_skill_id ?? null,
    contentHash: raw.content_hash,
    evidenceHash: raw.evidence_hash,
    createdBy: raw.created_by,
    pipelineRole: raw.pipeline_role ?? null,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

function transformPullRequestCandidate(
  raw: z.infer<typeof ProjectSkillPullRequestCandidateResponseSchema>,
): ProjectSkillPullRequestCandidate {
  return {
    number: raw.number,
    title: raw.title,
    state: raw.state ?? null,
    url: raw.url ?? null,
    mergedAt: raw.merged_at ?? null,
    closedAt: raw.closed_at ?? null,
    updatedAt: raw.updated_at ?? null,
    headRefName: raw.head_ref_name ?? null,
    baseRefName: raw.base_ref_name ?? null,
  };
}

function transformConversationProjectSkill(
  raw: RawConversationProjectSkill,
): ConversationProjectSkill {
  return {
    skill: transformProjectSkill(raw.skill),
    generatedByConversation: raw.generated_by_conversation,
    usedByConversation: raw.used_by_conversation,
    usageCount: raw.usage_count,
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
    enabled: raw.enabled,
    autoInject: raw.auto_inject,
    autoDistill: raw.auto_distill,
    injectionMaxSkills: raw.injection_max_skills,
    injectionMaxChars: raw.injection_max_chars,
    injectionGuidanceMaxChars: raw.injection_guidance_max_chars,
    reportMinOutcomes: raw.report_min_outcomes,
    verificationCorpusGate: raw.verification_corpus_gate,
    exportEnabled: raw.export_enabled,
  };
}

function transformProjectSkillReportCard(
  raw: RawProjectSkillReportCard,
): ProjectSkillReportCard {
  return {
    projectSkillId: raw.project_skill_id,
    title: raw.title,
    bucket: raw.bucket,
    stage: raw.stage,
    pinned: raw.pinned,
    usageCount: raw.usage_count,
    linkedOutcomeCount: raw.linked_outcome_count,
    succeededOutcomeCount: raw.succeeded_outcome_count,
    failedOutcomeCount: raw.failed_outcome_count,
    unknownOutcomeCount: raw.unknown_outcome_count,
    lastUsedAt: raw.last_used_at ?? null,
    ageDays: raw.age_days,
    agingStatus: raw.aging_status,
    evidenceLevel: raw.evidence_level,
  };
}

function transformProjectSkillImportPreviewRow(
  raw: RawProjectSkillImportPreviewRow,
): ProjectSkillImportPreviewRow {
  return {
    index: raw.index,
    externalId: raw.external_id ?? null,
    title: raw.title,
    decision: raw.decision,
    reasons: raw.reasons,
    duplicateProjectSkillId: raw.duplicate_project_skill_id ?? null,
  };
}

function transformProjectSkillImportPreview(
  raw: z.infer<typeof ProjectSkillImportPreviewResponseSchema>,
): ProjectSkillImportPreviewResult {
  return {
    rows: raw.rows.map(transformProjectSkillImportPreviewRow),
    eligibleCount: raw.eligible_count,
    invalidCount: raw.invalid_count,
    duplicateCount: raw.duplicate_count,
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

  async listForConversation(
    input: ListConversationProjectSkillsInput,
  ): Promise<ConversationProjectSkill[]> {
    const raw = await postJson<unknown>("project_skills/conversation", {
      project_id: input.projectId,
      conversation_id: input.conversationId,
    });
    const parsed = ListConversationProjectSkillsResponseSchema.parse(raw);
    return parsed.skills.map(transformConversationProjectSkill);
  },

  async processConversation(
    input: ListConversationProjectSkillsInput,
  ): Promise<ProcessConversationProjectSkillsResult> {
    const raw = await postJson<unknown>("project_skills/conversation/process", {
      project_id: input.projectId,
      conversation_id: input.conversationId,
    });
    const parsed = ProcessConversationProjectSkillsResponseSchema.parse(raw);
    return {
      stagedSkills: parsed.staged_skills.map(transformProjectSkill),
      skippedExisting: parsed.skipped_existing,
      messageCount: parsed.message_count,
    };
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
      ...(input.includeGitHistory != null
        ? { include_git_history: input.includeGitHistory }
        : {}),
      ...(input.includeGithubPrHistory != null
        ? { include_github_pr_history: input.includeGithubPrHistory }
        : {}),
    });
    const parsed = DistillProjectSkillsResponseSchema.parse(raw);
    return {
      stagedSkills: parsed.staged_skills.map(transformProjectSkill),
      skippedExisting: parsed.skipped_existing,
      updatedExisting: parsed.updated_existing,
      ingestedOutcomes: parsed.ingested_outcomes,
      scannedGitCommits: parsed.scanned_git_commits,
      scannedGithubPrs: parsed.scanned_github_prs,
    };
  },

  async listPullRequestCandidates(input: {
    projectId: string;
    limit?: number | null;
  }): Promise<ListProjectSkillPullRequestCandidatesResult> {
    const raw = await postJson<unknown>("project_skills/pr_candidates/list", {
      project_id: input.projectId,
      ...(input.limit != null ? { limit: input.limit } : {}),
    });
    const parsed = ListProjectSkillPullRequestCandidatesResponseSchema.parse(raw);
    return {
      candidates: parsed.candidates.map(transformPullRequestCandidate),
      count: parsed.count,
      limit: parsed.limit,
    };
  },

  async stageFromPullRequest(input: {
    projectId: string;
    number: number;
  }): Promise<StageProjectSkillFromPullRequestResult> {
    const raw = await postJson<unknown>("project_skills/pr_candidates/stage", {
      project_id: input.projectId,
      number: input.number,
    });
    const parsed = StageProjectSkillFromPullRequestResponseSchema.parse(raw);
    return {
      skill: parsed.skill ? transformProjectSkill(parsed.skill) : null,
      skippedExisting: parsed.skipped_existing,
    };
  },

  async update(input: UpdateProjectSkillInput): Promise<ProjectSkill | null> {
    const raw = await postJson<unknown>("project_skills/update", {
      project_skill_id: input.projectSkillId,
      title: input.title,
      bucket: input.bucket,
      stage: input.stage,
      scope_paths: input.scopePaths,
      compact_guidance: input.compactGuidance,
      body_markdown: input.bodyMarkdown,
      predicted_effect: input.predictedEffect,
      ...(input.sourceSyncEnabled != null
        ? { source_sync_enabled: input.sourceSyncEnabled }
        : {}),
    });
    const parsed = ProjectSkillLifecycleResponseSchema.parse(raw);
    return parsed.skill ? transformProjectSkill(parsed.skill) : null;
  },

  async listReportCards(
    input: ListProjectSkillReportCardsInput,
  ): Promise<ProjectSkillReportCardsResult> {
    const raw = await postJson<unknown>("project_skills/report_cards", {
      project_id: input.projectId,
      min_linked_outcomes: input.minLinkedOutcomes ?? null,
      stale_after_days: input.staleAfterDays ?? null,
    });
    const parsed = ProjectSkillReportCardsResponseSchema.parse(raw);
    return {
      cards: parsed.cards.map(transformProjectSkillReportCard),
      count: parsed.count,
    };
  },

  async previewImport(
    input: PreviewProjectSkillImportInput,
  ): Promise<ProjectSkillImportPreviewResult> {
    const raw = await postJson<unknown>("project_skills/import/preview", {
      project_id: input.projectId,
      candidates: input.candidates.map((candidate) => ({
        external_id: candidate.externalId ?? null,
        title: candidate.title,
        bucket: candidate.bucket,
        stage: candidate.stage,
        scope_paths: candidate.scopePaths ?? [],
        compact_guidance: candidate.compactGuidance,
        body_markdown: candidate.bodyMarkdown,
        predicted_effect: candidate.predictedEffect,
        provenance_json: candidate.provenance,
        source_snapshot_json: candidate.sourceSnapshot,
      })),
    });
    const parsed = ProjectSkillImportPreviewResponseSchema.parse(raw);
    return transformProjectSkillImportPreview(parsed);
  },

  async applyImport(
    input: ApplyProjectSkillImportInput,
  ): Promise<ProjectSkillImportApplyResult> {
    const raw = await postJson<unknown>("project_skills/import/apply", {
      project_id: input.projectId,
      candidates: input.candidates.map((candidate) => ({
        external_id: candidate.externalId ?? null,
        title: candidate.title,
        bucket: candidate.bucket,
        stage: candidate.stage,
        scope_paths: candidate.scopePaths ?? [],
        compact_guidance: candidate.compactGuidance,
        body_markdown: candidate.bodyMarkdown,
        predicted_effect: candidate.predictedEffect,
        provenance_json: candidate.provenance,
        source_snapshot_json: candidate.sourceSnapshot,
      })),
      confirm_import: input.confirmImport,
    });
    const parsed = ProjectSkillImportApplyResponseSchema.parse(raw);
    return {
      preview: transformProjectSkillImportPreview(parsed.preview),
      importedSkills: parsed.imported_skills.map(transformProjectSkill),
      importedCount: parsed.imported_count,
      syncedCount: parsed.synced_count,
    };
  },

  async applyProjectDirectoryImport(
    input: ProjectSkillDirectoryImportInput | string,
  ): Promise<ProjectSkillImportApplyResult> {
    const request =
      typeof input === "string"
        ? { projectId: input, sourceRoots: undefined, sourceSyncEnabled: undefined }
        : input;
    const raw = await postJson<unknown>(
      "project_skills/import/project_directory/apply",
      {
        project_id: request.projectId,
        confirm_import: true,
        ...(request.sourceRoots ? { source_roots: request.sourceRoots } : {}),
        ...(request.sourceSyncEnabled != null
          ? { source_sync_enabled: request.sourceSyncEnabled }
          : {}),
      },
    );
    const parsed = ProjectSkillImportApplyResponseSchema.parse(raw);
    return {
      preview: transformProjectSkillImportPreview(parsed.preview),
      importedSkills: parsed.imported_skills.map(transformProjectSkill),
      importedCount: parsed.imported_count,
      syncedCount: parsed.synced_count,
    };
  },

  async promoteMemory(input: PromoteMemoryToProjectSkillInput): Promise<ProjectSkill> {
    const raw = await postJson<unknown>("project_skills/promote_memory", {
      project_id: input.projectId,
      memory_id: input.memoryId,
      title: input.title ?? null,
      bucket: input.bucket,
      stage: input.stage,
      compact_guidance: input.compactGuidance,
      body_markdown: input.bodyMarkdown,
      predicted_effect: input.predictedEffect,
    });
    const parsed = PromoteMemoryToProjectSkillResponseSchema.parse(raw);
    return transformProjectSkill(parsed.skill);
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
    settings: ProjectSkillSettingsPatch,
  ): Promise<ProjectSkillSettings> {
    const raw = await postJson<unknown>("project_skills/settings/update", {
      project_id: projectId,
      ...(settings.enabled !== undefined ? { enabled: settings.enabled } : {}),
      ...(settings.autoInject !== undefined
        ? { auto_inject: settings.autoInject }
        : {}),
      ...(settings.autoDistill !== undefined
        ? { auto_distill: settings.autoDistill }
        : {}),
      ...(settings.injectionMaxSkills !== undefined
        ? { injection_max_skills: settings.injectionMaxSkills }
        : {}),
      ...(settings.injectionMaxChars !== undefined
        ? { injection_max_chars: settings.injectionMaxChars }
        : {}),
      ...(settings.injectionGuidanceMaxChars !== undefined
        ? { injection_guidance_max_chars: settings.injectionGuidanceMaxChars }
        : {}),
      ...(settings.reportMinOutcomes !== undefined
        ? { report_min_outcomes: settings.reportMinOutcomes }
        : {}),
      ...(settings.verificationCorpusGate !== undefined
        ? { verification_corpus_gate: settings.verificationCorpusGate }
        : {}),
      ...(settings.exportEnabled !== undefined
        ? { export_enabled: settings.exportEnabled }
        : {}),
    });
    return transformProjectSkillSettings(
      ProjectSkillSettingsResponseSchema.parse(raw),
    );
  },
} as const;
