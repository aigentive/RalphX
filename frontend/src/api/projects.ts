// Projects and Workflows API module
// Extracted from src/lib/tauri.ts following the domain API pattern

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import {
  ProjectResponseSchema,
  transformProject,
  type CreateProject,
  type UpdateProject,
  type Project,
} from "@/types/project";
import {
  WorkflowResponseSchema,
  WorkflowColumnResponseSchema,
  transformWorkflow,
  transformWorkflowColumn,
  type WorkflowSchema,
  type WorkflowColumn,
} from "@/types/workflow";
import { TauriVoidSchema, typedInvoke, typedInvokeWithTransform } from "@/lib/tauri";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";
import {
  CreateWorkflowInputSchema,
  UpdateWorkflowInputSchema,
  type CreateWorkflowInput,
  type UpdateWorkflowInput,
} from "@/lib/api/workflows";

/**
 * True while the active environment is remote, so the shell's boot reads must use the
 * spawn-free twins rather than their Elevated/Denied local counterparts.
 */
function remoteShellReadsEnabled(): boolean {
  return isRemoteEnvironmentId(getTransportEnvironmentId());
}

/** Two scalars — deliberately not the provider settings surface. */
export const RemoteProviderReadinessSchema = z.object({
  onboardingComplete: z.boolean(),
  enabledProviderCount: z.number(),
});

export type RemoteProviderReadiness = z.infer<typeof RemoteProviderReadinessSchema>;

/**
 * Project list schema for array responses (snake_case from backend)
 */
const ProjectListResponseSchema = z.array(ProjectResponseSchema);
const PrTemplateResponseSchema = z.string().nullable();

/**
 * Transform project list from snake_case to camelCase
 */
function transformProjectList(
  response: z.infer<typeof ProjectListResponseSchema>
): Project[] {
  return response.map(transformProject);
}

/**
 * Workflow list schema for array responses
 */
const WorkflowListResponseSchema = z.array(WorkflowResponseSchema);

/**
 * Workflow column list schema for array responses
 */
const WorkflowColumnListResponseSchema = z.array(WorkflowColumnResponseSchema);

/**
 * Get git branches for a working directory
 * @param workingDirectory The path to the git repository
 * @returns Array of branch names (main/master sorted first)
 */
export async function getGitBranches(workingDirectory: string): Promise<string[]> {
  const result = await invoke<string[]>("get_git_branches", { workingDirectory });
  return result;
}

/**
 * Get the default branch for a git repository
 * Uses fallback chain: origin/HEAD -> main -> master -> first branch
 * @param workingDirectory The path to the git repository
 * @returns The default branch name
 */
export async function getGitDefaultBranch(workingDirectory: string): Promise<string> {
  const result = await invoke<string>("get_git_default_branch", { workingDirectory });
  return result;
}

/**
 * Get the current local branch for a git repository.
 * @param workingDirectory The path to the git repository
 * @returns The current local branch name
 */
export async function getGitCurrentBranch(workingDirectory: string): Promise<string> {
  const result = await invoke<string>("get_git_current_branch", { workingDirectory });
  return result;
}

const GithubPullRequestSearchResultSchema = z.object({
  number: z.number(),
  title: z.string(),
  url: z.string(),
  headRefName: z.string(),
  headRefOid: z.string().nullable().optional(),
  baseRefName: z.string(),
  isDraft: z.boolean(),
  updatedAt: z.string().nullable().optional(),
  authorLogin: z.string().nullable().optional(),
  assigneeLogins: z.array(z.string()).default([]),
  reviewDecision: z.string().nullable().optional(),
  latestReviewAuthorLogins: z.array(z.string()).default([]),
  reviewRequestLogins: z.array(z.string()).default([]),
  isCrossRepository: z.boolean(),
});

export type GithubPullRequestSearchResult = z.infer<
  typeof GithubPullRequestSearchResultSchema
>;

export interface SearchGithubPullRequestsInput {
  projectId: string;
  query?: string;
  limit?: number;
}

export async function searchGithubPullRequests(
  input: SearchGithubPullRequestsInput
): Promise<GithubPullRequestSearchResult[]> {
  const result = await invoke<unknown>("search_github_pull_requests", { input });
  return z.array(GithubPullRequestSearchResultSchema).parse(result);
}

/**
 * Projects API object containing all typed Tauri command wrappers for projects
 */
