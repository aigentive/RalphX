// HTTP server for MCP proxy - exposes Tauri commands via HTTP
// This allows the MCP server to call RalphX functionality via REST API

use axum::{
    http::StatusCode,
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

use crate::application::{AppState, TeamService, TeamStateTracker};
use crate::commands::ExecutionState;
use crate::error::AppResult;
use crate::utils::backend_endpoint::{backend_http_base_url, backend_http_bind_addr};
use delegation::DelegationService;

// ============================================================================
// Submodules
// ============================================================================

pub mod handlers;
pub mod delegation;
pub mod helpers;
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

pub async fn start_http_server(
    app_state: Arc<AppState>,
    execution_state: Arc<ExecutionState>,
    team_tracker: TeamStateTracker,
) -> AppResult<()> {
    // Build TeamService for HTTP handlers (wraps tracker with DB persistence + events)
    let team_service = {
        let tracker_arc = Arc::new(team_tracker.clone());
        match &app_state.app_handle {
            Some(handle) => Arc::new(TeamService::new_with_repos(
                tracker_arc,
                handle.clone(),
                app_state.team_session_repo.clone(),
                app_state.team_message_repo.clone(),
            )),
            None => Arc::new(TeamService::new_without_events(tracker_arc)),
        }
    };

    let state = HttpServerState {
        app_state,
        execution_state,
        team_tracker,
        team_service,
        delegation_service: Arc::new(DelegationService::new()),
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
        .route("/api/auth/keys/:id/permissions", put(update_key_permissions))
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
        .route(
            "/api/internal/projects",
            get(list_projects_internal),
        )
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
        .route("/api/create_plan_artifact", post(create_plan_artifact))
        .route("/api/update_plan_artifact", post(update_plan_artifact))
        .route("/api/edit_plan_artifact", post(edit_plan_artifact))
        .route(
            "/api/artifact/:artifact_id/history",
            get(get_artifact_history),
        )
        .route("/api/link_proposals_to_plan", post(link_proposals_to_plan))
        .route("/api/get_session_plan/:session_id", get(get_session_plan))
        // Plan verification tools (ralphx-ideation + worker agents)
        .route(
            "/api/ideation/sessions/:id/verification",
            post(post_verification_status),
        )
        .route(
            "/api/ideation/sessions/:id/verification",
            get(get_plan_verification),
        )
        .route(
            "/api/ideation/sessions/:id/verification/infra-failure",
            post(mark_verification_infra_failure),
        )
        .route(
            "/api/ideation/sessions/:id/revert-and-skip",
            post(revert_and_skip),
        )
        .route(
            "/api/ideation/sessions/:id/stop-verification",
            post(stop_verification),
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
        .route(
            "/api/verification/confirm",
            post(confirm_verification),
        )
        .route(
            "/api/verification/dismiss",
            post(dismiss_verification),
        )
        .route(
            "/api/verification/auto-accept",
            post(set_auto_accept_verification),
        )
        .route(
            "/api/verification/specialists",
            get(get_verification_specialists),
        )
        .route(
            "/api/verification/confirmation-status/:session_id",
            get(get_confirmation_status),
        )
        .route(
            "/api/verification/pending-confirmations",
            get(get_pending_verification_confirmations),
        )
        // Child session tools (ralphx-ideation + ralphx-ideation-team-lead + ralphx-plan-verifier agents)
        .route(
            "/api/ideation/sessions/:id/child-status",
            get(get_child_session_status_handler),
        )
        .route(
            "/api/ideation/sessions/:id/message",
            post(send_ideation_session_message_handler),
        )
        .route("/api/coordination/delegate/start", post(start_delegate))
        .route(
            "/api/coordination/delegated-session/:id/status",
            get(get_delegated_session_status),
        )
        .route("/api/coordination/delegate/wait", post(wait_delegate))
        .route("/api/coordination/delegate/cancel", post(cancel_delegate))
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
        // Review tools (reviewer agent)
        .route("/api/complete_review", post(complete_review))
        .route("/api/review_notes/:task_id", get(get_review_notes))
        // Review chat tools (review-chat agent) - post-review human decision
        .route("/api/approve_task", post(approve_task))
        .route("/api/request_task_changes", post(request_task_changes))
        // Review issue tools (worker + reviewer agents)
        .route("/api/task_issues/:task_id", get(get_task_issues_http))
        .route("/api/issue_progress/:task_id", get(get_issue_progress_http))
        .route(
            "/api/mark_issue_in_progress",
            post(mark_issue_in_progress_http),
        )
        .route("/api/mark_issue_addressed", post(mark_issue_addressed_http))
        // Worker context tools (worker agent)
        .route("/api/task_context/:task_id", get(get_task_context))
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
            "/api/agent-workspaces/:conversation_id/complete-repair",
            post(complete_agent_workspace_repair),
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
        .route("/api/external/project/:id/status", get(get_project_status_http))
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
        .route("/api/external/webhooks/:id", delete(unregister_webhook_http))
        .route("/api/external/webhooks", get(list_webhooks_http))
        .route("/api/external/webhooks/health", get(get_webhook_health_http))
        .route(
            "/api/external/task-note",
            post(create_task_note_http),
        )
        // Team endpoints (agent teams) — two-phase plan flow
        .route("/api/team/plan/request", post(request_team_plan_register))
        .route("/api/team/plan/await/:plan_id", get(await_team_plan))
        .route("/api/team/plan/pending/:context_id", get(get_pending_plan))
        .route("/api/team/plan/approve", post(approve_team_plan))
        .route("/api/team/plan/reject", post(reject_team_plan))
        .route("/api/team/spawn", post(request_teammate_spawn))
        .route("/api/team/artifact", post(create_team_artifact))
        .route(
            "/api/team/verification_finding",
            post(publish_verification_finding),
        )
        .route("/api/team/artifacts/:session_id", get(get_team_artifacts))
        .route(
            "/api/team/verification-findings/:session_id",
            get(get_verification_findings),
        )
        .route(
            "/api/team/session_state/:session_id",
            get(get_team_session_state),
        )
        .route("/api/team/session_state", post(save_team_session_state))
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
    let listener = bind_with_retry(&bind_addr, 5, Duration::from_millis(250)).await?;

    tracing::info!(url = %backend_http_base_url(), "MCP HTTP server listening");

    axum::serve(listener, app).await.map_err(|e| {
        crate::error::AppError::Infrastructure(format!("HTTP server crashed: {}", e))
    })?;

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
