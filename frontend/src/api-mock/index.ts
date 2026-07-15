/**
 * Mock API Module
 *
 * Provides a mock implementation of the Tauri API for browser testing.
 * Mirrors the interface of src/lib/tauri.ts api object.
 *
 * Usage:
 * - List/get operations return factory-generated mock data
 * - Create/update/delete operations return success (no-op for read-only mode)
 * - Sufficient for visual regression testing, styling checks, layout verification
 */

import { mockTasksApi, mockStepsApi } from "./tasks";
import { mockProjectsApi, mockWorkflowsApi } from "./projects";
import { mockMethodologiesApi } from "./methodologies";
import { mockArtifactsApi } from "./artifact";
import { mockResearchApi } from "./research";
import { mockAskUserQuestionApi } from "./ask-user-question";
import { mockPermissionApi } from "./permission";
import { mockExecutionApi } from "./execution";
import { mockTaskGraphApi } from "./task-graph";
import { mockReviewsApi, mockFixTasksApi } from "./reviews";
import { mockReviewIssuesApi } from "./review-issues";
import { mockQaApi } from "./qa";
import { mockTestDataApi } from "./test-data";
import { mockPlanBranchApi } from "./plan-branch";
import { mockPlanApi } from "./plan";
import { mockAgentIssueReportApi } from "./agent-issue-report";
import { mockAutomationsApi } from "./automations";

// Re-export for direct imports
export { mockTasksApi, mockStepsApi } from "./tasks";
export { mockProjectsApi, mockWorkflowsApi, mockGetGitBranches, mockGetGitDefaultBranch } from "./projects";
export { mockMethodologiesApi } from "./methodologies";
export { mockArtifactsApi } from "./artifact";
export { mockResearchApi } from "./research";
export { mockAskUserQuestionApi } from "./ask-user-question";
export { mockPermissionApi } from "./permission";
export { mockExecutionApi } from "./execution";
export { mockTaskGraphApi } from "./task-graph";
export { mockChatApi } from "./chat";
export { mockIdeationApi } from "./ideation";
export { mockReviewsApi, mockFixTasksApi } from "./reviews";
export { mockReviewIssuesApi } from "./review-issues";
export { mockQaApi } from "./qa";
export { mockActivityEventsApi } from "./activity-events";
export { mockArtifactApi } from "./artifact";
export { mockTestDataApi } from "./test-data";
export { mockPlanBranchApi } from "./plan-branch";
export { mockPlanApi } from "./plan";
export { mockAgentIssueReportApi } from "./agent-issue-report";
export { mockAutomationsApi } from "./automations";
export { clearMockNotifications, getStore, resetStore, seedMockNotifications } from "./store";
export type { MockNotification } from "./store";

/**
 * Aggregate mock API object matching the structure of the real API
 *
 * This mirrors the api object from src/lib/tauri.ts
 */
export const mockApi = {
  health: {
    check: async () => ({ status: "ok" }),
  },

  tasks: mockTasksApi,
  projects: mockProjectsApi,
  workflows: mockWorkflowsApi,
  methodologies: mockMethodologiesApi,
  artifacts: mockArtifactsApi,
  research: mockResearchApi,
  askUserQuestion: mockAskUserQuestionApi,
  permission: mockPermissionApi,
  qa: mockQaApi,
  reviews: mockReviewsApi,
  reviewIssues: mockReviewIssuesApi,
  fixTasks: mockFixTasksApi,
  execution: mockExecutionApi,
  taskGraph: mockTaskGraphApi,
  steps: mockStepsApi,
  testData: mockTestDataApi,
  planBranches: mockPlanBranchApi,
  plan: mockPlanApi,
  agentIssueReport: mockAgentIssueReportApi,
  automations: mockAutomationsApi,
} as const;

/**
 * Type alias for the mock API (matches real API shape)
 */
export type MockApi = typeof mockApi;
