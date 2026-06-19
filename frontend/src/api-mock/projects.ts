/**
 * Mock Projects API
 *
 * Mirrors the interface of src/api/projects.ts with mock implementations.
 */

import type { Project, CreateProject, UpdateProject } from "@/types/project";
import type { WorkflowSchema, WorkflowColumn } from "@/types/workflow";
import type { InternalStatus } from "@/types/status";
import type { CreateWorkflowInput, UpdateWorkflowInput } from "@/lib/api/workflows";
import { createMockProject, generateTestUuid } from "@/test/mock-data";
import { getStore } from "./store";

// ============================================================================
// Mock Projects API
// ============================================================================

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
    });
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
    return updated;
  },

  delete: async (_projectId: string): Promise<boolean> => {
    return true;
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
