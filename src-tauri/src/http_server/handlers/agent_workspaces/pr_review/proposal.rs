use super::*;

/// POST /api/agent-workspaces/{conversation_id}/pr-review-artifact
pub async fn write_agent_workspace_pr_review_artifact(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<WriteAgentWorkspacePrReviewArtifactRequest>,
) -> Result<Json<WriteAgentWorkspacePrReviewArtifactResponse>, JsonError> {
    let content = non_empty_string(req.content, "content")?;
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    if workspace.has_terminal_publication_pr_status() {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Pull request is already merged or closed",
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
    let head_sha = req.head_sha.or_else(|| review_pr_head_sha(&workspace));
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        head_sha.clone(),
        true,
    )
    .await?;
    let previous_artifact = match monitor.review_artifact_id.clone() {
        Some(artifact_id) => {
            let latest_id = state
                .app_state
                .artifact_repo
                .resolve_latest_artifact_id(&artifact_id)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?;
            state
                .app_state
                .artifact_repo
                .get_by_id(&latest_id)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?
        }
        None => None,
    };

    let title = req
        .title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            previous_artifact
                .as_ref()
                .map(|artifact| artifact.name.clone())
        })
        .unwrap_or_else(|| format!("PR #{} Review", pr_number));
    let previous_artifact_id = previous_artifact
        .as_ref()
        .map(|artifact| artifact.id.as_str().to_string());
    let next_version = previous_artifact
        .as_ref()
        .map(|artifact| artifact.metadata.version.saturating_add(1))
        .unwrap_or(1);
    let mut artifact =
        Artifact::new_inline(title, ArtifactType::PrReview, content, "ralphx-pr-reviewer");
    artifact.metadata.version = next_version;

    let created = if let Some(previous) = previous_artifact {
        state
            .app_state
            .artifact_repo
            .create_with_previous_version(artifact, previous.id)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?
    } else {
        state
            .app_state
            .artifact_repo
            .create(artifact)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?
    };

    monitor.last_seen_head_sha = head_sha.clone().or(monitor.last_seen_head_sha);
    monitor.last_review_run_id = req.created_by_run_id.or(monitor.last_review_run_id);
    monitor.review_artifact_id = Some(created.id.clone());
    monitor.review_artifact_head_sha = head_sha.clone();
    monitor.review_artifact_version = Some(created.metadata.version);
    monitor.review_artifact_updated_at = Some(created.metadata.created_at);
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    if monitor.status == AgentWorkspacePrReviewMonitorStatus::Terminal {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Pull request became terminal before the review action settled",
            None,
        ));
    }

    let content_text = match &created.content {
        crate::domain::entities::ArtifactContent::Inline { text } => text.clone(),
        crate::domain::entities::ArtifactContent::File { path } => format!("[File: {}]", path),
    };
    let event_name = if previous_artifact_id.is_some() {
        "pr_review_artifact:updated"
    } else {
        "pr_review_artifact:created"
    };
    crate::http_server::emit_http_event(
        &state,
        event_name,
        serde_json::json!({
            "conversationId": conversation_id.as_str(),
            "prNumber": pr_number,
            "headSha": head_sha,
            "previousArtifactId": previous_artifact_id,
            "artifact": {
                "id": created.id.as_str(),
                "name": created.name.clone(),
                "content": content_text,
                "version": created.metadata.version,
            }
        }),
    );

    let mut artifact_response = ArtifactResponse::from(created);
    artifact_response.previous_artifact_id = previous_artifact_id.clone();

    Ok(Json(WriteAgentWorkspacePrReviewArtifactResponse {
        success: true,
        monitor: AgentWorkspacePrReviewMonitorResponse::from(monitor),
        artifact: artifact_response,
        previous_artifact_id,
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/pr-review-actions
pub async fn propose_agent_workspace_pr_review_action(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<ProposeAgentWorkspacePrReviewActionRequest>,
) -> Result<Json<ProposeAgentWorkspacePrReviewActionResponse>, JsonError> {
    let head_sha = non_empty_string(req.head_sha, "head_sha")?;
    let summary = non_empty_string(req.summary, "summary")?;
    let review_body = non_empty_string(req.review_body, "review_body")?;
    let proposed_action = AgentWorkspacePrReviewActionKind::from_str(req.proposed_action.trim())
        .map_err(|error| json_error(StatusCode::BAD_REQUEST, error, None))?;
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    if workspace.has_terminal_publication_pr_status() {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Pull request is already merged or closed",
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
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        Some(head_sha.clone()),
        true,
    )
    .await?;
    ensure_review_artifact_for_head(&monitor, &head_sha)?;
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
    if health.sync_state.head_ref_oid.as_deref() != Some(head_sha.as_str()) {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Pull request head changed; run a fresh review before proposing an action",
            None,
        ));
    }

    let action = AgentWorkspacePrReviewAction::new(
        conversation_id.clone(),
        pr_number,
        head_sha.clone(),
        proposed_action,
        summary,
        review_body,
        req.findings_json,
        req.created_by_run_id.clone(),
    );
    let entering_awaiting_user = monitor.monitor_enabled
        && monitor.status != AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    monitor.status = if monitor.monitor_enabled {
        AgentWorkspacePrReviewMonitorStatus::AwaitingUser
    } else {
        AgentWorkspacePrReviewMonitorStatus::Paused
    };
    monitor.first_review_completed = true;
    monitor.last_reviewed_head_sha = Some(action.head_sha.clone());
    monitor.last_review_run_id = req.created_by_run_id;
    monitor.last_review_outcome = Some(proposed_action.to_string());
    monitor.last_error = None;
    let transition = state
        .app_state
        .agent_conversation_workspace_repo
        .transition_pr_review_state_if_nonterminal(
            monitor,
            Some(AgentWorkspacePrReviewActionMutation::UpsertPending(action)),
        )
        .await
        .map_err(|error| match error {
            AppError::Conflict(message) => json_error(StatusCode::CONFLICT, message, None),
            error => json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None),
        })?
        .ok_or_else(|| {
            json_error(
                StatusCode::CONFLICT,
                "Pull request became terminal before the review action settled",
                None,
            )
        })?;
    let mut monitor = transition.monitor;
    let action = transition.action.ok_or_else(|| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Review PR proposal transition did not return its action",
            None,
        )
    })?;
    let recent_actions = state
        .app_state
        .agent_conversation_workspace_repo
        .list_pr_review_actions(&conversation_id, 100)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    for superseded in recent_actions
        .iter()
        .filter(|candidate| candidate.status == AgentWorkspacePrReviewActionStatus::Superseded)
    {
        state
            .app_state
            .notification_service()
            .resolve_workflow_notification(&pr_review_notification_key(
                conversation_id.as_str(),
                &superseded.id,
            ))
            .await;
    }
    if monitor.can_auto_approve(&action) {
        match submit_agent_workspace_pr_review_action(
            State(state.clone()),
            Path((conversation_id.to_string(), action.id.clone())),
            Json(SubmitAgentWorkspacePrReviewActionRequest { action_kind: None }),
        )
        .await
        {
            Ok(Json(submission)) => {
                return Ok(Json(ProposeAgentWorkspacePrReviewActionResponse {
                    success: true,
                    monitor: submission.monitor,
                    action: submission.action,
                }));
            }
            Err(_) => {
                monitor = state
                    .app_state
                    .agent_conversation_workspace_repo
                    .get_pr_review_monitor(&conversation_id)
                    .await
                    .map_err(|error| {
                        json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                    })?
                    .unwrap_or(monitor);
                if monitor.status == AgentWorkspacePrReviewMonitorStatus::Terminal {
                    return Err(json_error(
                        StatusCode::CONFLICT,
                        "Pull request became terminal during automatic review submission",
                        None,
                    ));
                }
            }
        }
    }
    if entering_awaiting_user {
        state
            .app_state
            .notification_service()
            .record(NewNotification {
                project_id: Some(monitor.project_id.to_string()),
                category: NotificationCategory::PrReviewAction,
                severity: NotificationSeverity::ActionRequired,
                title: format!("PR #{} needs your review", monitor.pr_number),
                body: Some("A PR review action is waiting for your decision".to_string()),
                target: NotificationTarget {
                    kind: NotificationTargetKind::AgentConversation,
                    project_id: Some(monitor.project_id.to_string()),
                    task_id: None,
                    conversation_id: Some(monitor.conversation_id.to_string()),
                    setup_conversation_id: None,
                    automation_id: None,
                    run_id: None,
                },
                dedupe_key: Some(pr_review_notification_key(
                    monitor.conversation_id.as_str(),
                    &action.id,
                )),
            })
            .await;
    }

    Ok(Json(ProposeAgentWorkspacePrReviewActionResponse {
        success: true,
        monitor: AgentWorkspacePrReviewMonitorResponse::from(monitor),
        action: AgentWorkspacePrReviewActionResponse::from(action),
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/complete-pr-review-run
pub async fn complete_agent_workspace_pr_review_run(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    Json(req): Json<CompleteAgentWorkspacePrReviewRunRequest>,
) -> Result<Json<CompleteAgentWorkspacePrReviewRunResponse>, JsonError> {
    let summary = non_empty_string(req.summary, "summary")?;
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;
    if workspace.has_terminal_publication_pr_status() {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Pull request is already merged or closed",
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
    let head_sha = req.head_sha.or_else(|| review_pr_head_sha(&workspace));
    let mut monitor = load_or_create_pr_review_monitor(
        state.app_state.as_ref(),
        &workspace,
        pr_number,
        head_sha.clone(),
        true,
    )
    .await?;
    monitor.first_review_completed = true;
    monitor.last_seen_head_sha = head_sha.clone().or(monitor.last_seen_head_sha);
    monitor.last_reviewed_head_sha = head_sha.or(monitor.last_reviewed_head_sha);
    monitor.last_review_run_id = req.created_by_run_id;
    monitor.last_review_outcome = req.outcome.or_else(|| Some("no_action".to_string()));
    monitor.last_error = req.blocker.or_else(|| {
        if summary.trim().is_empty() {
            Some("Review run completed without a summary".to_string())
        } else {
            None
        }
    });
    monitor.status = monitor.settlement_status();
    let monitor = state
        .app_state
        .agent_conversation_workspace_repo
        .transition_pr_review_state_if_nonterminal(monitor, None)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        .ok_or_else(|| {
            json_error(
                StatusCode::CONFLICT,
                "Pull request became terminal before review completion",
                None,
            )
        })?
        .monitor;
    maybe_start_pr_review_monitor_polling(state.app_state.as_ref(), &workspace, &monitor).await;

    Ok(Json(CompleteAgentWorkspacePrReviewRunResponse {
        success: true,
        monitor: AgentWorkspacePrReviewMonitorResponse::from(monitor),
    }))
}
