/**
 * Tool registration for ralphx-external-mcp
 *
 * All tools are v1_ prefixed per the versioning decision in the plan.
 * Full tool implementations are in Phase 4 (discovery/ideation) and Phase 5 (pipeline).
 * This module registers placeholder tool definitions for the server scaffold.
 */

import type { Server } from "@modelcontextprotocol/sdk/server/index.js";
import {
  ListToolsRequestSchema,
  CallToolRequestSchema,
} from "@modelcontextprotocol/sdk/types.js";
import type { ApiKeyContext } from "../types.js";
import {
  handleListProjects,
  handleGetProjectStatus,
  handleGetPipelineOverview,
} from "./discovery.js";
import {
  handleStartIdeation,
  handleGetIdeationStatus,
  handleSendIdeationMessage,
  handleGetIdeationMessages,
  handleListProposals,
  handleGetProposalDetail,
  handleGetPlan,
  handleAcceptPlanAndSchedule,
  handleModifyProposal,
  handleAnalyzeDependencies,
  handleTriggerPlanVerification,
  handleGetPlanVerification,
  handleListIdeationSessions,
  handleGetSessionTasks,
  handleAppendTaskToPlan,
} from "./ideation.js";
import {
  handleGetTaskDetail,
  handleGetTaskDiff,
  handleGetReviewSummary,
  handleApproveReview,
  handleRequestChanges,
  handleGetMergePipeline,
  handleResolveEscalation,
  handlePauseTask,
  handleCancelTask,
  handleRetryTask,
  handleResumeScheduling,
  handleCreateTaskNote,
} from "./pipeline.js";
import {
  handleGetRecentEvents,
  handleSubscribeEvents,
  handleGetAttentionItems,
  handleGetExecutionCapacity,
  handleRegisterWebhook,
  handleUnregisterWebhook,
  handleListWebhooks,
  handleGetWebhookHealth,
} from "./events.js";
import { handleBatchTaskStatus, handleGetTaskSteps } from "./tasks.js";
import { handleGetAgentGuide } from "./guide.js";
import { handleRegisterProject } from "./projects.js";

/** Tool categories by phase */
export const TOOL_CATEGORIES = {
  setup: ["v1_register_project"],
  onboarding: ["v1_get_agent_guide"],
  discovery: ["v1_list_projects", "v1_get_project_status", "v1_get_pipeline_overview"],
  ideation: [
    "v1_start_ideation",
    "v1_get_ideation_status",
    "v1_send_ideation_message",
    "v1_get_ideation_messages",
    "v1_list_proposals",
    "v1_get_proposal_detail",
    "v1_get_plan",
    "v1_accept_plan_and_schedule",
    "v1_modify_proposal",
    "v1_analyze_dependencies",
    "v1_trigger_plan_verification",
    "v1_get_plan_verification",
    "v1_list_ideation_sessions",
    "v1_get_session_tasks",
    "v1_append_task_to_plan",
  ],
  tasks: ["v1_get_task_steps", "v1_batch_task_status"],
  pipeline: [
    "v1_get_task_detail",
    "v1_get_task_diff",
    "v1_get_review_summary",
    "v1_approve_review",
    "v1_request_changes",
    "v1_get_merge_pipeline",
    "v1_resolve_escalation",
    "v1_pause_task",
    "v1_cancel_task",
    "v1_retry_task",
    "v1_resume_scheduling",
    "v1_create_task_note",
  ],
  events: [
    "v1_subscribe_events",
    "v1_get_recent_events",
    "v1_get_attention_items",
    "v1_get_execution_capacity",
    "v1_register_webhook",
    "v1_unregister_webhook",
    "v1_list_webhooks",
    "v1_get_webhook_health",
  ],
} as const;

