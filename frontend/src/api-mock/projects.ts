/**
 * Mock Projects API
 *
 * Mirrors the interface of src/api/projects.ts with mock implementations.
 */

import type { Project, CreateProject, UpdateProject } from "@/types/project";
import type { WorkflowSchema, WorkflowColumn } from "@/types/workflow";
import type { InternalStatus } from "@/types/status";
import type { CreateWorkflowInput, UpdateWorkflowInput } from "@/lib/api/workflows";
import type {
  PrepareNewProjectDirectoryInput,
  PreparedProjectDirectory,
} from "@/api/projects";
import {
  ProjectCandidateResponseSchema,
  transformProjectCandidate,
  type ProjectCandidate,
} from "@/types/project-candidate";
import { createMockProject, generateTestUuid } from "@/test/mock-data";
import { getStore } from "./store";

// ============================================================================
// Mock Projects API
// ============================================================================

const mockPrTemplates = new Map<string, string | null>();

export const mockProjectsApi = {
  list: async (): Promise<Project[]> => {
    const store = getStore();
    return Array.from(store.projects.values());
  },

  get: async (projectId: string): Promise<Project> => {
    const store = getStore();
    const project = store.projects.get(projectId);
    if (!project) {
      throw new Error(`Project not found: ${projectId}`);
    }
    return project;
  },

  create: async (input: CreateProject): Promise<Project> => {
    const project = createMockProject({
      id: generateTestUuid(),
      name: input.name,
      workingDirectory: input.workingDirectory,
      gitMode: input.gitMode ?? "worktree",
      baseBranch: input.baseBranch ?? null,
      worktreeParentDirectory: input.worktreeParentDirectory ?? null,
    });
    getStore().projects.set(project.id, project);
    return project;
  },

  update: async (projectId: string, input: UpdateProject): Promise<Project> => {
    const store = getStore();
    const existing = store.projects.get(projectId);
    if (!existing) {
      throw new Error(`Project not found: ${projectId}`);
    }
    // Merge only the provided fields
    const updated: Project = {
      ...existing,
      updatedAt: new Date().toISOString(),
    };
    if (input.name !== undefined) updated.name = input.name;
    if (input.workingDirectory !== undefined) updated.workingDirectory = input.workingDirectory;
    if (input.gitMode !== undefined) updated.gitMode = input.gitMode;
    if (input.baseBranch !== undefined) updated.baseBranch = input.baseBranch;
    if (input.worktreeParentDirectory !== undefined) updated.worktreeParentDirectory = input.worktreeParentDirectory;
    if (input.mergeValidationMode !== undefined) updated.mergeValidationMode = input.mergeValidationMode;
    store.projects.set(projectId, updated);
    return updated;
  },

  delete: async (_projectId: string): Promise<boolean> => {
    return true;
  },

  readPrTemplate: async (projectId: string): Promise<string | null> => {
    return mockPrTemplates.get(projectId) ?? null;
  },

  writePrTemplate: async (projectId: string, content: string): Promise<null> => {
    mockPrTemplates.set(projectId, content);
    return null;
  },

  updateCustomAnalysis: async (projectId: string, customAnalysis: string | null): Promise<Project> => {
    const store = getStore();
    const project = store.projects.get(projectId);
    if (!project) {
      throw new Error(`Project not found: ${projectId}`);
    }
    project.customAnalysis = customAnalysis;
    project.updatedAt = new Date().toISOString();
    store.projects.set(projectId, project);
    return project;
  },

  reanalyzeProject: async (_projectId: string): Promise<void> => {
    // No-op in mock — analyzer agent would run in real mode
  },

  /**
   * Project-creation probes. App.tsx hands these to ProjectCreationWizard as
   * `api.projects.*`, and web mode swaps the whole `api` object for `mockApi`
   * (src/lib/tauri.ts) — so these never reach the invoke-level mocks in
   * src/mocks/tauri-api-core.ts and must be mirrored here too. They return the
   * camelCase frontend types, because the real API's Zod transform is also
   * bypassed on this path.
   */
  inspectProjectCandidate: async (path: string): Promise<ProjectCandidate> =>
    transformProjectCandidate(
      ProjectCandidateResponseSchema.parse(await mockInspectProjectCandidate(path))
    ),

  prepareNewProjectDirectory: (
    input: PrepareNewProjectDirectoryInput
  ): Promise<PreparedProjectDirectory> => mockPrepareNewProjectDirectory(input),

  discardPreparedProjectDirectory: (path: string): Promise<void> =>
    mockDiscardPreparedProjectDirectory(path),
} as const;

// ============================================================================
// Mock Workflows API
// ============================================================================

