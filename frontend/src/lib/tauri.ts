// Tauri invoke wrappers with type safety using Zod schemas
// This file serves as the main entry point and re-exports domain-specific API modules
//
// Web Mode Support:
// When running in a browser (without Tauri), this module automatically switches
// to mock implementations for visual testing and development.

import { z } from "zod";
import { isWebMode } from "./tauri-detection";

// Re-export environment detection utilities
export { isWebMode, isTauriMode } from "./tauri-detection";

// ============================================================================
// Core Utilities
// ============================================================================

// `typedInvoke` / `typedInvokeWithTransform` live in their own module so importing
// them does not drag this barrel's entire API graph along; re-exported here so every
// existing `from "@/lib/tauri"` call site is unaffected.
export {
  typedInvoke,
  typedInvokeWithTransform,
} from "./typed-invoke";

import { typedInvoke } from "./typed-invoke";

// ============================================================================
// Shared Schemas
// ============================================================================

/**
 * Tauri serializes Rust () as JSON null, not undefined. Use instead of z.void() for commands returning Result<(), _>.
 */
export const TauriVoidSchema = z.null();

// ============================================================================
// Health Check (Universal)
// ============================================================================

/**
 * Health check response schema
 */
export const HealthResponseSchema = z.object({
  status: z.string(),
});

export type HealthResponse = z.infer<typeof HealthResponseSchema>;

// ============================================================================
// Re-exports from Domain API Modules
// ============================================================================

// Execution API
export {
  executionApi,
  type ExecutionStatusResponse,
  type ExecutionCommandResponse,
  ExecutionStatusResponseSchema,
  ExecutionCommandResponseSchema,
  transformExecutionStatus,
  transformExecutionCommand,
} from "@/api/execution";

// Test Data API
export {
  testDataApi,
  type SeedResponse,
  type TestDataProfile,
} from "@/api/test-data";

// Projects API
export {
  projectsApi,
  workflowsApi,
  getGitBranches,
  getGitDefaultBranch,
} from "@/api/projects";

// Methodologies API
export {
  methodologiesApi,
  type MethodologyResponse,
  type MethodologyActivationResponse,
  MethodologyResponseSchema,
  MethodologyActivationResponseSchema,
} from "@/api/methodologies";

// Artifacts API
export {
  artifactsApi,
  type ArtifactResponse,
  type BucketResponse,
  type ArtifactRelationResponse,
  type CreateArtifactInput,
  type UpdateArtifactInput,
  type CreateBucketInput,
  type AddRelationInput,
  ArtifactResponseSchema,
  BucketResponseSchema,
  ArtifactRelationResponseSchema,
} from "@/api/artifacts";

// Research API
export {
  researchApi,
  type ResearchProcessResponse,
  type ResearchPresetResponse,
  type StartResearchInput,
  type CustomDepthInput,
  ResearchProcessResponseSchema,
  ResearchPresetResponseSchema,
  StartResearchInputSchema,
  CustomDepthInputSchema,
} from "@/api/research";

// Ask User Question API
export { askUserQuestionApi, type ResolveQuestionInput } from "@/api/ask-user-question";

// Permission API
export { permissionApi, type ResolvePermissionInput } from "@/api/permission";

// QA API
export {
  qaApi,
  type UpdateQASettingsInput,
  AcceptanceCriterionResponseSchema,
  QATestStepResponseSchema,
  QAStepResultResponseSchema,
  QAResultsResponseSchema,
  TaskQAResponseSchema,
  type AcceptanceCriterionResponse,
  type QATestStepResponse,
  type QAStepResultResponse,
  type QAResultsResponse,
  type TaskQAResponse,
} from "@/api/qa-api";

// Reviews API
export {
  reviewsApi,
  fixTasksApi,
  type ApproveReviewInput,
  type RequestChangesInput,
  type RejectReviewInput,
  type ApproveFixTaskInput,
  type RejectFixTaskInput,
  ReviewResponseSchema,
  ReviewActionResponseSchema,
  ReviewNoteResponseSchema,
  ReviewIssueSchema,
  FixTaskAttemptsResponseSchema,
  ReviewListResponseSchema,
  ReviewNoteListResponseSchema,
  type ReviewResponse,
  type ReviewActionResponse,
  type ReviewNoteResponse,
  type ReviewIssue,
  type FixTaskAttemptsResponse,
} from "@/api/reviews-api";

// Tasks API
export {
  tasksApi,
  stepsApi,
  type InjectTaskInput,
  type InjectTaskResponse,
  InjectTaskResponseSchemaRaw,
  transformInjectTaskResponse,
} from "@/api/tasks";