/** Register all tool handlers on the MCP server */
export function registerTools(
  server: Server,
  getKeyContext: () => ApiKeyContext | undefined
): void {
  // List tools — returns all available tool definitions
  server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: [
      // Setup: Project registration (requires CREATE_PROJECT permission)
      {
        name: "v1_register_project",
        description:
          "Register a folder as a RalphX project. Creates the directory if it doesn't exist, initializes git if needed. Requires CREATE_PROJECT permission (bit 8). The creating key automatically gets access to the new project.",
        inputSchema: {
          type: "object" as const,
          properties: {
            working_directory: {
              type: "string",
              description: "Absolute path to the project directory (will be created if it doesn't exist)",
            },
            name: {
              type: "string",
              description: "Optional project name (defaults to directory basename)",
            },
          },
          required: ["working_directory"],
        },
      },
      // Flow 0: Onboarding
      {
        name: "v1_get_agent_guide",
        description:
          "Get the complete RalphX agent workflow guide. CALL THIS FIRST. Returns tool reference, sequencing rules, patterns, and anti-patterns. Use section parameter to get focused content.",
        inputSchema: {
          type: "object" as const,
          properties: {
            section: {
              type: "string",
              enum: ["setup", "overview", "discovery", "ideation", "tasks", "pipeline", "events", "patterns"],
              description: "Optional: return only a specific section to save context window",
            },
          },
          required: [],
        },
      },
      // Flow 1: Project Discovery (Phase 4)
      {
        name: "v1_list_projects",
        description: "List projects accessible to this API key",
        inputSchema: {
          type: "object" as const,
          properties: {},
          required: [],
        },
      },
      {
        name: "v1_get_project_status",
        description: "Get project details, task counts, and running agent status",
        inputSchema: {
          type: "object" as const,
          properties: {
            project_id: { type: "string", description: "Project ID" },
          },
          required: ["project_id"],
        },
      },
      {
        name: "v1_get_pipeline_overview",
        description: "Get tasks grouped by pipeline stage with counts",
        inputSchema: {
          type: "object" as const,
          properties: {
            project_id: { type: "string", description: "Project ID" },
            since: {
              type: "string",
              description: "ISO 8601 timestamp — only return tasks changed after this time in changed_tasks list. Stage counts always reflect all tasks.",
            },
          },
          required: ["project_id"],
        },
      },
      // Flow 2: Ideation & Planning (Phase 4)
      {
        name: "v1_start_ideation",
        description:
          "Create an ideation session and spawn an orchestrator agent with the given prompt. Returns existing_active_sessions for visibility. Use idempotency_key for safe retries — same key returns same session without creating a duplicate.",
        inputSchema: {
          type: "object" as const,
          properties: {
            project_id: { type: "string", description: "Target project ID" },
            prompt: { type: "string", description: "Initial prompt for the orchestrator" },
            idempotency_key: {
              type: "string",
              description:
                "Optional client-provided key for safe retries. Same key+API key always returns the same session.",
            },
          },
          required: ["project_id", "prompt"],
        },
      },
      {
        name: "v1_get_ideation_status",
        description: "Get ideation session status, agent state, proposal count, and verification state. Returns agent_status (idle/generating/waiting_for_input), verification_status, and verification_in_progress. Use agent_status instead of the deprecated agent_running boolean.",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Ideation session ID" },
          },
          required: ["session_id"],
        },
      },
      {
        name: "v1_send_ideation_message",
        description: "Send a message to the ideation agent",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Ideation session ID" },
            message: { type: "string", description: "Message to send" },
          },
          required: ["session_id", "message"],
        },
      },
      {
        name: "v1_get_ideation_messages",
        description:
          "Get user and orchestrator messages for an ideation session. Excludes system messages and auto-verification messages. Returns agent_status (idle/generating/waiting_for_input).",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Ideation session ID" },
            limit: { type: "number", description: "Max messages to return (default 50)" },
            offset: { type: "number", description: "Pagination offset (default 0)" },
          },
          required: ["session_id"],
        },
      },
      {
        name: "v1_list_proposals",
        description: "List proposals in an ideation session",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Ideation session ID" },
          },
          required: ["session_id"],
        },
      },
      {
        name: "v1_get_proposal_detail",
        description: "Get full proposal details including steps and acceptance criteria",
        inputSchema: {
          type: "object" as const,
          properties: {
            proposal_id: { type: "string", description: "Proposal ID" },
          },
          required: ["proposal_id"],
        },
      },
      {
        name: "v1_get_plan",
        description: "Get plan artifact content for an ideation session",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Ideation session ID" },
          },
          required: ["session_id"],
        },
      },
      {
        name: "v1_accept_plan_and_schedule",
        description:
          "Saga: apply proposals → create tasks → schedule (idempotent, resumable on failure). " +
          "Optional: set base_branch_override to merge into a branch other than the project default (e.g. 'develop').",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Ideation session ID" },
            base_branch_override: {
              type: "string",
              description:
                "Branch to merge into instead of the project default (e.g. 'develop', 'staging'). " +
                "Use v1_get_project_details to discover the project's default base_branch.",
            },
          },
          required: ["session_id"],
        },
      },
      {
        name: "v1_modify_proposal",
        description: "Update a proposal before acceptance",
        inputSchema: {
          type: "object" as const,
          properties: {
            proposal_id: { type: "string", description: "Proposal ID" },
            changes: { type: "object", description: "Fields to update" },
          },
          required: ["proposal_id", "changes"],
        },
      },
      {
        name: "v1_analyze_dependencies",
        description: "Get dependency graph for proposals in a session",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Ideation session ID" },
          },
          required: ["session_id"],
        },
      },
      {
        name: "v1_trigger_plan_verification",
        description:
          "Trigger automatic plan verification for a session. Returns status: 'triggered' | 'already_running' | 'no_plan'",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Ideation session ID" },
          },
          required: ["session_id"],
        },
      },
      {
        name: "v1_get_plan_verification",
        description:
          "Get plan verification status for a session. Returns: status, in_progress, round, max_rounds, gap_count, gap_score (weighted: critical×10+high×3+medium×1), gaps (array of {severity, category, description}), convergence_reason. Also returns `verification_child` (object or null): `active_child_session_id` (non-null only when verification is in progress), `latest_child_session_id`, `latest_child_archived`, `agent_state` ('likely_generating'|'likely_waiting'|'idle'), `last_assistant_message` (500-char truncated), `last_assistant_message_at`. Check these fields to recover from `agent_completed_without_update` without a second lookup.",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Ideation session ID" },
          },
          required: ["session_id"],
        },
      },
      {
        name: "v1_list_ideation_sessions",
        description: "List ideation sessions for a project with optional status filter",
        inputSchema: {
          type: "object" as const,
          properties: {
            project_id: { type: "string", description: "Project ID" },
            status: {
              type: "string",
              enum: ["active", "accepted", "archived", "all"],
              description: "Filter by status (default: all)",
            },
            limit: { type: "number", description: "Max sessions to return (default: 20, max: 100)" },
            updated_after: {
              type: "string",
              description: "ISO 8601 timestamp (RFC 3339). Only return sessions updated after this time.",
            },
          },
          required: ["project_id"],
        },
      },
      {
        name: "v1_get_session_tasks",
        description: "Get all tasks created from an ideation session with aggregate delivery_status. Returns task list, delivery_status (not_scheduled | in_progress | pending_review | partial | delivered), and task_count.",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Ideation session ID" },
            changed_since: { type: "string", description: "ISO 8601 / RFC3339 timestamp — only return tasks updated after this time" },
          },
          required: ["session_id"],
        },
      },
      {
        name: "v1_append_task_to_plan",
        description:
          "Append a one-off task to an already accepted ideation plan while its plan branch is still active, including when its PR is open and waiting. " +
          "Use this for small follow-ups after plan acceptance; start a new ideation instead once the PR/plan is closed, merged, terminal, or actively merging.",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Accepted ideation session ID" },
            title: { type: "string", description: "Short task title" },
            description: { type: "string", description: "Task description and implementation intent" },
            steps: {
              type: "array",
              items: { type: "string" },
              description: "Concrete execution steps for the appended task",
            },
            acceptance_criteria: {
              type: "array",
              items: { type: "string" },
              description: "Acceptance criteria the task must satisfy",
            },
            depends_on_task_ids: {
              type: "array",
              items: { type: "string" },
              description: "Optional existing task IDs that must complete first",
            },
            priority: { type: "number", description: "Optional numeric priority" },
          },
          required: ["session_id", "title", "steps", "acceptance_criteria"],
        },
      },
      // Task Steps
      {
        name: "v1_get_task_steps",
        description: "List all steps for a task, including status and completion notes",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_id: { type: "string", description: "Task ID" },
          },
          required: ["task_id"],
        },
      },
      // Flow 3: Task Pipeline Supervision (Phase 5)
      {
        name: "v1_get_task_detail",
        description: "Get full task details, steps, and branch info",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_id: { type: "string", description: "Task ID" },
          },
          required: ["task_id"],
        },
      },
      {
        name: "v1_get_task_diff",
        description: "Get git diff stats for a task branch",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_id: { type: "string", description: "Task ID" },
          },
          required: ["task_id"],
        },
      },
      {
        name: "v1_get_review_summary",
        description: "Get review notes and findings for a task",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_id: { type: "string", description: "Task ID" },
          },
          required: ["task_id"],
        },
      },
      {
        name: "v1_approve_review",
        description: "Approve a task review, moving it to merge",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_id: { type: "string", description: "Task ID" },
          },
          required: ["task_id"],
        },
      },
      {
        name: "v1_request_changes",
        description: "Request changes on a task review with feedback",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_id: { type: "string", description: "Task ID" },
            feedback: { type: "string", description: "Change request feedback" },
          },
          required: ["task_id", "feedback"],
        },
      },
      {
        name: "v1_get_merge_pipeline",
        description: "Get all merge activity for scoped projects",
        inputSchema: {
          type: "object" as const,
          properties: {
            project_id: { type: "string", description: "Project ID" },
          },
          required: ["project_id"],
        },
      },
      {
        name: "v1_resolve_escalation",
        description: "Handle an escalated review for a task",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_id: { type: "string", description: "Task ID" },
            resolution: {
              type: "string",
              enum: ["approve", "request_changes", "cancel"],
              description: "Resolution action",
            },
            feedback: { type: "string", description: "Optional feedback" },
          },
          required: ["task_id", "resolution"],
        },
      },
      {
        name: "v1_pause_task",
        description: "Pause a running task",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_id: { type: "string", description: "Task ID" },
          },
          required: ["task_id"],
        },
      },
      {
        name: "v1_cancel_task",
        description: "Cancel a task",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_id: { type: "string", description: "Task ID" },
          },
          required: ["task_id"],
        },
      },
      {
        name: "v1_retry_task",
        description: "Retry a failed or stopped task",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_id: { type: "string", description: "Task ID" },
          },
          required: ["task_id"],
        },
      },
      {
        name: "v1_resume_scheduling",
        description:
          "Resume a failed v1_accept_plan_and_schedule from its last successful step",
        inputSchema: {
          type: "object" as const,
          properties: {
            session_id: { type: "string", description: "Ideation session ID" },
          },
          required: ["session_id"],
        },
      },
      {
        name: "v1_create_task_note",
        description: "Annotate a task with a progress note visible to human reviewers",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_id: { type: "string", description: "Task ID" },
            note: { type: "string", description: "Note text to append to the task" },
          },
          required: ["task_id", "note"],
        },
      },
      // Flow 4: Events & Monitoring (Phase 6)
      {
        name: "v1_subscribe_events",
        description: "SSE stream of state change events for scoped projects",
        inputSchema: {
          type: "object" as const,
          properties: {
            project_id: { type: "string", description: "Project ID to filter events" },
          },
          required: [],
        },
      },
      {
        name: "v1_get_recent_events",
        description:
          "Cursor-based event retrieval from DB (survives restarts). Pass cursor=0 or omit for all recent events.",
        inputSchema: {
          type: "object" as const,
          properties: {
            project_id: { type: "string", description: "Project ID to filter events" },
            cursor: {
              type: "number",
              description: "Last event ID received (0 for all recent). Primary pagination field.",
            },
            last_id: {
              type: "number",
              description: "Deprecated alias for cursor. Use cursor instead.",
            },
            limit: { type: "number", description: "Max events to return (default: 50)" },
            event_type: {
              type: "string",
              description: "Optional event type filter (e.g. 'task:status_changed'). Omit to receive all types.",
            },
          },
          required: [],
        },
      },
      {
        name: "v1_get_attention_items",
        description:
          "Get tasks needing attention (escalated reviews, merge conflicts) for scoped projects",
        inputSchema: {
          type: "object" as const,
          properties: {
            project_id: { type: "string", description: "Project ID" },
          },
          required: [],
        },
      },
      {
        name: "v1_get_execution_capacity",
        description:
          "Get execution capacity: can_start (bool), project_running (N), project_queued (N)",
        inputSchema: {
          type: "object" as const,
          properties: {
            project_id: { type: "string", description: "Project ID" },
          },
          required: ["project_id"],
        },
      },
      {
        name: "v1_register_webhook",
        description:
          "Register a webhook URL to receive real-time RalphX pipeline events via HTTP POST. " +
          "Returns the HMAC-SHA256 secret — store it securely, it won't be shown again. " +
          "Idempotent: registering the same URL resets failure count and reactivates if inactive.",
        inputSchema: {
          type: "object" as const,
          properties: {
            url: {
              type: "string",
              description: "Webhook URL to receive event POSTs (must be reachable from RalphX)",
            },
            event_types: {
              type: "array",
              items: { type: "string" },
              description:
                "Optional: filter to specific event types. Omit to receive all events. " +
                "Examples: task:status_changed, review:ready, merge:completed, ideation:proposals_ready",
            },
            project_ids: {
              type: "array",
              items: { type: "string" },
              description:
                "Optional: filter to specific project IDs. Omit to receive events for all authorized projects.",
            },
          },
          required: ["url"],
        },
      },
      {
        name: "v1_unregister_webhook",
        description: "Remove a webhook registration. Use v1_list_webhooks to find the webhook_id.",
        inputSchema: {
          type: "object" as const,
          properties: {
            webhook_id: { type: "string", description: "Webhook registration ID to remove" },
          },
          required: ["webhook_id"],
        },
      },
      {
        name: "v1_list_webhooks",
        description:
          "List all active webhook registrations for this API key, including their IDs, URLs, event type filters, project filters, and failure counts.",
        inputSchema: {
          type: "object" as const,
          properties: {},
          required: [],
        },
      },
      {
        name: "v1_get_webhook_health",
        description:
          "Check delivery health for all registered webhooks. Returns per-webhook stats: active status, failure count, and last failure time. " +
          "Use this to detect broken webhooks (active: false means auto-deactivated after 10+ consecutive failures). " +
          "Re-register the same URL to reset failure count and reactivate.",
        inputSchema: {
          type: "object" as const,
          properties: {},
          required: [],
        },
      },
      // Flow 5: Batch task operations
      {
        name: "v1_batch_task_status",
        description:
          "Batch lookup status for up to 50 task IDs. Returns tasks array + errors array with reason: not_found | access_denied.",
        inputSchema: {
          type: "object" as const,
          properties: {
            task_ids: {
              type: "array",
              items: { type: "string" },
              description: "List of task IDs to look up (max 50)",
            },
          },
          required: ["task_ids"],
        },
      },
    ],
  }));

  // Call tool handler — Flow 1 (discovery) and Flow 2 (ideation) implemented in Phase 4.
  // Flow 3 (pipeline) and Flow 4 (events) remain stubs until Phase 5/6.
  server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const { name, arguments: rawArgs } = request.params;
    const args = (rawArgs ?? {}) as Record<string, unknown>;

    const context = getKeyContext();
    if (!context) {
      return {
        content: [
          {
            type: "text" as const,
            text: JSON.stringify({ error: "unauthenticated", message: "No valid API key context." }),
          },
        ],
        isError: true,
      };
    }

    let text: string;
    let isError = false;

    switch (name) {
      // --- Setup: Project registration ---
      case "v1_register_project": {
        const registerResult = await handleRegisterProject(args, context);
        text = registerResult.text;
        isError = registerResult.isError;
        break;
      }

      // --- Flow 0: Onboarding ---
      case "v1_get_agent_guide":
        text = await handleGetAgentGuide(args, context);
        break;

      // --- Flow 1: Discovery ---
      case "v1_list_projects":
        text = await handleListProjects(args, context);
        break;
      case "v1_get_project_status":
        text = await handleGetProjectStatus(args, context);
        break;
      case "v1_get_pipeline_overview":
        text = await handleGetPipelineOverview(args, context);
        break;

      // --- Flow 2: Ideation ---
      case "v1_start_ideation":
        text = await handleStartIdeation(args, context);
        break;
      case "v1_get_ideation_status":
        text = await handleGetIdeationStatus(args, context);
        break;
      case "v1_send_ideation_message":
        text = await handleSendIdeationMessage(args, context);
        break;
      case "v1_get_ideation_messages":
        text = await handleGetIdeationMessages(args, context);
        break;
      case "v1_list_proposals":
        text = await handleListProposals(args, context);
        break;
      case "v1_get_proposal_detail":
        text = await handleGetProposalDetail(args, context);
        break;
      case "v1_get_plan":
        text = await handleGetPlan(args, context);
        break;
      case "v1_accept_plan_and_schedule":
        text = await handleAcceptPlanAndSchedule(args, context);
        break;
      case "v1_modify_proposal":
        text = await handleModifyProposal(args, context);
        break;
      case "v1_analyze_dependencies":
        text = await handleAnalyzeDependencies(args, context);
        break;
      case "v1_trigger_plan_verification":
        text = await handleTriggerPlanVerification(args, context);
        break;
      case "v1_get_plan_verification":
        text = await handleGetPlanVerification(args, context);
        break;
      case "v1_list_ideation_sessions":
        text = await handleListIdeationSessions(args, context);
        break;
      case "v1_get_session_tasks":
        text = await handleGetSessionTasks(args, context);
        break;
      case "v1_append_task_to_plan":
        text = await handleAppendTaskToPlan(args, context);
        break;

      // --- Task Steps ---
      case "v1_get_task_steps":
        text = await handleGetTaskSteps(args, context);
        break;

      // --- Flow 3: Pipeline Supervision ---
      case "v1_get_task_detail":
        text = await handleGetTaskDetail(args, context);
        break;
      case "v1_get_task_diff":
        text = await handleGetTaskDiff(args, context);
        break;
      case "v1_get_review_summary":
        text = await handleGetReviewSummary(args, context);
        break;
      case "v1_approve_review":
        text = await handleApproveReview(args, context);
        break;
      case "v1_request_changes":
        text = await handleRequestChanges(args, context);
        break;
      case "v1_get_merge_pipeline":
        text = await handleGetMergePipeline(args, context);
        break;
      case "v1_resolve_escalation":
        text = await handleResolveEscalation(args, context);
        break;
      case "v1_pause_task":
        text = await handlePauseTask(args, context);
        break;
      case "v1_cancel_task":
        text = await handleCancelTask(args, context);
        break;
      case "v1_retry_task":
        text = await handleRetryTask(args, context);
        break;
      case "v1_resume_scheduling":
        text = await handleResumeScheduling(args, context);
        break;
      case "v1_create_task_note":
        text = await handleCreateTaskNote(args, context);
        break;

      // --- Flow 4: Events & Monitoring ---
      case "v1_get_recent_events":
        text = await handleGetRecentEvents(args, context);
        break;
      case "v1_subscribe_events":
        text = await handleSubscribeEvents(args, context);
        break;
      case "v1_get_attention_items":
        text = await handleGetAttentionItems(args, context);
        break;
      case "v1_get_execution_capacity":
        text = await handleGetExecutionCapacity(args, context);
        break;
      case "v1_register_webhook":
        text = await handleRegisterWebhook(args, context);
        break;
      case "v1_unregister_webhook":
        text = await handleUnregisterWebhook(args, context);
        break;
      case "v1_list_webhooks":
        text = await handleListWebhooks(args, context);
        break;
      case "v1_get_webhook_health":
        text = await handleGetWebhookHealth(args, context);
        break;

      // --- Flow 5: Batch task operations ---
      case "v1_batch_task_status":
        text = await handleBatchTaskStatus(args, context);
        break;

      default:
        text = JSON.stringify({
          error: "not_implemented",
          tool: name,
          message: `Tool '${name}' is not recognized.`,
        });
        isError = true;
        break;
    }

    return {
      content: [{ type: "text" as const, text }],
      isError,
    };
  });
}
