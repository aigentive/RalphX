/**
 * Mock Plan Branch API
 *
 * Mirrors the interface of src/api/plan-branch.ts with mock implementations.
 * Returns camelCase data matching the PlanBranch type (like other mockApi modules).
 * The tauri-api-core.ts handlers convert to snake_case when needed.
 */

import type { PlanBranch, EnableFeatureBranchInput } from "@/api/plan-branch.types";
import { generateTestUuid } from "@/test/mock-data";

// ============================================================================
// Mock State
// ============================================================================

const mockPlanBranches: Map<string, PlanBranch> = new Map();

function ensureMockData(): void {
  if (mockPlanBranches.size > 0) return;

  // Sample active plan branch for plan-mock-1
  const branch: PlanBranch = {
    id: "plan-branch-mock-1",
    planArtifactId: "plan-mock-1",
    sessionId: "session-mock-1",
    projectId: "project-mock-1",
    branchName: "ralphx/demo-project/plan-a1b2c3",
    sourceBranch: "main",
    status: "active",
    mergeTaskId: "task-mock-merge-1",
    createdAt: new Date().toISOString(),
    mergedAt: null,
    prNumber: null,
    prUrl: null,
    prDraft: null,
    prPushStatus: null,
    prStatus: null,
    prPollingActive: false,
    prEligible: false,
    baseBranchOverride: null,
    executionPlanId: null,
  };
  mockPlanBranches.set(branch.id, branch);
}

// ============================================================================
// Mock Plan Branch API (camelCase for mockApi consumers)
// ============================================================================

export const mockPlanBranchApi = {
  getByPlan: async (planArtifactId: string): Promise<PlanBranch | null> => {
    ensureMockData();
    for (const branch of mockPlanBranches.values()) {
      if (branch.planArtifactId === planArtifactId) {
        return branch;
      }
    }
    return null;
  },

  getByProject: async (projectId: string): Promise<PlanBranch[]> => {
    ensureMockData();
    return Array.from(mockPlanBranches.values()).filter(
      (b) => b.projectId === projectId
    );
  },

  enable: async (input: EnableFeatureBranchInput): Promise<PlanBranch> => {
    const branch: PlanBranch = {
      id: generateTestUuid(),
      planArtifactId: input.planArtifactId,
      sessionId: input.sessionId,
      projectId: input.projectId,
      branchName: `ralphx/project/plan-${input.planArtifactId.slice(0, 6)}`,
      sourceBranch: "main",
      status: "active",
      mergeTaskId: generateTestUuid(),
      createdAt: new Date().toISOString(),
      mergedAt: null,
      prNumber: null,
      prUrl: null,
      prDraft: null,
      prPushStatus: null,
      prStatus: null,
      prPollingActive: false,
      prEligible: false,
      baseBranchOverride: input.baseBranchOverride ?? null,
      executionPlanId: null,
    };
    mockPlanBranches.set(branch.id, branch);
    return branch;
  },

  disable: async (planArtifactId: string): Promise<void> => {
    for (const [id, branch] of mockPlanBranches.entries()) {
      if (branch.planArtifactId === planArtifactId) {
        mockPlanBranches.delete(id);
        break;
      }
    }
  },

  updateProjectSetting: async (
    _projectId: string,
    _enabled: boolean
  ): Promise<void> => {
    // No-op in mock mode
  },
} as const;

// ============================================================================
// Snake_case transform for tauri-api-core.ts command handlers
// ============================================================================

export function toSnakeCasePlanBranch(b: PlanBranch) {
  return {
    id: b.id,
    plan_artifact_id: b.planArtifactId,
    session_id: b.sessionId,
    project_id: b.projectId,
    branch_name: b.branchName,
    source_branch: b.sourceBranch,
    status: b.status,
    merge_task_id: b.mergeTaskId,
    created_at: b.createdAt,
    merged_at: b.mergedAt,
    pr_number: b.prNumber,
    pr_url: b.prUrl,
    pr_draft: b.prDraft,
    pr_push_status: b.prPushStatus,
    pr_status: b.prStatus,
    pr_polling_active: b.prPollingActive,
    pr_eligible: b.prEligible,
    base_branch_override: b.baseBranchOverride,
  };
}
