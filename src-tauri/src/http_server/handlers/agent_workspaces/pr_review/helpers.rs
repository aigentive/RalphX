use super::*;

pub(super) fn review_pr_number(workspace: &AgentConversationWorkspace) -> Option<i64> {
    workspace
        .source_pull_request
        .as_ref()
        .map(|pull_request| pull_request.number)
        .or(workspace.publication_pr_number)
}

pub(super) fn review_pr_url(workspace: &AgentConversationWorkspace) -> Option<String> {
    workspace
        .source_pull_request
        .as_ref()
        .and_then(|pull_request| pull_request.url.clone())
        .or_else(|| workspace.publication_pr_url.clone())
}

pub(super) fn review_pr_head_sha(workspace: &AgentConversationWorkspace) -> Option<String> {
    workspace
        .source_pull_request
        .as_ref()
        .and_then(|pull_request| pull_request.head_ref_oid.clone())
}

pub(super) async fn maybe_start_pr_review_monitor_polling(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspacePrReviewMonitor,
) {
    if workspace.mode != AgentConversationWorkspaceMode::ReviewPr
        || !monitor.monitor_enabled
        || matches!(
            monitor.status,
            AgentWorkspacePrReviewMonitorStatus::Paused
                | AgentWorkspacePrReviewMonitorStatus::Terminal
        )
    {
        return;
    }
    if state
        .pr_poller_registry
        .is_agent_workspace_polling(&workspace.conversation_id)
    {
        return;
    }

    let Some(pr_number) = review_pr_number(workspace) else {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            "Review PR monitor could not start because the workspace has no PR number"
        );
        return;
    };
    if monitor.pr_number != pr_number {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            monitor_pr_number = monitor.pr_number,
            workspace_pr_number = pr_number,
            "Review PR monitor could not start because monitor/workspace PR numbers differ"
        );
        return;
    }

    let project = match state.project_repo.get_by_id(&workspace.project_id).await {
        Ok(Some(project)) => project,
        Ok(None) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                project_id = workspace.project_id.as_str(),
                "Review PR monitor could not start because the project was not found"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                project_id = workspace.project_id.as_str(),
                error = %error,
                "Review PR monitor failed to load project before poller start"
            );
            return;
        }
    };
    let worktree_path =
        match resolve_valid_agent_conversation_workspace_path(&project, workspace).await {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Review PR monitor could not start because the workspace path is not usable"
                );
                return;
            }
        };

    let chat_service: Arc<dyn crate::application::chat_service::ChatService> =
        Arc::new(state.build_chat_service());
    state.pr_poller_registry.start_agent_workspace_polling(
        workspace.conversation_id.clone(),
        pr_number,
        project,
        worktree_path,
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        chat_service,
    );
}

pub(super) async fn fetch_review_pr_remote_context(
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

pub(super) async fn fetch_current_review_pr_head_sha(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
    github: &dyn GithubServiceTrait,
) -> Result<Option<String>, JsonError> {
    let working_dir = std::path::Path::new(&workspace.worktree_path);
    let remote_head = github
        .fetch_pr_health(working_dir, pr_number)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::BAD_GATEWAY,
                "Could not verify the current pull request head",
                Some(error.to_string()),
            )
        })?
        .sync_state
        .head_ref_oid;
    let head_sha = remote_head;
    if head_sha.is_none() {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            pr_number,
            "Review PR submit could not resolve current head SHA"
        );
    }
    let _ = state;
    Ok(head_sha)
}

pub(super) async fn load_or_create_pr_review_monitor(
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

pub(super) fn pr_review_submission_event(
    action_kind: AgentWorkspacePrReviewActionKind,
) -> PrReviewSubmissionEvent {
    match action_kind {
        AgentWorkspacePrReviewActionKind::RequestChanges => PrReviewSubmissionEvent::RequestChanges,
        AgentWorkspacePrReviewActionKind::Approve => PrReviewSubmissionEvent::Approve,
        AgentWorkspacePrReviewActionKind::Comment => PrReviewSubmissionEvent::Comment,
    }
}

pub(super) fn monitor_for_retryable_submission_failure(
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
