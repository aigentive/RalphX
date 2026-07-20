use super::*;
use crate::domain::services::append_ralphx_generated_footer;

/// POST /api/agent-workspaces/{conversation_id}/pr-review-actions/{action_id}/submit
pub async fn submit_agent_workspace_pr_review_action(
    State(state): State<HttpServerState>,
    Path((conversation_id, action_id)): Path<(String, String)>,
    Json(req): Json<SubmitAgentWorkspacePrReviewActionRequest>,
) -> Result<Json<SubmitAgentWorkspacePrReviewActionResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let action = state
        .app_state
        .agent_conversation_workspace_repo
        .get_pr_review_action(&action_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "PR review action not found", None))?;
    if action.conversation_id != conversation_id {
        return Err(json_error(
            StatusCode::NOT_FOUND,
            "PR review action not found for this workspace",
            None,
        ));
    }
    if action.status != AgentWorkspacePrReviewActionStatus::Pending {
        return Err(json_error(
            StatusCode::CONFLICT,
            "PR review action is no longer pending",
            None,
        ));
    }
    let override_kind = req
        .action_kind
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(AgentWorkspacePrReviewActionKind::from_str)
        .transpose()
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, error, None))?;
    let action_kind = override_kind.unwrap_or(action.proposed_action);
    let event = pr_review_submission_event(action_kind);
    let pr_number = review_pr_number(&workspace).ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "Review PR mode requires a linked pull request",
            None,
        )
    })?;
    if action.pr_number != pr_number {
        return Err(json_error(
            StatusCode::CONFLICT,
            "PR review action belongs to a different pull request",
            None,
        ));
    }
    let github = state.app_state.github_service.as_ref().ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "GitHub review submission is unavailable",
            None,
        )
    })?;
    let current_head_sha = fetch_current_review_pr_head_sha(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        github.as_ref(),
    )
    .await?;
    if current_head_sha.as_deref() != Some(action.head_sha.as_str()) {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Pull request head changed; run a fresh review before submitting",
            None,
        ));
    }
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        Some(action.head_sha.clone()),
        true,
    )
    .await?;
    ensure_review_artifact_for_head(&monitor, &action.head_sha)?;

    let claimed = state
        .app_state
        .agent_conversation_workspace_repo
        .claim_pending_pr_review_action(&action.id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let claim_conflict = json_error(
        StatusCode::CONFLICT,
        "PR review action is already being submitted",
        None,
    );
    if !claimed {
        return Err(claim_conflict);
    }
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Submitting;
    monitor.last_error = None;
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let outbound_review_body = append_ralphx_generated_footer(&action.review_body);
    let submitted = match github
        .submit_pr_review(
            std::path::Path::new(&workspace.worktree_path),
            pr_number,
            event,
            &outbound_review_body,
        )
        .await
    {
        Ok(submitted) => submitted,
        Err(error) => {
            let error_message = error.to_string();
            state
                .app_state
                .agent_conversation_workspace_repo
                .update_pr_review_action_status(
                    &action.id,
                    AgentWorkspacePrReviewActionStatus::Pending,
                    None,
                )
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            let mut retry_monitor =
                monitor_for_retryable_submission_failure(monitor, error_message.clone());
            retry_monitor.last_seen_head_sha = Some(action.head_sha.clone());
            let retry_monitor = state
                .app_state
                .agent_conversation_workspace_repo
                .upsert_pr_review_monitor(retry_monitor)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            state
                .app_state
                .notification_service()
                .record(NewNotification {
                    project_id: Some(retry_monitor.project_id.to_string()),
                    category: NotificationCategory::PrReviewAction,
                    severity: NotificationSeverity::ActionRequired,
                    title: format!("PR #{} needs your review", retry_monitor.pr_number),
                    body: Some(
                        "A PR review action could not be submitted and needs your decision"
                            .to_string(),
                    ),
                    target: NotificationTarget {
                        kind: NotificationTargetKind::AgentConversation,
                        project_id: Some(retry_monitor.project_id.to_string()),
                        task_id: None,
                        conversation_id: Some(retry_monitor.conversation_id.to_string()),
                        setup_conversation_id: None,
                        automation_id: None,
                        run_id: None,
                    },
                    dedupe_key: Some(pr_review_notification_key(
                        retry_monitor.conversation_id.as_str(),
                        &action.id,
                    )),
                })
                .await;
            return Err(json_error(
                StatusCode::BAD_GATEWAY,
                "Failed to submit GitHub PR review",
                Some(error_message),
            ));
        }
    };
    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_review_action_status(
            &action.id,
            AgentWorkspacePrReviewActionStatus::Submitted,
            Some(&submitted.id),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        Some(action.head_sha.clone()),
        true,
    )
    .await?;
    monitor.first_review_completed = true;
    monitor.last_seen_head_sha = Some(action.head_sha.clone());
    monitor.last_reviewed_head_sha = Some(action.head_sha.clone());
    monitor.last_review_outcome = Some(action_kind.to_string());
    monitor.last_submitted_review_id = Some(submitted.id.clone());
    monitor.last_error = None;
    monitor.status = monitor.settlement_status();
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .mark_pr_review_first_action_resolved(&monitor.conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    maybe_start_pr_review_monitor_polling(state.app_state.as_ref(), &workspace, &monitor).await;
    let action = state
        .app_state
        .agent_conversation_workspace_repo
        .get_pr_review_action(&action.id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "PR review action not found", None))?;
    state
        .app_state
        .notification_service()
        .resolve_workflow_notification(&pr_review_notification_key(
            conversation_id.as_str(),
            &action.id,
        ))
        .await;

    Ok(Json(SubmitAgentWorkspacePrReviewActionResponse {
        success: true,
        monitor: AgentWorkspacePrReviewMonitorResponse::from(monitor),
        action: AgentWorkspacePrReviewActionResponse::from(action),
        submitted_review_id: submitted.id,
        submitted_review_url: submitted.url,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/pr-review-actions/{action_id}/skip
pub async fn skip_agent_workspace_pr_review_action(
    State(state): State<HttpServerState>,
    Path((conversation_id, action_id)): Path<(String, String)>,
    Json(req): Json<SkipAgentWorkspacePrReviewActionRequest>,
) -> Result<Json<SkipAgentWorkspacePrReviewActionResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let action = state
        .app_state
        .agent_conversation_workspace_repo
        .get_pr_review_action(&action_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "PR review action not found", None))?;
    if action.conversation_id != conversation_id {
        return Err(json_error(
            StatusCode::NOT_FOUND,
            "PR review action not found for this workspace",
            None,
        ));
    }
    if action.status != AgentWorkspacePrReviewActionStatus::Pending {
        return Err(json_error(
            StatusCode::CONFLICT,
            "PR review action is no longer pending",
            None,
        ));
    }
    state
        .app_state
        .agent_conversation_workspace_repo
        .update_pr_review_action_status(
            &action.id,
            AgentWorkspacePrReviewActionStatus::Skipped,
            None,
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        action.pr_number,
        Some(action.head_sha.clone()),
        true,
    )
    .await?;
    monitor.first_review_completed = true;
    monitor.last_seen_head_sha = Some(action.head_sha.clone());
    monitor.last_reviewed_head_sha = Some(action.head_sha.clone());
    monitor.last_review_outcome = Some("skipped".to_string());
    monitor.last_error = req.reason;
    monitor.status = if monitor.monitor_enabled {
        AgentWorkspacePrReviewMonitorStatus::Watching
    } else {
        AgentWorkspacePrReviewMonitorStatus::Paused
    };
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .mark_pr_review_first_action_resolved(&monitor.conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    maybe_start_pr_review_monitor_polling(state.app_state.as_ref(), &workspace, &monitor).await;
    let action = state
        .app_state
        .agent_conversation_workspace_repo
        .get_pr_review_action(&action.id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "PR review action not found", None))?;
    state
        .app_state
        .notification_service()
        .resolve_workflow_notification(&pr_review_notification_key(
            conversation_id.as_str(),
            &action.id,
        ))
        .await;

    Ok(Json(SkipAgentWorkspacePrReviewActionResponse {
        success: true,
        monitor: AgentWorkspacePrReviewMonitorResponse::from(monitor),
        action: AgentWorkspacePrReviewActionResponse::from(action),
    }))
}
