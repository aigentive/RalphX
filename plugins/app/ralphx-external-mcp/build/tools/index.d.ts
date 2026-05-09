/**
 * Tool registration for ralphx-external-mcp
 *
 * All tools are v1_ prefixed per the versioning decision in the plan.
 * Full tool implementations are in Phase 4 (discovery/ideation) and Phase 5 (pipeline).
 * This module registers placeholder tool definitions for the server scaffold.
 */
import type { Server } from "@modelcontextprotocol/sdk/server/index.js";
import type { ApiKeyContext } from "../types.js";
/** Tool categories by phase */
export declare const TOOL_CATEGORIES: {
    readonly setup: readonly ["v1_register_project"];
    readonly onboarding: readonly ["v1_get_agent_guide"];
    readonly discovery: readonly ["v1_list_projects", "v1_get_project_status", "v1_get_pipeline_overview"];
    readonly ideation: readonly ["v1_start_ideation", "v1_get_ideation_status", "v1_send_ideation_message", "v1_get_ideation_messages", "v1_list_proposals", "v1_get_proposal_detail", "v1_get_plan", "v1_accept_plan_and_schedule", "v1_modify_proposal", "v1_analyze_dependencies", "v1_trigger_plan_verification", "v1_get_plan_verification", "v1_list_ideation_sessions", "v1_get_session_tasks", "v1_append_task_to_plan"];
    readonly tasks: readonly ["v1_get_task_steps", "v1_batch_task_status"];
    readonly pipeline: readonly ["v1_get_task_detail", "v1_get_task_diff", "v1_get_review_summary", "v1_approve_review", "v1_request_changes", "v1_get_merge_pipeline", "v1_resolve_escalation", "v1_pause_task", "v1_cancel_task", "v1_retry_task", "v1_resume_scheduling", "v1_create_task_note"];
    readonly events: readonly ["v1_subscribe_events", "v1_get_recent_events", "v1_get_attention_items", "v1_get_execution_capacity", "v1_register_webhook", "v1_unregister_webhook", "v1_list_webhooks", "v1_get_webhook_health"];
};
/** Register all tool handlers on the MCP server */
export declare function registerTools(server: Server, getKeyContext: () => ApiKeyContext | undefined): void;
//# sourceMappingURL=index.d.ts.map