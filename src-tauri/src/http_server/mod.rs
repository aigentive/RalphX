// HTTP server for MCP proxy - exposes Tauri commands via HTTP
// This allows the MCP server to call RalphX functionality via REST API

use axum::{
    http::StatusCode,
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use crate::application::AppState;
use crate::application::execution_state::ExecutionState;
use crate::error::AppResult;
use crate::utils::backend_endpoint::{backend_http_base_url, backend_http_bind_addr};
use delegation::DelegationService;

// ============================================================================
// Submodules
// ============================================================================

pub mod delegation;
#[cfg(test)]
mod delegation_tests;
pub mod handlers;
pub mod helpers;
pub mod native_delegation_launcher;
pub mod project_scope;
pub mod types;

use handlers::*;
pub use project_scope::*;
pub use types::*;

/// Health check endpoint — returns 200 OK with no body.
/// Must be unauthenticated and registered before any auth middleware layers.
pub(crate) async fn health_handler() -> StatusCode {
    StatusCode::OK
}

pub(crate) fn emit_app_event(app_state: &AppState, event: &str, payload: Value) {
    app_state.events.emit(event, payload);
}

pub(crate) fn emit_http_event(state: &HttpServerState, event: &str, payload: Value) {
    emit_app_event(&state.app_state, event, payload);
}

pub(crate) async fn recover_agent_workflow_runs_for_startup(
    app_state: Arc<AppState>,
    execution_state: Arc<ExecutionState>,
) -> AppResult<usize> {
    let state = HttpServerState {
        app_state,
        execution_state,
        delegation_service: Arc::new(DelegationService::new()),
        external_mcp_supervisor: None,
    };
    handlers::agent_workflows::recover_agent_workflow_runs(&state).await
}

pub(crate) fn emit_serialized_http_event<T: Serialize + ?Sized>(
    state: &HttpServerState,
    event: &str,
    payload: &T,
) {
    if let Err(error) =
        ralphx_events::emit_serialized(state.app_state.events.as_ref(), event, payload)
    {
        tracing::warn!(%event, %error, "Failed to serialize HTTP event payload");
    }
}

pub async fn start_http_server(
    app_state: Arc<AppState>,
    execution_state: Arc<ExecutionState>,
    shutdown: crate::application::HttpShutdownHandle,
) -> AppResult<()> {
    start_http_server_with_listener_ready(app_state, execution_state, shutdown, None, None).await
}

/// Starts the HTTP server and resolves the supplied sender only after the
/// local listener has bound. The caller owns readiness policy beyond binding.
pub async fn start_http_server_with_listener_ready(
    app_state: Arc<AppState>,
    execution_state: Arc<ExecutionState>,
    shutdown: crate::application::HttpShutdownHandle,
    mut listener_ready: Option<oneshot::Sender<AppResult<()>>>,
    external_mcp_supervisor: Option<
        Arc<
            dyn Fn() -> Option<Arc<crate::infrastructure::ExternalMcpSupervisor>> + Send + Sync,
        >,
    >,
) -> AppResult<()> {
    let state = HttpServerState {
        app_state,
        execution_state,
        delegation_service: Arc::new(DelegationService::new()),
        external_mcp_supervisor,
    };

    // Management routes — require admin API key + localhost-only CORS.
    // Bootstrap exception: unauthenticated when no active keys exist.
    // CORS restricted to Tauri app and local dev server origins (defense-in-depth
    // against CSRF from external websites; server already binds to 127.0.0.1).
    let management_routes = Router::new()
        .route("/api/auth/keys", post(create_api_key))
        .route("/api/auth/keys", get(list_api_keys))
        .route("/api/auth/keys/:id", delete(delete_api_key))
        .route("/api/auth/keys/:id/rotate", post(rotate_api_key))
        .route("/api/auth/keys/:id/projects", put(update_api_key_projects))
        .route("/api/auth/keys/:id/audit", get(get_audit_log))
        .route(
            "/api/auth/keys/:id/permissions",
            put(update_key_permissions),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_admin_key,
        ))
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(|origin, _| {
                    let s = origin.as_bytes();
                    s.starts_with(b"http://localhost")
                        || s.starts_with(b"https://localhost")
                        || s == b"tauri://localhost"
                        || s == b"https://tauri.localhost"
                }))
                .allow_methods(Any)
                .allow_headers(Any),
        );

    // Internal routes — NO CORS.
    // Accessed only by the internal MCP server (ralphx-mcp-server) running as a
    // child process on the same machine. Non-browser HTTP clients do not perform
    // CORS preflight requests, so adding a CORS layer here is both unnecessary
    // and undesirable (it would allow browser pages to call these routes).
    let internal_routes = Router::new()
        .route("/api/internal/projects", get(list_projects_internal))
        .route(
            "/api/internal/cross_project/create_session",
            post(create_cross_project_session_http),
        )
        .route(
            "/api/internal/cross_project/migrate_proposals",
            post(migrate_proposals_http),
        )
        .route(
            "/api/internal/sessions/:id/cross_project_check",
            post(set_cross_project_checked),
        );

    // Public API routes — permissive CORS.
    // Includes all existing MCP tool endpoints and external API endpoints.
    // The CorsLayer is applied only to this sub-router so the permissive
    // allow_origin(Any) does NOT bleed into management_routes (which use a
    // restrictive localhost-only CORS) or internal_routes (which need no CORS).
    let public_routes = Router::new()
        // Health check — unauthenticated, no auth middleware
        .route("/health", get(health_handler))
        .merge(management_routes)
        // Validate endpoints (public — validate a bearer token, no admin needed)
        .route("/api/auth/validate-key", get(validate_api_key))
        // Legacy validate_key endpoint (kept for backward compat)
        .route("/api/validate_key", get(validate_key))
        // Managed Team lifecycle surface
        .route("/api/managed_team/ensure", post(ensure_managed_team))
        .route(
            "/api/managed_team/status/:conversation_id",
            get(get_managed_team_status),
        )
        .route(
            "/api/managed_team/roster/:team_id",
            get(get_managed_team_roster),
        )
        .route("/api/managed_team/member", post(add_managed_team_member))
        .route(
            "/api/managed_team/member/assign",
            post(assign_managed_team_member),
        )
        .route(
            "/api/managed_team/member/stop",
            post(stop_managed_team_member),
        )
        .route("/api/managed_team/exit", post(exit_managed_team))
        .route(
            "/api/managed_team/members/idle",
            get(list_idle_managed_team_members),
        )
        .route("/api/managed_team/message", post(send_managed_team_message))
        .route(
            "/api/managed_team/member/roster",
            get(get_managed_team_member_roster),
        )
        .route(
            "/api/managed_team/members/status",
            get(get_managed_team_member_status),
        )
        // Ideation tools (ralphx-ideation agent)
        .route("/api/create_task_proposal", post(create_task_proposal))
        .route("/api/finalize_proposals", post(finalize_proposals))
        .route("/api/update_task_proposal", post(update_task_proposal))
        .route("/api/archive_task_proposal", post(archive_task_proposal))
        // Proposal query tools (ralphx-ideation agent)
        .route(
            "/api/list_session_proposals/:session_id",
            get(list_session_proposals),
        )
        .route("/api/proposal/:proposal_id", get(get_proposal))
        // Dependency analysis tools (ralphx-ideation agent)
        .route(
            "/api/analyze_dependencies/:session_id",
            get(analyze_session_dependencies),
        )
        // Session tools (ralphx-utility-session-namer agent)
        .route("/api/update_session_title", post(update_session_title))
        // Persona tools (flag-gated in the handler because MCP grants are flag-agnostic).
        .route("/api/save_persona_draft", post(save_persona_draft))
        .route("/api/get_persona_draft/:id", get(get_persona_draft))
        // Session linking tools (ralphx-ideation agent)
        .route("/api/create_child_session", post(create_child_session))
        .route(
            "/api/parent_session_context/:session_id",
            get(get_parent_session_context),
        )
        // Session messages (context recovery for ideation agents)
        .route("/api/get_session_messages", post(get_session_messages))
        // Plan artifact tools (ralphx-ideation agent)
        // NOTE: All ideation mutation routes MUST call assert_session_mutable() after fetching the session.
        .route(
            "/api/create_plan_artifact",
            post(create_plan_artifact_with_headers),
        )
        .route("/api/update_plan_artifact", post(update_plan_artifact))
        .route("/api/edit_plan_artifact", post(edit_plan_artifact))
        // UI-owned Plan-mode action; intentionally not exposed as an agent MCP tool.
        .route("/api/approve_plan_artifact", post(approve_plan_artifact))
        .route(
            "/api/plan_complexity_assessment/:session_id",
            get(get_plan_complexity_assessment),
        )
        .route(
            "/api/submit_plan_complexity_assessment",
            post(submit_plan_complexity_assessment),
        )
        .route(
            "/api/artifact/:artifact_id/history",
            get(get_artifact_history),
        )
        .route("/api/link_proposals_to_plan", post(link_proposals_to_plan))
        .route("/api/get_session_plan/:session_id", get(get_session_plan))
        // Plan verification tools (ralphx-ideation + worker agents)
        .route(
            "/api/ideation/sessions/:id/verification",
            get(get_plan_verification),
        )
        // Acceptance gate tools (user confirmation for require_accept_for_finalize)
        .route(
            "/api/ideation/sessions/:id/accept-finalize",
            post(accept_finalize),
        )
        .route(
            "/api/ideation/sessions/:id/reject-finalize",
            post(reject_finalize),
        )
        .route(
            "/api/ideation/sessions/:id/acceptance-status",
            get(get_acceptance_status),
        )
        .route(
            "/api/ideation/pending-confirmations",
            get(get_pending_confirmations),
        )
        // Verification confirmation endpoints (UI-session gate)
        .route("/api/verification/confirm", post(confirm_verification))
        .route(
            "/api/plan-verification/complete",
            post(complete_plan_verification_http),
        )
        // Child session tools for the primary ideation agent.
        .route(
            "/api/ideation/sessions/:id/child-status",
            get(get_child_session_status_handler),
        )
        .route(
            "/api/ideation/sessions/:id/message",
            post(send_ideation_session_message_handler),
        )
        .route(
            "/api/coordination/delegate/start",
            post(start_delegate_with_runtime_context),
        )
        .route(
            "/api/coordination/delegated-session/:id/status",
            get(get_delegated_session_status),
        )
        .route("/api/coordination/delegate/wait", post(wait_delegate))
        .route("/api/coordination/delegate/cancel", post(cancel_delegate))
        .route("/api/coordination/delegate/park", post(park_delegate))
        .route(
            "/api/coordination/delegate/parent-context",
            post(get_delegate_parent_context),
        )
        .route(
            "/api/agent_workflows/scripts/create",
            post(create_agent_workflow_script),
        )
        .route(
            "/api/agent_workflows/scripts/approve",
            post(approve_agent_workflow_script),
        )
        .route(
            "/api/agent_workflows/runs/start",
            post(start_agent_workflow_run),
        )
        .route(
            "/api/agent_workflows/runs/get",
            post(get_agent_workflow_run),
        )
        .route(
            "/api/agent_workflows/runs/latest",
            post(get_latest_agent_workflow_run_for_script),
        )
        .route(
            "/api/agent_workflows/runs/pause",
            post(pause_agent_workflow_run),
        )
        .route(
            "/api/agent_workflows/runs/resume",
            post(resume_agent_workflow_run),
        )
        .route(
            "/api/agent_workflows/runs/cancel",
            post(cancel_agent_workflow_run),
        )
        // Native agent task tools (lightweight todo/dependency tracking)
        .route("/api/agent_tasks/create", post(create_agent_task))
        .route("/api/agent_tasks/get", post(get_agent_task))
        .route("/api/agent_tasks/list", post(list_agent_tasks))
        .route("/api/agent_tasks/lists", post(list_agent_task_lists))
        .route(
            "/api/agent_tasks/list_for_list",
            post(list_agent_tasks_for_list),
        )
        .route("/api/agent_tasks/update", post(update_agent_task))
        .route("/api/agent_tasks/claim", post(claim_agent_task))
        .route("/api/agent_tasks/complete", post(complete_agent_task))
        .route(
            "/api/agent_tasks/delegate_assignment/get",
            post(get_delegate_assignment),
        )
        .route(
            "/api/agent_tasks/delegate_assignment/complete",
            post(complete_delegate_assignment),
        )
        .route(
            "/api/agent_tasks/delegate_assignment/release",
            post(release_delegate_assignment),
        )
        // Automation setup-agent tools; caller identity is header-derived.
        .route("/api/get_automation", post(get_automation))
        .route("/api/update_automation", post(update_automation))
        .route(
            "/api/verify_automation_decomposition",
            post(verify_automation_decomposition),
        )
        .route("/api/finalize_automation", post(finalize_automation))
        .route("/api/run_automation_now", post(run_automation_now))
        .route(
            "/api/pause_automation",
            post(pause_automation_for_setup_agent),
        )
        .route(
            "/api/resume_automation",
            post(resume_automation_for_setup_agent),
        )
        .route(
            "/api/cancel_automation_run",
            post(cancel_latest_automation_run),
        )
        .route(
            "/api/cancel_automation",
            post(cancel_automation_for_setup_agent),
        )
        .route(
            "/api/restart_automation",
            post(restart_automation_for_setup_agent),
        )
        .route(
            "/api/retry_automation_judge",
            post(retry_automation_judge_for_setup_agent),
        )
        .route(
            "/api/retry_automation_plan_judge",
            post(retry_automation_plan_judge_for_setup_agent),
        )
        .route(
            "/api/skip_automation_judge",
            post(skip_latest_automation_judge),
        )
        .route(
            "/api/get_automation_publish_status",
            post(get_automation_publish_status),
        )
        .route(
            "/api/check_automation_publish_readiness",
            post(check_automation_publish_readiness),
        )
        .route(
            "/api/update_automation_from_base",
            post(update_automation_from_base),
        )
        .route(
            "/api/publish_automation_workspace",
            post(publish_automation_workspace),
        )
        // Task tools (ralphx-chat-task agent)
        .route("/api/update_task", post(update_task))
        .route("/api/add_task_note", post(add_task_note))
        .route("/api/get_task_details", post(get_task_details))
        // Project tools (ralphx-chat-project agent)
        .route("/api/list_tasks", post(list_tasks))
        .route("/api/suggest_task", post(suggest_task))
        .route(
            "/api/append_task_to_ideation_plan",
            post(append_ideation_plan_task_http),
        )
        .route(
            "/api/create_followup_agent_conversation",
            post(create_followup_agent_conversation),
        )
        .route("/api/register_agent_issue", post(register_agent_issue))
        .route(
            "/api/agent_conversation_issues/list",
            post(list_agent_conversation_issues),
        )
        .route(
            "/api/agent_conversation_issues/status",
            post(update_agent_conversation_issue_status),
        )
        .route(
            "/api/agent_conversation_issues/convert_followup",
            post(convert_agent_conversation_issue_followup),
        )
        // Review tools (reviewer agent)
        .route("/api/complete_review", post(complete_review))
        .route("/api/review_notes/:task_id", get(get_review_notes))
        // Review chat tools (review-chat agent) - post-review human decision
        .route("/api/approve_task", post(approve_task))
        .route("/api/request_task_changes", post(request_task_changes))
        // Review issue tools (worker + reviewer agents)
        .route("/api/task_issues/:task_id", get(get_task_issues_http))
        .route(
            "/api/ticket_attachments/list",
            post(list_ticket_attachments_http),
        )
        .route(
            "/api/ticket_attachments/fetch",
            post(fetch_ticket_attachment_http),
        )
        // Atlassian MCP tools (role-tiered; enforcement re-derives the tier per
        // request from the run's persisted routing role and project).
        .route(
            "/api/atlassian-mcp/jira/search",
            post(atlassian_mcp::jira::jira_search_issues),
        )
        .route(
            "/api/atlassian-mcp/jira/issue",
            post(atlassian_mcp::jira::jira_get_issue),
        )
        .route(
            "/api/atlassian-mcp/jira/projects",
            post(atlassian_mcp::jira::jira_list_projects),
        )
        .route(
            "/api/atlassian-mcp/jira/transitions",
            post(atlassian_mcp::jira::jira_list_transitions),
        )
        .route(
            "/api/atlassian-mcp/jira/issue/create",
            post(atlassian_mcp::jira::jira_create_issue),
        )
        .route(
            "/api/atlassian-mcp/jira/issue/update",
            post(atlassian_mcp::jira::jira_update_issue),
        )
        .route(
            "/api/atlassian-mcp/jira/issue/comment",
            post(atlassian_mcp::jira::jira_add_comment),
        )
        .route(
            "/api/atlassian-mcp/jira/issue/transition",
            post(atlassian_mcp::jira::jira_transition_issue),
        )
        .route(
            "/api/atlassian-mcp/jira/issue/assign",
            post(atlassian_mcp::jira::jira_assign_issue),
        )
        .route(
            "/api/atlassian-mcp/confluence/search",
            post(atlassian_mcp::confluence::confluence_search_pages),
        )
        .route(
            "/api/atlassian-mcp/confluence/page",
            post(atlassian_mcp::confluence::confluence_get_page),
        )
        .route(
            "/api/atlassian-mcp/confluence/page/create",
            post(atlassian_mcp::confluence::confluence_create_page),
        )
        .route(
            "/api/atlassian-mcp/confluence/page/update",
            post(atlassian_mcp::confluence::confluence_update_page),
        )
        .route(
            "/api/atlassian-mcp/request",
            post(atlassian_mcp::raw::atlassian_api_request),
        )
        .route("/api/issue_progress/:task_id", get(get_issue_progress_http))
        .route(
            "/api/mark_issue_in_progress",
            post(mark_issue_in_progress_http),
        )
        .route("/api/mark_issue_addressed", post(mark_issue_addressed_http))
        // Worker context tools (worker agent)
        .route("/api/task_context/:task_id", get(get_task_context))
        .route("/api/task_validation/run", post(run_task_validation_http))
        .route(
            "/api/task_validation/summary/:task_id",
            get(get_task_validation_summary_http),
        )
        .route(
            "/api/task_validation/diff",
            post(get_validation_task_diff_http),
        )
        .route(
            "/api/task_validation/diff_stat",
            post(get_validation_task_diff_stat_http),
        )
        .route("/api/artifact/:artifact_id", get(get_artifact_full))
        .route(
            "/api/artifact/:artifact_id/version/:version",
            get(get_artifact_version),
        )
        .route(
            "/api/artifact/:artifact_id/related",
            get(get_related_artifacts),
        )
        .route("/api/artifacts/search", post(search_artifacts))
        // Task step endpoints (worker agent)
        .route("/api/task_steps/:task_id", get(get_task_steps_http))
        .route("/api/start_step", post(start_step_http))
        .route("/api/complete_step", post(complete_step_http))
        .route("/api/skip_step", post(skip_step_http))
        .route("/api/fail_step", post(fail_step_http))
        .route("/api/add_step", post(add_step_http))
        .route("/api/step_progress/:task_id", get(get_step_progress_http))
        .route("/api/step_context/:step_id", get(get_step_context_http))
        .route("/api/sub_steps/:parent_step_id", get(get_sub_steps_http))
        // Permission bridge endpoints
        .route("/api/permission/request", post(request_permission))
        .route("/api/permission/await/:request_id", get(await_permission))
        .route("/api/permission/resolve", post(resolve_permission))
        // Question bridge endpoints (AskUserQuestion)
        .route("/api/question/request", post(request_question))
        .route("/api/question/await/:request_id", get(await_question))
        .route("/api/question/resolve", post(resolve_question))
        // Git merge endpoints (merger agent)
        .route("/api/git/tasks/:id/complete-merge", post(complete_merge))
        .route(
            "/api/branch-updates/tasks/:id/context",
            get(get_branch_update_context),
        )
        .route(
            "/api/branch-updates/tasks/:id/complete",
            post(complete_branch_update),
        )
        .route(
            "/api/branch-updates/tasks/:id/report-conflict",
            post(report_branch_update_conflict),
        )
        .route(
            "/api/branch-updates/tasks/:id/report-incomplete",
            post(report_branch_update_incomplete),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/complete-repair",
            post(complete_agent_workspace_repair),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/publish-status",
            get(get_agent_workspace_publish_status),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/publish-readiness",
            get(check_agent_workspace_publish_readiness),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/update-from-base",
            post(update_agent_workspace_from_base),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/publish",
            post(publish_agent_workspace),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/commit-local",
            post(commit_agent_workspace_locally_handler),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/pr-fix-context",
            get(get_agent_workspace_pr_fix_context),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/pr-review-context",
            get(get_agent_workspace_pr_review_context),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/pr-review-settings",
            put(update_agent_workspace_pr_review_settings),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/workspace-review-context",
            get(get_agent_workspace_review_context),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/workspace-review-files",
            get(list_agent_workspace_review_files),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/workspace-review-diff-page",
            get(get_agent_workspace_review_diff_page),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/workspace-review-start-preview",
            get(get_agent_workspace_review_start_preview),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/workspace-review-runs",
            post(start_agent_workspace_review_run),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/workspace-review-fixer-runs",
            post(start_agent_workspace_review_fixer_run),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/workspace-review-approve-anyway",
            post(approve_agent_workspace_review_anyway_handler),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/workspace-review-artifact",
            post(write_agent_workspace_review_artifact),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/workspace-review-hunk-annotations",
            post(write_agent_workspace_review_hunk_annotations),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/complete-workspace-review-run",
            post(complete_agent_workspace_review_run),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/pr-review-artifact",
            post(write_agent_workspace_pr_review_artifact),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/pr-review-actions",
            post(propose_agent_workspace_pr_review_action),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/complete-pr-review-run",
            post(complete_agent_workspace_pr_review_run),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/pr-review-actions/:action_id/submit",
            post(submit_agent_workspace_pr_review_action),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/pr-review-actions/:action_id/skip",
            post(skip_agent_workspace_pr_review_action),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/pr-comments/:comment_id",
            get(read_agent_workspace_pr_comment),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/complete-pr-fix",
            post(complete_agent_workspace_pr_fix),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/pr-description",
            post(submit_agent_workspace_pr_description),
        )
        // Agent workspace diff endpoints (Extension A — staged/unstaged; Extension B — cumulative)
        .route(
            "/api/agent-workspaces/:conversation_id/staged-changes",
            get(get_agent_workspace_staged_file_changes),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/unstaged-changes",
            get(get_agent_workspace_unstaged_file_changes),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/staged-changes/*file_path",
            get(get_agent_workspace_staged_file_diff),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/unstaged-changes/*file_path",
            get(get_agent_workspace_unstaged_file_diff),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/cumulative-changes",
            get(get_agent_workspace_cumulative_file_changes),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/cumulative-changes/*file_path",
            get(get_agent_workspace_cumulative_file_diff),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/file-content-range",
            get(get_agent_workspace_file_content_range),
        )
        .route(
            "/api/agent-workspaces/:conversation_id/file-diff-page",
            get(get_agent_workspace_file_diff_page),
        )
        .route("/api/git/tasks/:id/report-conflict", post(report_conflict))
        .route(
            "/api/git/tasks/:id/report-incomplete",
            post(report_incomplete),
        )
        .route("/api/git/tasks/:id/commits", get(get_task_commits))
        .route("/api/git/tasks/:id/diff-stats", get(get_task_diff_stats))
        .route("/api/git/tasks/:id/merge-target", get(get_merge_target))
        // Project analysis endpoints (ralphx-project-analyzer + worker/reviewer/merger agents)
        .route("/api/projects/:id/analysis", get(get_project_analysis))
        .route("/api/projects/:id/analysis", post(save_project_analysis))
        // Execution complete endpoint (worker agent exit signal)
        .route(
            "/api/execution/tasks/:task_id/complete",
            post(execution_complete_http),
        )
        // Execution settings endpoints (Phase 82)
        .route("/api/execution/global-settings", get(get_global_settings))
        .route(
            "/api/execution/global-settings",
            post(update_global_settings),
        )
        // Memory tools (read + write; access restricted via MCP allowlist)
        .route("/api/search_memories", post(search_memories))
        .route("/api/get_memory", post(get_memory))
        .route("/api/get_memories_for_paths", post(get_memories_for_paths))
        .route("/api/upsert_memories", post(upsert_memories))
        .route("/api/mark_memory_obsolete", post(mark_memory_obsolete))
        .route(
            "/api/refresh_memory_rule_index",
            post(refresh_memory_rule_index),
        )
        .route("/api/ingest_rule_file", post(ingest_rule_file))
        .route(
            "/api/rebuild_archive_snapshots",
            post(rebuild_archive_snapshots),
        )
        .route(
            "/api/get_conversation_transcript",
            post(get_conversation_transcript),
        )
        // Conversation active state endpoint (streaming state hydration)
        .route(
            "/api/conversations/:id/active-state",
            get(get_conversation_active_state),
        )
        // External API endpoints (Phase 4 — external MCP server consumers)
        .route(
            "/api/external/projects",
            get(list_projects_http).post(register_project_external),
        )
        .route(
            "/api/external/project/:id/status",
            get(get_project_status_http),
        )
        .route("/api/external/start_ideation", post(start_ideation_http))
        .route(
            "/api/external/ideation_status/:id",
            get(get_ideation_status_http),
        )
        .route(
            "/api/external/sessions/:project_id",
            get(list_ideation_sessions_http),
        )
        .route(
            "/api/external/sessions/:session_id/tasks",
            get(get_session_tasks_http).post(append_session_task_http),
        )
        .route(
            "/api/external/pipeline/:project_id",
            get(get_pipeline_overview_http),
        )
        .route("/api/external/events/poll", get(poll_events_http))
        .route("/api/external/events/stream", get(stream_events_http))
        .route(
            "/api/external/attention/:project_id",
            get(get_attention_items_http),
        )
        .route(
            "/api/external/execution_capacity/:project_id",
            get(get_execution_capacity_http),
        )
        .route(
            "/api/external/task_transition",
            post(external_task_transition_http),
        )
        .route("/api/external/task/:id", get(get_task_detail_http))
        .route("/api/external/task/:id/diff", get(get_task_diff_http))
        .route(
            "/api/external/task/:id/review_summary",
            get(get_task_review_summary_http),
        )
        .route(
            "/api/external/merge_pipeline/:project_id",
            get(get_merge_pipeline_http),
        )
        .route("/api/external/review_action", post(review_action_http))
        .route(
            "/api/external/apply_proposals",
            post(external_apply_proposals),
        )
        .route(
            "/api/external/ideation_message",
            post(ideation_message_http),
        )
        .route(
            "/api/external/trigger_verification",
            post(trigger_verification_http),
        )
        .route(
            "/api/external/plan_verification/:session_id",
            get(get_plan_verification_external_http),
        )
        .route(
            "/api/external/ideation_messages/:session_id",
            get(get_ideation_messages_http),
        )
        .route(
            "/api/external/tasks/batch_status",
            post(batch_task_status_http),
        )
        .route(
            "/api/external/webhooks/register",
            post(register_webhook_http),
        )
        .route(
            "/api/external/webhooks/:id",
            delete(unregister_webhook_http),
        )
        .route("/api/external/webhooks", get(list_webhooks_http))
        .route(
            "/api/external/webhooks/health",
            get(get_webhook_health_http),
        )
        .route(
            "/api/integrations/linear/webhook",
            post(receive_linear_webhook_http),
        )
        .route("/api/external/task-note", post(create_task_note_http))
        // RX-native delegated-agent artifact endpoints.
        .route("/api/team/artifact", post(create_team_artifact))
        .route("/api/team/artifacts/:session_id", get(get_team_artifacts))
        // Permissive CORS applied only to public routes — does NOT apply to
        // internal_routes (which need no CORS) or management_routes (which have
        // their own restrictive CorsLayer already).
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    let app = Router::new()
        .merge(internal_routes)
        .merge(public_routes)
        .with_state(state);

    let bind_addr = backend_http_bind_addr();
    let listener = match bind_with_retry(&bind_addr, 5, Duration::from_millis(250)).await {
        Ok(listener) => {
            if let Some(sender) = listener_ready.take() {
                let _ = sender.send(Ok(()));
            }
            listener
        }
        Err(error) => {
            if let Some(sender) = listener_ready.take() {
                let _ = sender.send(Err(crate::AppError::Infrastructure(error.to_string())));
            }
            return Err(error);
        }
    };

    tracing::info!(url = %backend_http_base_url(), "MCP HTTP server listening");

    // Graceful shutdown: when triggered, axum stops accepting new connections,
    // closes idle keep-alive sockets, and lets in-flight requests drain. The
    // Tauri shutdown handler fires this on `RunEvent::ExitRequested` so
    // sockets close cleanly before the process is reaped — reduces orphaned
    // TIME_WAIT pileups that exhaust the macOS ephemeral port pool.
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.wait_for_shutdown().await;
            tracing::info!("MCP HTTP server received graceful shutdown signal");
        })
        .await
        .map_err(|e| {
            crate::error::AppError::Infrastructure(format!("HTTP server crashed: {}", e))
        })?;

    tracing::info!("MCP HTTP server shut down cleanly");
    Ok(())
}

async fn bind_with_retry(
    address: &str,
    attempts: usize,
    delay: Duration,
) -> AppResult<tokio::net::TcpListener> {
    for attempt in 1..=attempts {
        match tokio::net::TcpListener::bind(address).await {
            Ok(listener) => return Ok(listener),
            Err(e) if attempt < attempts => {
                tracing::warn!(
                    "Failed to bind HTTP server to {} (attempt {}/{}): {}",
                    address,
                    attempt,
                    attempts,
                    e
                );
                tokio::time::sleep(delay).await;
            }
            Err(e) => {
                return Err(crate::error::AppError::Infrastructure(format!(
                    "Failed to bind HTTP server to {} after {} attempts: {}",
                    address, attempts, e
                )));
            }
        }
    }

    Err(crate::error::AppError::Infrastructure(format!(
        "Failed to bind HTTP server to {} after {} attempts",
        address, attempts
    )))
}
