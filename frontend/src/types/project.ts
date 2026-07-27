// Project types and Zod schema
// Must match the Rust backend Project struct

import { z } from "zod";

/**
 * Git mode for project repository
 */
export const GitModeSchema = z.enum(["worktree"]);
export type GitMode = z.infer<typeof GitModeSchema>;

/**
 * Merge validation mode for post-merge build checks
 */
export const MergeValidationModeSchema = z.enum(["block", "auto_fix", "warn", "off"]);
export type MergeValidationMode = z.infer<typeof MergeValidationModeSchema>;

/** Live transport capability for future GitHub pull-request workflows. */
export const RepositoryCapabilityResponseSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("local_only") }),
  z.object({
    kind: z.literal("github"),
    fetch_url: z.string().nullable(),
    push_url: z.string(),
  }),
  z.object({
    kind: z.literal("other_remote"),
    fetch_url: z.string().nullable(),
    push_url: z.string(),
  }),
  z.object({ kind: z.literal("inspection_failed"), message: z.string() }),
]);

export type RepositoryCapability =
  | { kind: "localOnly" }
  | { kind: "github"; fetchUrl: string | null; pushUrl: string }
  | { kind: "otherRemote"; fetchUrl: string | null; pushUrl: string }
  | { kind: "inspectionFailed"; message: string };

export type ProjectWorkspacePublishMode =
  | { kind: "persistedPr" }
  | { kind: "newPr" }
  | { kind: "localCommit"; guidance: string }
  | { kind: "unavailable"; guidance: string };

const UNKNOWN_REPOSITORY_CAPABILITY_MESSAGE =
  "Repository capability is unavailable. Refresh the project before starting a PR workflow.";
const UNKNOWN_REPOSITORY_CAPABILITY: RepositoryCapability = {
  kind: "inspectionFailed",
  message: UNKNOWN_REPOSITORY_CAPABILITY_MESSAGE,
};

export function transformRepositoryCapability(
  capability: z.infer<typeof RepositoryCapabilityResponseSchema>
): RepositoryCapability {
  switch (capability.kind) {
    case "local_only":
      return { kind: "localOnly" };
    case "github":
      return {
        kind: "github",
        fetchUrl: capability.fetch_url,
        pushUrl: capability.push_url,
      };
    case "other_remote":
      return {
        kind: "otherRemote",
        fetchUrl: capability.fetch_url,
        pushUrl: capability.push_url,
      };
    case "inspection_failed":
      return { kind: "inspectionFailed", message: capability.message };
  }
}

export function isGithubRepositoryCapability(
  capability: RepositoryCapability
): boolean {
  return capability.kind === "github";
}

/**
 * Backend response schema - expects snake_case from Rust serialization
 */
export const ProjectResponseSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  working_directory: z.string().min(1),
  git_mode: GitModeSchema,
  base_branch: z.string().nullable(),
  worktree_parent_directory: z.string().nullish(),
  use_feature_branches: z.boolean().default(true),
  merge_validation_mode: MergeValidationModeSchema.default("block"),
  detected_analysis: z.string().nullish(),
  custom_analysis: z.string().nullish(),
  analyzed_at: z.string().nullish(),
  github_pr_enabled: z.boolean().default(false),
  repository_capability: RepositoryCapabilityResponseSchema.optional(),
  // Accept RFC3339 timestamps with offset (e.g., +00:00) not just Z
  created_at: z.string().datetime({ offset: true }),
  updated_at: z.string().datetime({ offset: true }),
});

/**
 * Frontend Project interface - uses camelCase
 */
export interface Project {
  id: string;
  name: string;
  workingDirectory: string;
  gitMode: GitMode;
  baseBranch: string | null;
  worktreeParentDirectory: string | null;
  useFeatureBranches: boolean;
  mergeValidationMode: MergeValidationMode;
  detectedAnalysis: string | null;
  customAnalysis: string | null;
  analyzedAt: string | null;
  githubPrEnabled: boolean;
  repositoryCapability?: RepositoryCapability;
  createdAt: string;
  updatedAt: string;
}

export function getProjectRepositoryCapability(
  project: Pick<Project, "repositoryCapability"> | null | undefined,
): RepositoryCapability {
  return project?.repositoryCapability ?? UNKNOWN_REPOSITORY_CAPABILITY;
}

export function getProjectWorkspacePublishMode(
  project: Pick<Project, "githubPrEnabled" | "repositoryCapability"> | null | undefined,
  hasPersistedPr: boolean,
): ProjectWorkspacePublishMode {
  if (hasPersistedPr) {
    return { kind: "persistedPr" };
  }

  const capability = getProjectRepositoryCapability(project);
  if (capability.kind === "github" && project?.githubPrEnabled === true) {
    return { kind: "newPr" };
  }
  if (capability.kind === "localOnly" || capability.kind === "otherRemote") {
    return {
      kind: "localCommit",
      guidance: "This repository cannot open GitHub pull requests. Commit changes to the isolated workspace branch without pushing or merging.",
    };
  }
  if (capability.kind === "github") {
    return {
      kind: "localCommit",
      guidance: "GitHub PR mode is off for this project. Commit locally, or enable GitHub PR mode in project settings to open a pull request.",
    };
  }
  return {
    kind: "unavailable",
    guidance: "RalphX could not inspect this repository's remote configuration. Refresh the project before starting a PR workflow.",
  };
}

/**
 * Transform snake_case backend response to camelCase frontend type
 */
export function transformProject(
  response: z.infer<typeof ProjectResponseSchema>
): Project {
  return {
    id: response.id,
    name: response.name,
    workingDirectory: response.working_directory,
    gitMode: response.git_mode,
    baseBranch: response.base_branch,
    worktreeParentDirectory: response.worktree_parent_directory ?? null,
    useFeatureBranches: response.use_feature_branches,
    mergeValidationMode: response.merge_validation_mode,
    detectedAnalysis: response.detected_analysis ?? null,
    customAnalysis: response.custom_analysis ?? null,
    analyzedAt: response.analyzed_at ?? null,
    githubPrEnabled: response.github_pr_enabled,
    repositoryCapability: transformRepositoryCapability(
      response.repository_capability ?? {
        kind: "inspection_failed",
        message: UNKNOWN_REPOSITORY_CAPABILITY_MESSAGE,
      },
    ),
    createdAt: response.created_at,
    updatedAt: response.updated_at,
  };
}

// Legacy export for backward compatibility - use ProjectResponseSchema instead
export const ProjectSchema = ProjectResponseSchema;

/**
 * Schema for creating a new project
 * Excludes auto-generated fields (id, timestamps)
 */
export const CreateProjectSchema = z.object({
  name: z.string().min(1, "Project name is required"),
  workingDirectory: z.string().min(1, "Working directory is required"),
  gitMode: GitModeSchema.default("worktree"),
  baseBranch: z.string().optional(),
  worktreeParentDirectory: z.string().optional(),
});

export type CreateProject = z.infer<typeof CreateProjectSchema>;

/**
 * Schema for updating a project
 * All fields are optional
 */
export const UpdateProjectSchema = z.object({
  name: z.string().min(1).optional(),
  workingDirectory: z.string().min(1).optional(),
  gitMode: GitModeSchema.optional(),
  baseBranch: z.string().nullable().optional(),
  worktreeParentDirectory: z.string().nullable().optional(),
  mergeValidationMode: MergeValidationModeSchema.optional(),
});

export type UpdateProject = z.infer<typeof UpdateProjectSchema>;