const mockWorkflowColumns: WorkflowSchema["columns"] = [
  {
    id: "draft",
    name: "Draft",
    mapsTo: "backlog" as InternalStatus,
  },
  {
    id: "ready",
    name: "Ready",
    mapsTo: "ready" as InternalStatus,
  },
  {
    id: "in_progress",
    name: "In Progress",
    mapsTo: "executing" as InternalStatus,
  },
  {
    id: "in_review",
    name: "In Review",
    mapsTo: "pending_review" as InternalStatus,
  },
  {
    id: "done",
    name: "Done",
    mapsTo: "approved" as InternalStatus,
  },
];

const mockWorkflows: WorkflowSchema[] = [
  {
    id: "workflow-default",
    name: "Default Workflow",
    columns: mockWorkflowColumns,
    isDefault: true,
  },
];

export const mockWorkflowsApi = {
  /**
   * Get a workflow by ID
   */
  get: async (workflowId: string): Promise<WorkflowSchema | null> => {
    const workflow = mockWorkflows.find((w) => w.id === workflowId);
    return workflow ?? null;
  },

  /**
   * List all workflows
   */
  list: async (): Promise<WorkflowSchema[]> => {
    return mockWorkflows;
  },

  /**
   * Get columns for the active/default workflow
   */
  getActiveColumns: async (): Promise<WorkflowColumn[]> => {
    const defaultWorkflow = mockWorkflows.find((w) => w.isDefault);
    return defaultWorkflow?.columns ?? mockWorkflowColumns;
  },

  /**
   * Create a new workflow (no-op in mock, returns fake workflow)
   */
  create: async (input: CreateWorkflowInput): Promise<WorkflowSchema> => {
    return {
      id: `mock-workflow-${Date.now()}`,
      name: input.name,
      description: input.description,
      columns: input.columns.map((col) => ({
        id: col.id,
        name: col.name,
        mapsTo: col.maps_to as InternalStatus,
        color: col.color,
        icon: col.icon,
      })),
      isDefault: input.is_default ?? false,
    };
  },

  /**
   * Update an existing workflow (no-op in mock, returns updated data)
   */
  update: async (id: string, input: UpdateWorkflowInput): Promise<WorkflowSchema> => {
    const existing = mockWorkflows.find((w) => w.id === id);
    if (!existing) {
      throw new Error(`Workflow not found: ${id}`);
    }
    return {
      ...existing,
      ...(input.name && { name: input.name }),
      ...(input.description !== undefined && { description: input.description }),
      ...(input.is_default !== undefined && { isDefault: input.is_default }),
    };
  },

  /**
   * Delete a workflow (no-op in mock)
   */
  delete: async (_id: string): Promise<void> => {
    // No-op for mock
  },

  /**
   * Set a workflow as the default (no-op in mock, returns updated workflow)
   */
  setDefault: async (id: string): Promise<WorkflowSchema> => {
    const workflow = mockWorkflows.find((w) => w.id === id);
    if (!workflow) {
      throw new Error(`Workflow not found: ${id}`);
    }
    return { ...workflow, isDefault: true };
  },

  /**
   * Seed builtin workflows (no-op in mock)
   */
  seedBuiltin: async (): Promise<number> => {
    return 1;
  },

  /**
   * Get the built-in workflow definitions
   */
  getBuiltin: async (): Promise<WorkflowSchema[]> => {
    return mockWorkflows;
  },
} as const;

// ============================================================================
// Mock Git Branches
// ============================================================================

export async function mockGetGitBranches(_workingDirectory: string): Promise<string[]> {
  return ["main", "develop", "feature/mock-branch"];
}

export async function mockGetGitDefaultBranch(_workingDirectory: string): Promise<string> {
  // Return "main" as the default branch for mock purposes
  return "main";
}

export async function mockGetGitCurrentBranch(_workingDirectory: string): Promise<string> {
  return "main";
}

// ============================================================================
// Mock Project Candidate Probe (snake_case, mirrors the backend wire shape)
// ============================================================================

export async function mockInspectProjectCandidate(path: string): Promise<unknown> {
  if (!path) {
    return { kind: "not_found" };
  }
  const store = getStore();
  const existing = Array.from(store.projects.values()).find(
    (project) => project.workingDirectory === path
  );
  return {
    kind: "repository",
    repository_root: path,
    current_branch: "main",
    default_branch: "main",
    branches: ["main", "develop"],
    has_commits: true,
    is_dirty: false,
    capability: { kind: "local_only" },
    already_registered_as: existing ? { id: existing.id, name: existing.name } : null,
  };
}

export interface MockPrepareNewProjectDirectoryInput {
  parentDirectory: string;
  folderName: string;
}

export async function mockPrepareNewProjectDirectory(
  input: MockPrepareNewProjectDirectoryInput
): Promise<{ path: string; created: boolean }> {
  return { path: `${input.parentDirectory}/${input.folderName}`, created: true };
}

