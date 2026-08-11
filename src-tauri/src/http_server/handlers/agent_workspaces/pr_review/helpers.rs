use super::*;

pub(in crate::http_server::handlers::agent_workspaces) fn review_pr_number(
    workspace: &AgentConversationWorkspace,
) -> Option<i64> {
    workspace
        .source_pull_request
        .as_ref()
        .map(|pull_request| pull_request.number)
        .or(workspace.publication_pr_number)
}

pub(in crate::http_server::handlers::agent_workspaces) fn review_pr_url(
    workspace: &AgentConversationWorkspace,
) -> Option<String> {
    workspace
        .source_pull_request
        .as_ref()
        .and_then(|pull_request| pull_request.url.clone())
        .or_else(|| workspace.publication_pr_url.clone())
}

pub(in crate::http_server::handlers::agent_workspaces) fn review_pr_head_sha(
    workspace: &AgentConversationWorkspace,
) -> Option<String> {
    workspace
        .source_pull_request
        .as_ref()
        .and_then(|pull_request| pull_request.head_ref_oid.clone())
}

pub(in crate::http_server::handlers::agent_workspaces) async fn maybe_start_pr_review_monitor_polling(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspacePrReviewMonitor,
) {
    if let Err(error) =
        crate::application::services::pr_merge_poller::start_review_pr_lifecycle_polling(
            state, workspace, monitor,
        )
        .await
    {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            error = %error,
            "Review PR lifecycle polling could not start"
        );
    }
}

pub(in crate::http_server::handlers::agent_workspaces) async fn fetch_review_pr_remote_context(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
) -> Result<(Option<PrHealth>, Option<PrReviewFeedback>), JsonError> {
    let Some(github) = state.github_service.as_ref() else {
        return Ok((None, None));
    };
    let working_dir = std::path::Path::new(&workspace.worktree_path);
    let health = github.fetch_pr_health(working_dir, pr_number).await.ok();
    if let Some(health) = health.as_ref() {
        import_agent_workspace_pr_comment_evidence(
            Arc::clone(&state.agent_conversation_workspace_repo),
            &workspace.conversation_id,
            pr_number,
            health,
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    }
    let review_feedback = github
        .check_pr_review_feedback(working_dir, pr_number)
        .await
        .ok()
        .flatten();
    Ok((health, review_feedback))
}

pub(in crate::http_server::handlers::agent_workspaces) async fn fetch_review_pr_health_for_mutation(
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
    github: &dyn GithubServiceTrait,
) -> Result<PrHealth, JsonError> {
    github
        .fetch_pr_health(std::path::Path::new(&workspace.worktree_path), pr_number)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::BAD_GATEWAY,
                "Could not verify the current pull request state",
                Some(error.to_string()),
            )
        })
}

pub(in crate::http_server::handlers::agent_workspaces) async fn reconcile_terminal_review_pr_health(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
    health: &PrHealth,
) -> Result<bool, JsonError> {
    let (status, summary) = match &health.sync_state.status {
        PrStatus::Merged { .. } => ("merged", "Pull request merged"),
        PrStatus::Closed => ("closed", "Pull request closed without merging"),
        PrStatus::Open => return Ok(false),
    };
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Project not found", None))?;
    let chat_service: Arc<dyn crate::application::chat_service::ChatService> =
        Arc::new(state.build_chat_service());
    let outcome = crate::application::agent_workspace_terminal_cleanup::settle_review_pr_terminal_observation(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        Some(Arc::clone(&state.plan_branch_repo)),
        Some(chat_service),
        Some(state.notification_service()),
        &workspace.conversation_id,
        &project,
        pr_number,
        status,
        summary,
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    if let Err(error) = outcome.require_runtime_shutdown() {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            pr_number,
            error,
            "Review PR terminal authority committed while local cleanup remains pending"
        );
    }
    state
        .pr_poller_registry
        .stop_agent_workspace_polling(&workspace.conversation_id);
    Ok(true)
}

pub(in crate::http_server::handlers::agent_workspaces) async fn load_or_create_pr_review_monitor(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
    head_sha: Option<String>,
    enable_new_monitor: bool,
) -> Result<AgentWorkspacePrReviewMonitor, JsonError> {
    let existing = state
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&workspace.conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    Ok(existing.unwrap_or_else(|| {
        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            workspace.conversation_id.clone(),
            workspace.project_id.clone(),
            pr_number,
            head_sha,
        );
        if enable_new_monitor {
            monitor.monitor_enabled = true;
            monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
        }
        monitor
    }))
}

pub(in crate::http_server::handlers::agent_workspaces) fn ensure_review_artifact_for_head(
    monitor: &AgentWorkspacePrReviewMonitor,
    head_sha: &str,
) -> Result<(), JsonError> {
    let has_matching_artifact = monitor.review_artifact_id.is_some()
        && monitor.review_artifact_head_sha.as_deref() == Some(head_sha);
    if has_matching_artifact {
        return Ok(());
    }

    Err(json_error(
        StatusCode::CONFLICT,
        "Write the Review for the current PR head before proposing or submitting a PR review action",
        None,
    ))
}

pub(in crate::http_server::handlers::agent_workspaces) fn pr_review_submission_event(
    action_kind: AgentWorkspacePrReviewActionKind,
) -> PrReviewSubmissionEvent {
    match action_kind {
        AgentWorkspacePrReviewActionKind::RequestChanges => PrReviewSubmissionEvent::RequestChanges,
        AgentWorkspacePrReviewActionKind::Approve => PrReviewSubmissionEvent::Approve,
        AgentWorkspacePrReviewActionKind::Comment => PrReviewSubmissionEvent::Comment,
    }
}

pub(in crate::http_server::handlers::agent_workspaces) fn monitor_for_retryable_submission_failure(
    mut monitor: AgentWorkspacePrReviewMonitor,
    error: String,
) -> AgentWorkspacePrReviewMonitor {
    monitor.last_error = Some(error);
    monitor.status = if monitor.monitor_enabled {
        AgentWorkspacePrReviewMonitorStatus::AwaitingUser
    } else {
        AgentWorkspacePrReviewMonitorStatus::Paused
    };
    monitor
}
