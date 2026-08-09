use super::*;

/// GET /api/agent-workspaces/{conversation_id}/pr-review-context
pub async fn get_agent_workspace_pr_review_context(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
) -> Result<Json<AgentWorkspacePrReviewContextResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let mut workspace =
        load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    let pr_number = review_pr_number(&workspace).ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "Review PR mode requires a linked pull request",
            None,
        )
    })?;
    let pr_url = review_pr_url(&workspace);
    let source_head_sha = review_pr_head_sha(&workspace);
    let (mut health, review_feedback) =
        fetch_review_pr_remote_context(state.app_state.as_ref(), &workspace, pr_number).await?;
    if let Some(health) = health.as_ref() {
        if reconcile_terminal_review_pr_health(
            state.app_state.as_ref(),
            &workspace,
            pr_number,
            health,
        )
        .await?
        {
            workspace =
                load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
        }
    }
    let workspace_response = agent_workspace_response_with_pr_supervision_for_state(
        state.app_state.as_ref(),
        &state.execution_state,
        workspace.clone(),
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error, None))?;
    let events =
        load_agent_workspace_publication_events(state.app_state.as_ref(), &conversation_id).await?;
    let verified_remote_head_sha = health
        .as_ref()
        .and_then(|health| health.sync_state.head_ref_oid.clone());
    let current_head_sha = verified_remote_head_sha.clone().or(source_head_sha);
    if let Some(health) = health.as_mut() {
        truncate_pr_health_issue_comments(health);
    }
    let issue_comment_evidence = load_agent_workspace_pr_comment_evidence(
        state.app_state.as_ref(),
        &conversation_id,
        pr_number,
    )
    .await?;
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let current_pending_action = match verified_remote_head_sha.as_deref() {
        Some(head_sha) => state
            .app_state
            .agent_conversation_workspace_repo
            .get_pending_pr_review_action_for_head(&conversation_id, pr_number, head_sha)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?,
        None => None,
    };
    let pending_action = match current_pending_action {
        Some(action) => Some(action),
        None => state
            .app_state
            .agent_conversation_workspace_repo
            .get_latest_pending_pr_review_action(&conversation_id, pr_number)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?,
    };
    let pending_action_head_status =
        pending_action
            .as_ref()
            .map(|action| match verified_remote_head_sha.as_deref() {
                Some(head_sha) if head_sha == action.head_sha => {
                    AgentWorkspacePrReviewActionHeadStatus::Current
                }
                Some(_) => AgentWorkspacePrReviewActionHeadStatus::Stale,
                None => AgentWorkspacePrReviewActionHeadStatus::Unverified,
            });
    let recent_actions = state
        .app_state
        .agent_conversation_workspace_repo
        .list_pr_review_actions(&conversation_id, 20)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;

    Ok(Json(AgentWorkspacePrReviewContextResponse {
        success: true,
        workspace: workspace_response,
        events,
        pr_number,
        pr_url,
        current_head_sha,
        health,
        review_feedback,
        monitor: monitor.map(AgentWorkspacePrReviewMonitorResponse::from),
        pending_action: pending_action.map(AgentWorkspacePrReviewActionResponse::from),
        pending_action_head_status,
        recent_actions: recent_actions
            .into_iter()
            .map(AgentWorkspacePrReviewActionResponse::from)
            .collect(),
        issue_comment_evidence,
    }))
}

/// PUT /api/agent-workspaces/{conversation_id}/pr-review-settings
pub async fn update_agent_workspace_pr_review_settings(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<UpdateAgentWorkspacePrReviewSettingsRequest>,
) -> Result<Json<UpdateAgentWorkspacePrReviewSettingsResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    if workspace.has_terminal_publication_pr_status() {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Pull request is already merged or closed",
            None,
        ));
    }
    if workspace.mode != AgentConversationWorkspaceMode::ReviewPr {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "PR Review settings are available only in Review PR workspaces",
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
    if req.auto_approve_enabled.is_none() && req.monitor_enabled.is_none() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "At least one PR Review setting is required",
            None,
        ));
    }
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        review_pr_head_sha(&workspace),
        req.monitor_enabled == Some(true),
    )
    .await?;
    if let Some(enabled) = req.auto_approve_enabled {
        monitor.auto_approve_enabled = enabled;
    }
    if let Some(enabled) = req.monitor_enabled {
        if !enabled
            && matches!(
                monitor.status,
                AgentWorkspacePrReviewMonitorStatus::Reviewing
                    | AgentWorkspacePrReviewMonitorStatus::Submitting
            )
            && req.active_review_policy.is_none()
        {
            return Err(json_error(
                StatusCode::CONFLICT,
                "active_review_choice_required",
                Some("Choose whether to finish or cancel the active PR review".to_string()),
            ));
        }
        if let Some(policy) = req.active_review_policy.as_deref() {
            if !matches!(policy, "finish_current" | "cancel_current") {
                return Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "active_review_policy must be finish_current or cancel_current",
                    None,
                ));
            }
        }
        monitor.monitor_enabled = enabled;
        if enabled {
            monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
            if let Some(head_sha) = monitor.last_seen_head_sha.as_deref() {
                let pending_action = state
                    .app_state
                    .agent_conversation_workspace_repo
                    .get_pending_pr_review_action_for_head(
                        &workspace.conversation_id,
                        monitor.pr_number,
                        head_sha,
                    )
                    .await
                    .map_err(|error| {
                        json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                    })?;
                if pending_action.is_some() {
                    monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
                }
            }
        } else {
            monitor.status = AgentWorkspacePrReviewMonitorStatus::Paused;
        }
    }
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .transition_pr_review_state_if_nonterminal(monitor, None)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| {
            json_error(
                StatusCode::CONFLICT,
                "Review PR settings changed after terminal or stale authority",
                None,
            )
        })?
        .monitor;
    if req.monitor_enabled == Some(true) {
        maybe_start_pr_review_monitor_polling(state.app_state.as_ref(), &workspace, &monitor).await;
    } else if req.monitor_enabled == Some(false)
        && req.active_review_policy.as_deref() == Some("cancel_current")
    {
        let chat_service = state.app_state.build_chat_service();
        chat_service
            .stop_agent(
                ChatContextType::Project,
                &workspace.conversation_id.as_str(),
            )
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Monitoring was paused, but the active PR review could not be cancelled",
                    Some(error.to_string()),
                )
            })?;
    }

    Ok(Json(UpdateAgentWorkspacePrReviewSettingsResponse {
        success: true,
        monitor: AgentWorkspacePrReviewMonitorResponse::from(monitor),
    }))
}