export async function mockDiscardPreparedProjectDirectory(_path: string): Promise<void> {
  // No-op in mock — nothing was actually created on disk.
}

// ============================================================================
// Mock Clone (validate/start/cancel/status) — snake_case only where the
// backend tags a field internally (RepositoryCapability.kind, CloneJobStatus
// terminal `state`); everything else mirrors the fully camelCase wire shape.
// ============================================================================

export interface MockCloneTargetPlan {
  normalizedUrl: string | null;
  folderName: string | null;
  branch: string | null;
  suggestedSshUrl: string | null;
  destination: string | null;
  ready: boolean;
  problem: string | null;
}

export interface MockValidateCloneTargetInput {
  url: string;
  parentDirectory?: string;
  folderName?: string;
}

function deriveCloneFolderName(url: string): string | null {
  const lastSegment = url.split(/[/:]/).filter(Boolean).pop();
  if (!lastSegment) {
    return null;
  }
  return lastSegment.endsWith(".git") ? lastSegment.slice(0, -4) : lastSegment;
}

function deriveSuggestedSshUrl(url: string): string | null {
  const match = /^https:\/\/github\.com\/([^/]+)\/([^/]+?)(?:\.git)?$/i.exec(url);
  if (!match) {
    return null;
  }
  const [, owner, repo] = match;
  return `git@github.com:${owner}/${repo}.git`;
}

/**
 * Believable, deterministic clone-target validation for web mode: a
 * plausible URL is ready with a derived folder; an obviously bad one
 * (no path segments, or a local path) reports a plain-language problem.
 */
export function mockValidateCloneTarget(
  input: MockValidateCloneTargetInput
): MockCloneTargetPlan {
  const notReady = (problem: string): MockCloneTargetPlan => ({
    normalizedUrl: null,
    folderName: null,
    branch: null,
    suggestedSshUrl: null,
    destination: null,
    ready: false,
    problem,
  });

  const trimmed = input.url.trim();
  if (!trimmed) {
    return notReady("Enter a repository address to clone.");
  }

  const looksLocal =
    trimmed.startsWith("file://") ||
    trimmed.startsWith("/") ||
    trimmed.startsWith("./") ||
    trimmed.startsWith("../") ||
    trimmed.startsWith("~");
  const isHttps = /^https:\/\//i.test(trimmed);
  const isSsh = /^ssh:\/\//i.test(trimmed) || /^[\w.-]+@[\w.-]+:.+/.test(trimmed);
  const isShorthand =
    !looksLocal && !isHttps && !isSsh && /^[\w.-]+\/[\w.-]+$/.test(trimmed);

  if (looksLocal || (!isHttps && !isSsh && !isShorthand)) {
    return notReady(
      "This does not look like a repository address RalphX can clone. Use a repository URL, such as https://github.com/owner/repo."
    );
  }

  const treeMatch = /\/tree\/([^/]+)\/?$/.exec(trimmed);
  const cleanedUrl = treeMatch ? trimmed.slice(0, treeMatch.index) : trimmed;
  const branch = treeMatch?.[1] ?? null;
  const normalizedUrl = isShorthand ? `https://github.com/${cleanedUrl}.git` : cleanedUrl;
  const folderName = input.folderName?.trim() || deriveCloneFolderName(normalizedUrl);
  const suggestedSshUrl = deriveSuggestedSshUrl(normalizedUrl);
  const parentDirectory = input.parentDirectory?.trim();
  const destination =
    parentDirectory && folderName ? `${parentDirectory}/${folderName}` : null;

  if (!folderName) {
    return {
      normalizedUrl,
      folderName: null,
      branch,
      suggestedSshUrl,
      destination: null,
      ready: false,
      problem: "RalphX could not work out a folder name for this repository. Add one.",
    };
  }

  return {
    normalizedUrl,
    folderName,
    branch,
    suggestedSshUrl,
    destination,
    ready: true,
    problem: null,
  };
}

export interface MockStartProjectCloneInput {
  url: string;
  parentDirectory: string;
  folderName?: string;
  branch?: string;
}

type MockCloneJobOutcome = "success" | "auth_failed" | "unknown";

interface MockCloneJob {
  destination: string;
  defaultBranch: string | null;
  outcome: MockCloneJobOutcome;
  callCount: number;
  cancelled: boolean;
}

const MOCK_CLONE_PHASES = [
  "connecting",
  "counting",
  "compressing",
  "receiving",
  "resolving",
  "checking_out",
] as const;

const mockCloneJobs = new Map<string, MockCloneJob>();

/**
 * Forces the outcome of the next clone job for Playwright coverage, mirroring
 * the existing `window.__mockGhAuthStatus` fixture-switch pattern: set
 * `window.__mockCloneJobOutcome = "auth_failed"` before starting a clone to
 * exercise the auth-failure card, or `"unknown"` to exercise the
 * lost-track-of-this-clone path.
 */