export const projectsApi = {
  /**
   * List all projects.
   *
   * Under a remote environment this must call the spawn-free twin: `list_projects` runs
   * `inspect_repository_capability` per project, which is why it is ledgered Elevated and
   * left unregistered on the facade. Calling it remotely answers REMOTE_COMMAND_UNAVAILABLE,
   * which the shell reads as an empty workspace and renders as first-run onboarding.
   *
   * Both answers parse with the SAME schema and transform — the host's projection carries
   * snake_case field names identical to `ProjectResponse` and differs only by dropping
   * `repository_capability` (which the schema already marks optional).
   *
   * @returns Array of projects
   */
  list: (): Promise<Project[]> =>
    // Two literal call sites, not a computed name: the P-11 drift scan requires every
    // production command name to be a literal so the census can classify it.
    remoteShellReadsEnabled()
      ? typedInvokeWithTransform(
          "list_remote_projects",
          {},
          ProjectListResponseSchema,
          transformProjectList
        )
      : typedInvokeWithTransform(
          "list_projects",
          {},
          ProjectListResponseSchema,
          transformProjectList
        ),

  /**
   * Whether the ACTIVE environment's host has a usable provider.
   *
   * Remote-only by construction: locally the shell reads the full provider settings, which
   * are `Denied` on the facade (CLI probes, provider identities, credential surface). The
   * onboarding gate only ever asked a boolean, so the remote answer is a boolean.
   */
  remoteProviderReadiness: () =>
    typedInvoke(
      "get_remote_provider_readiness",
      {},
      RemoteProviderReadinessSchema
    ),

  /**
   * Get a single project by ID
   * @param projectId The project ID
   * @returns The project
   */
  get: (projectId: string) =>
    typedInvokeWithTransform(
      "get_project",
      { projectId },
      ProjectResponseSchema,
      transformProject
    ),

  /**
   * Create a new project
   * @param input Project creation data
   * @returns The created project
   */
  create: (input: CreateProject) =>
    typedInvokeWithTransform(
      "create_project",
      { input },
      ProjectResponseSchema,
      transformProject
    ),

  /**
   * Update an existing project
   * @param projectId The project ID
   * @param input Partial project data to update
   * @returns The updated project
   */
  update: (projectId: string, input: UpdateProject) =>
    typedInvokeWithTransform(
      "update_project",
      { id: projectId, input },
      ProjectResponseSchema,
      transformProject
    ),

  /**
   * Archive a project
   * @param projectId The project ID
   * @returns The archived project
   */
  archive: (projectId: string) =>
    typedInvokeWithTransform(
      "archive_project",
      { projectId },
      ProjectResponseSchema,
      transformProject
    ),

  /**
   * Read the project's fixed pull request template file.
   * @param projectId The project ID
   * @returns Exact file content, or null when the template is absent
   */
  readPrTemplate: (projectId: string) =>
    typedInvoke("read_pr_template", { projectId }, PrTemplateResponseSchema),

  /**
   * Write exact content to the project's fixed pull request template file.
   * @param projectId The project ID
   * @param content Exact template content
   */
  writePrTemplate: (projectId: string, content: string) =>
    typedInvoke("write_pr_template", { projectId, content }, TauriVoidSchema),

  /**
   * Update custom analysis override for a project
   * @param projectId The project ID
   * @param customAnalysis JSON string of analysis entries, or null to clear
   * @returns The updated project
   */
  updateCustomAnalysis: (projectId: string, customAnalysis: string | null) =>
    typedInvokeWithTransform(
      "update_custom_analysis",
      { id: projectId, customAnalysis },
      ProjectResponseSchema,
      transformProject
    ),

  /**
   * Re-analyze project build systems and validation commands
   * Triggers the ralphx-project-analyzer agent
   * @param projectId The project ID
   */
  reanalyzeProject: (projectId: string) =>
    invoke("reanalyze_project", { id: projectId }),
} as const;

/**
 * Workflows API object containing all typed Tauri command wrappers for workflows
 */
export const workflowsApi = {
  /**
   * Get a workflow by ID
   * @param workflowId The workflow ID
   * @returns The workflow or null if not found
   */
  get: async (workflowId: string): Promise<WorkflowSchema | null> => {
    const raw = await typedInvoke(
      "get_workflow",
      { id: workflowId },
      WorkflowResponseSchema.nullable()
    );
    return raw ? transformWorkflow(raw) : null;
  },

  /**
   * List all workflows
   * @returns Array of workflows
   */
  list: (): Promise<WorkflowSchema[]> =>
    typedInvokeWithTransform(
      "get_workflows",
      {},
      WorkflowListResponseSchema,
      (workflows) => workflows.map(transformWorkflow)
    ),

  /**
   * Get columns for the active/default workflow
   * @returns Array of workflow columns
   */
  getActiveColumns: (): Promise<WorkflowColumn[]> =>
    typedInvokeWithTransform(
      "get_active_workflow_columns",
      {},
      WorkflowColumnListResponseSchema,
      (columns) => columns.map(transformWorkflowColumn)
    ),

  /**
   * Create a new workflow
   * @param input Workflow creation data
   * @returns The created workflow
   */
  create: async (input: CreateWorkflowInput): Promise<WorkflowSchema> => {
    const validatedInput = CreateWorkflowInputSchema.parse(input);
    return typedInvokeWithTransform(
      "create_workflow",
      { input: validatedInput },
      WorkflowResponseSchema,
      transformWorkflow
    );
  },

  /**
   * Update an existing workflow
   * @param id The workflow ID
   * @param input Partial workflow data to update
   * @returns The updated workflow
   */
  update: async (id: string, input: UpdateWorkflowInput): Promise<WorkflowSchema> => {
    const validatedInput = UpdateWorkflowInputSchema.parse(input);
    return typedInvokeWithTransform(
      "update_workflow",
      { id, input: validatedInput },
      WorkflowResponseSchema,
      transformWorkflow
    );
  },

  /**
   * Set a workflow as the default
   * @param id The workflow ID to set as default
   * @returns The updated workflow
   */
  setDefault: (id: string): Promise<WorkflowSchema> =>
    typedInvokeWithTransform(
      "set_default_workflow",
      { id },
      WorkflowResponseSchema,
      transformWorkflow
    ),

  /**
   * Seed builtin workflows if they don't exist
   * @returns Number of workflows created
   */
  seedBuiltin: () => typedInvoke("seed_builtin_workflows", {}, z.number()),

  /**
   * Get the built-in workflow definitions (RalphX Default, Jira Compatible)
   * @returns Array of built-in workflows
   */
  getBuiltin: (): Promise<WorkflowSchema[]> =>
    typedInvokeWithTransform(
      "get_builtin_workflows",
      {},
      WorkflowListResponseSchema,
      (workflows) => workflows.map(transformWorkflow)
    ),
} as const;
