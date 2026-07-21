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
    if workspace.has_terminal_publication_pr_status() {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Pull request is already merged or closed",
            None,
        ));
    }
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
    let health =
        fetch_review_pr_health_for_mutation(&workspace, pr_number, github.as_ref()).await?;
    if reconcile_terminal_review_pr_health(state.app_state.as_ref(), &workspace, pr_number, &health)
        .await?
    {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Pull request is already merged or closed",
            None,
        ));
    }
    let current_head_sha = health.sync_state.head_ref_oid;
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

    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Submitting;
    monitor.last_error = None;
    let transition = state
        .app_state
        .agent_conversation_workspace_repo
        .transition_pr_review_state_if_nonterminal(
            monitor,
            Some(AgentWorkspacePrReviewActionMutation::CompareAndSet {
                action_id: action.id.clone(),
                expected: AgentWorkspacePrReviewActionStatus::Pending,
                status: AgentWorkspacePrReviewActionStatus::Submitting,
                submitted_review_id: None,
            }),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let claim_conflict = json_error(
        StatusCode::CONFLICT,
        "PR review action is already being submitted",
        None,
    );
    let Some(transition) = transition else {
        return Err(claim_conflict);
    };
    let monitor = transition.monitor;
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
            let mut retry_monitor =
                monitor_for_retryable_submission_failure(monitor, error_message.clone());
            retry_monitor.last_seen_head_sha = Some(action.head_sha.clone());
            let restored = state
                .app_state
                .agent_conversation_workspace_repo
                .transition_pr_review_state_if_nonterminal(
                    retry_monitor,
                    Some(AgentWorkspacePrReviewActionMutation::CompareAndSet {
                        action_id: action.id.clone(),
                        expected: AgentWorkspacePrReviewActionStatus::Submitting,
                        status: AgentWorkspacePrReviewActionStatus::Pending,
                        submitted_review_id: None,
                    }),
                )
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            let Some(restored) = restored else {
                return Err(json_error(
                    StatusCode::CONFLICT,
                    "PR review action was superseded while submission was in flight",
                    Some(error_message),
                ));
            };
            let retry_monitor = restored.monitor;
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
    let mut final_monitor = monitor;
    final_monitor.first_review_completed = true;
    final_monitor.first_action_resolved = true;
    final_monitor.last_seen_head_sha = Some(action.head_sha.clone());
    final_monitor.last_reviewed_head_sha = Some(action.head_sha.clone());
    final_monitor.last_review_outcome = Some(action_kind.to_string());
    final_monitor.last_submitted_review_id = Some(submitted.id.clone());
    final_monitor.last_error = None;
    final_monitor.status = final_monitor.settlement_status();
    let finalized = state
        .app_state
        .agent_conversation_workspace_repo
        .transition_pr_review_state_if_nonterminal(
            final_monitor,
            Some(AgentWorkspacePrReviewActionMutation::CompareAndSet {
                action_id: action.id.clone(),
                expected: AgentWorkspacePrReviewActionStatus::Submitting,
                status: AgentWorkspacePrReviewActionStatus::Submitted,
                submitted_review_id: Some(submitted.id.clone()),
            }),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let Some(finalized) = finalized else {
        return Err(json_error(
            StatusCode::CONFLICT,
            "PR review action was superseded while submission was in flight",
            Some(format!("GitHub review {} was submitted", submitted.id)),
        ));
    };
    let monitor = finalized.monitor;
    maybe_start_pr_review_monitor_polling(state.app_state.as_ref(), &workspace, &monitor).await;
    let action = finalized.action.ok_or_else(|| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Review PR submission transition did not return its action",
            None,
        )
    })?;
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
    if workspace.has_terminal_publication_pr_status() {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Pull request is already merged or closed",
            None,
        ));
    }
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
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        action.pr_number,
        Some(action.head_sha.clone()),
        true,
    )
    .await?;
    monitor.first_review_completed = true;
    monitor.first_action_resolved = true;
    monitor.last_seen_head_sha = Some(action.head_sha.clone());
    monitor.last_reviewed_head_sha = Some(action.head_sha.clone());
    monitor.last_review_outcome = Some("skipped".to_string());
    monitor.last_error = req.reason;
    monitor.status = if monitor.monitor_enabled {
        AgentWorkspacePrReviewMonitorStatus::Watching
    } else {
        AgentWorkspacePrReviewMonitorStatus::Paused
    };
    let transition = state
        .app_state
        .agent_conversation_workspace_repo
        .transition_pr_review_state_if_nonterminal(
            monitor,
            Some(AgentWorkspacePrReviewActionMutation::CompareAndSet {
                action_id: action.id.clone(),
                expected: AgentWorkspacePrReviewActionStatus::Pending,
                status: AgentWorkspacePrReviewActionStatus::Skipped,
                submitted_review_id: None,
            }),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let transition = transition.ok_or_else(|| {
        json_error(
            StatusCode::CONFLICT,
            "PR review action is no longer pending",
            None,
        )
    })?;
    let monitor = transition.monitor;
    maybe_start_pr_review_monitor_polling(state.app_state.as_ref(), &workspace, &monitor).await;
    let action = transition.action.ok_or_else(|| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Review PR skip transition did not return its action",
            None,
        )
    })?;
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