function getMockCloneJobOutcome(): MockCloneJobOutcome {
  if (typeof window === "undefined") {
    return "success";
  }
  return (
    (window as Window & { __mockCloneJobOutcome?: MockCloneJobOutcome })
      .__mockCloneJobOutcome ?? "success"
  );
}

export function mockStartProjectClone(
  input: MockStartProjectCloneInput
): { jobId: string } {
  const plan = mockValidateCloneTarget(input);
  const folderName = input.folderName?.trim() || plan.folderName || "cloned-repo";
  const destination = `${input.parentDirectory}/${folderName}`;
  const jobId = generateTestUuid();
  mockCloneJobs.set(jobId, {
    destination,
    defaultBranch: input.branch?.trim() || plan.branch || "main",
    outcome: getMockCloneJobOutcome(),
    callCount: 0,
    cancelled: false,
  });
  return { jobId };
}

export function mockCancelProjectClone(jobId: string): boolean {
  const job = mockCloneJobs.get(jobId);
  if (!job || job.cancelled || job.callCount > MOCK_CLONE_PHASES.length) {
    return false;
  }
  job.cancelled = true;
  return true;
}

/**
 * Drives web-mode clone progress: `MockEventBus` cannot deliver
 * `project:clone_*` events, so this status query is the only way Playwright
 * reaches progress and terminal frames. Advances one phase per call and
 * settles to a terminal state once every phase has been reported.
 */
export function mockGetCloneJobStatus(jobId: string): unknown {
  const job = mockCloneJobs.get(jobId);
  if (!job) {
    return { state: "unknown" };
  }
  if (job.outcome === "unknown") {
    return { state: "unknown" };
  }
  if (job.cancelled) {
    return { state: "cancelled", cleanedUp: true };
  }

  job.callCount += 1;
  const phaseIndex = job.callCount - 1;
  if (phaseIndex < MOCK_CLONE_PHASES.length) {
    return {
      state: "running",
      phase: MOCK_CLONE_PHASES[phaseIndex],
      percent: Math.round(((phaseIndex + 1) / MOCK_CLONE_PHASES.length) * 100),
    };
  }

  if (job.outcome === "auth_failed") {
    return {
      state: "failed",
      code: "CLONE_AUTH_FAILED",
      message: "GitHub needs you to sign in again before this repository can be cloned.",
      cleanedUp: true,
    };
  }

  const repoName = job.destination.split("/").pop() ?? "repository";
  return {
    state: "completed",
    destination: job.destination,
    defaultBranch: job.defaultBranch,
    capability: {
      kind: "github",
      fetch_url: null,
      push_url: `git@github.com:mock-org/${repoName}.git`,
    },
  };
}

// ============================================================================
// Mock Worktree Parent Validation (snake_case `verdict` tag)
// ============================================================================

export interface MockValidateWorktreeParentInput {
  path: string;
  repositoryRoot?: string;
}

export function mockValidateWorktreeParent(
  input: MockValidateWorktreeParentInput
): unknown {
  const path = input.path?.trim();
  if (!path) {
    return { verdict: "invalid", message: "Enter a folder path." };
  }
  if (path.includes("does-not-exist")) {
    return { verdict: "not_found", path };
  }
  if (path.includes("not-a-dir") || /\.[a-z0-9]{1,8}$/i.test(path)) {
    return { verdict: "not_a_directory", path };
  }
  const repositoryRoot = input.repositoryRoot?.trim();
  if (repositoryRoot && (path === repositoryRoot || path.startsWith(`${repositoryRoot}/`))) {
    return { verdict: "inside_repository", path };
  }
  if (path.includes("readonly")) {
    return { verdict: "not_writable", path };
  }
  return { verdict: "ok", path };
}

// ============================================================================
// Mock GitHub Repository Picker
// ============================================================================

export interface MockGitHubRepositorySummary {
  nameWithOwner: string;
  description: string | null;
  isPrivate: boolean;
  updatedAt: string;
}

export function mockListGithubRepositories(): MockGitHubRepositorySummary[] {
  return [
    {
      nameWithOwner: "acme/ralphx-demo",
      description: "Demo repository for RalphX walkthroughs.",
      isPrivate: false,
      updatedAt: "2026-08-10T14:00:00Z",
    },
    {
      nameWithOwner: "acme/internal-tools",
      description: "Internal tooling scripts.",
      isPrivate: true,
      updatedAt: "2026-08-05T09:30:00Z",
    },
    {
      nameWithOwner: "acme/docs-site",
      description: null,
      isPrivate: false,
      updatedAt: "2026-07-28T18:15:00Z",
    },
  ];
}