// Plan Branch API
export {
  planBranchApi,
  type PlanBranch,
  type PlanBranchStatus,
  type EnableFeatureBranchInput,
  PlanBranchSchema,
  PlanBranchStatusSchema,
  PlanBranchListSchema,
  PlanBranchNullableSchema,
  transformPlanBranch,
} from "@/api/plan-branch";

// Agent Issue Report API
export {
  agentIssueReportApi,
  type AgentIssueReportDraft,
  type AgentIssueReportDestination,
  type AgentIssueReportSubmitResponse,
  type BuildAgentIssueReportInput,
  type SubmitAgentIssueReportInput,
} from "@/api/agent-issue-report";

// Automations API
export {
  automationsApi,
  type Automation,
  type AutomationBaseRefKind,
  type AutomationChainMode,
  type AutomationCompletionSignal,
  type AutomationDetail,
  type AutomationJudgeState,
  type AutomationPromptAuthor,
  type AutomationRun,
  type AutomationRunMode,
  type AutomationRunScopedInput,
  type AutomationRunStatus,
  type AutomationScheduleResponse,
  type AutomationStatus,
  type CreateAutomationDraftInput,
  type CreateAutomationDraftResponse,
  type ListAutomationsInput,
  type PauseAutomationInput,
  type UpdateAutomationSettingsInput,
  type UpdateAutomationSetupInput,
  AutomationBaseRefKindSchema,
  AutomationChainModeSchema,
  AutomationCompletionSignalSchema,
  AutomationDetailSchema,
  AutomationJudgeStateSchema,
  AutomationListSchema,
  AutomationPromptAuthorSchema,
  AutomationScheduleResponseSchema,
  AutomationRunModeSchema,
  AutomationRunSchema,
  AutomationRunStatusSchema,
  AutomationSchema,
  AutomationStatusSchema,
  CreateAutomationDraftResponseSchema,
  transformAutomation,
  transformAutomationDetail,
  transformAutomationRun,
  transformAutomationRunScopedInput,
  transformAutomationScheduleResponse,
  transformCreateAutomationDraftResponse,
} from "@/api/automations";

// ============================================================================
// Aggregate API Object
// ============================================================================

import { executionApi } from "@/api/execution";
import { testDataApi } from "@/api/test-data";
import { projectsApi, workflowsApi } from "@/api/projects";
import { methodologiesApi } from "@/api/methodologies";
import { artifactsApi } from "@/api/artifacts";
import { researchApi } from "@/api/research";
import { askUserQuestionApi } from "@/api/ask-user-question";
import { permissionApi } from "@/api/permission";
import { qaApi } from "@/api/qa-api";
import { reviewsApi, fixTasksApi } from "@/api/reviews-api";
import { tasksApi, stepsApi } from "@/api/tasks";
import { planBranchApi } from "@/api/plan-branch";
import { agentIssueReportApi } from "@/api/agent-issue-report";
import { automationsApi } from "@/api/automations";

// Mock API imports for web mode
import { mockApi } from "@/api-mock";

/**
 * Real Tauri API object containing all typed Tauri command wrappers
 */
const realApi = {
  health: {
    /**
     * Check if the backend is running
     * @returns { status: "ok" } if healthy
     */
    check: () => typedInvoke("health_check", {}, HealthResponseSchema),
  },

  tasks: tasksApi,
  projects: projectsApi,
  workflows: workflowsApi,
  methodologies: methodologiesApi,
  artifacts: artifactsApi,
  research: researchApi,
  askUserQuestion: askUserQuestionApi,
  permission: permissionApi,
  qa: qaApi,
  reviews: reviewsApi,
  fixTasks: fixTasksApi,
  execution: executionApi,
  steps: stepsApi,
  testData: testDataApi,
  planBranches: planBranchApi,
  agentIssueReport: agentIssueReportApi,
  automations: automationsApi,
} as const;

/**
 * Aggregate API object - automatically switches between real Tauri API and mock API
 *
 * - In Tauri WebView: Uses real Tauri invoke() calls
 * - In browser (web mode): Uses mock implementations for testing
 *
 * This provides backward compatibility for existing imports of `api`
 *
 * Note: We cache the result after first access to avoid repeated checks,
 * but the check is deferred until first use to handle Tauri initialization timing.
 */
let _cachedApi: typeof realApi | typeof mockApi | null = null;

function getApi(): typeof realApi | typeof mockApi {
  if (_cachedApi === null) {
    _cachedApi = isWebMode() ? mockApi : realApi;
  }
  return _cachedApi;
}

export const api = new Proxy({} as typeof realApi, {
  get(_target, prop: keyof typeof realApi) {
    return getApi()[prop];
  },
});
