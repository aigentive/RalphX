// Project Settings types
// Global project configuration with Zod schemas

import { z } from "zod";

// ============================================================================
// Execution Settings
// ============================================================================

export const ExecutionSettingsSchema = z.object({
  /** Maximum concurrent tasks to run */
  max_concurrent_tasks: z.number().int().min(1).max(10).default(10),
  /** Maximum concurrent ideation or verification sessions for this project */
  project_ideation_max: z.number().int().min(0).max(10).default(5),
  /** Auto-commit changes after task completion — fixed product behavior, not user-configurable */
  auto_commit: z.boolean().default(true),
  /** Commit message prefix (conventional commits) */
  commit_message_prefix: z.string().default("feat: "),
  /** Pause queue when a task fails — persisted but no runtime enforcement */
  pause_on_failure: z.boolean().default(true),
  /** Default Autofix CI & Reviews setting for new agent conversations */
  agent_workspace_pr_autofix_default: z.boolean().default(false),
  /** Default GitHub auto-merge setting for new agent conversations */
  agent_workspace_pr_auto_merge_default: z.boolean().default(false),
});

export type ExecutionSettings = z.infer<typeof ExecutionSettingsSchema>;

export const DEFAULT_EXECUTION_SETTINGS: ExecutionSettings = {
  max_concurrent_tasks: 10,
  project_ideation_max: 5,
  auto_commit: true,
  commit_message_prefix: "feat: ",
  pause_on_failure: true,
  agent_workspace_pr_autofix_default: false,
  agent_workspace_pr_auto_merge_default: false,
};

// ============================================================================
// Project Review Settings (snake_case to match plan spec)
// ============================================================================

export const ProjectReviewSettingsSchema = z.object({
  /** Enable AI review of completed tasks */
  ai_review_enabled: z.boolean().default(true),
  /** Auto-create fix tasks for review failures */
  ai_review_auto_fix: z.boolean().default(true),
  /** Require approval before executing fix tasks */
  require_fix_approval: z.boolean().default(false),
  /** Require human review even after AI approval */
  require_human_review: z.boolean().default(false),
  /** Require workspace Review before publishing agent workspace branches */
  require_workspace_review: z.boolean().default(true),
  /** Spawn workspace repair agent for blocking Workspace Review findings */
  autofix_workspace_review_blocking_findings: z.boolean().default(true),
  /** Maximum fix attempts before moving to backlog */
  max_fix_attempts: z.number().int().min(1).max(10).default(3),
});

export type ProjectReviewSettings = z.infer<typeof ProjectReviewSettingsSchema>;

export const DEFAULT_PROJECT_REVIEW_SETTINGS: ProjectReviewSettings = {
  ai_review_enabled: true,
  ai_review_auto_fix: true,
  require_fix_approval: false,
  require_human_review: false,
  require_workspace_review: true,
  autofix_workspace_review_blocking_findings: true,
  max_fix_attempts: 3,
};

// ============================================================================
// Combined Project Settings
// ============================================================================

export const ProjectSettingsSchema = z.object({
  execution: ExecutionSettingsSchema.default(DEFAULT_EXECUTION_SETTINGS),
  review: ProjectReviewSettingsSchema.default(DEFAULT_PROJECT_REVIEW_SETTINGS),
});

export type ProjectSettings = z.infer<typeof ProjectSettingsSchema>;

export const DEFAULT_PROJECT_SETTINGS: ProjectSettings = {
  execution: DEFAULT_EXECUTION_SETTINGS,
  review: DEFAULT_PROJECT_REVIEW_SETTINGS,
};

// ============================================================================
// Settings Profile
// ============================================================================

export const SettingsProfileSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  description: z.string().optional(),
  settings: ProjectSettingsSchema,
  isDefault: z.boolean().default(false),
  createdAt: z.string().datetime(),
  updatedAt: z.string().datetime(),
});

export type SettingsProfile = z.infer<typeof SettingsProfileSchema>;

// ============================================================================
// Parsing Utilities
// ============================================================================

export function parseProjectSettings(data: unknown): ProjectSettings {
  return ProjectSettingsSchema.parse(data);
}

export function safeParseProjectSettings(
  data: unknown,
): ProjectSettings | null {
  const result = ProjectSettingsSchema.safeParse(data);
  return result.success ? result.data : null;
}

export function parseSettingsProfile(data: unknown): SettingsProfile {
  return SettingsProfileSchema.parse(data);
}

export function safeParseSettingsProfile(
  data: unknown,
): SettingsProfile | null {
  const result = SettingsProfileSchema.safeParse(data);
  return result.success ? result.data : null;
}
