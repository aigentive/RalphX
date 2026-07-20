use super::*;

fn delegated_event_seq() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).unwrap_or_default()
}

fn delegated_total_tokens(latest_run: &DelegatedRunSummary) -> Option<u64> {
    let total = latest_run.input_tokens.unwrap_or(0)
        + latest_run.output_tokens.unwrap_or(0)
        + latest_run.cache_creation_tokens.unwrap_or(0)
        + latest_run.cache_read_tokens.unwrap_or(0);
    if total == 0
        && latest_run.input_tokens.is_none()
        && latest_run.output_tokens.is_none()
        && latest_run.cache_creation_tokens.is_none()
        && latest_run.cache_read_tokens.is_none()
    {
        None
    } else {
        Some(total)
    }
}

fn delegated_duration_ms(latest_run: &DelegatedRunSummary) -> Option<u64> {
    let completed_at = latest_run.completed_at.as_ref()?;
    let started = chrono::DateTime::parse_from_rfc3339(&latest_run.started_at).ok()?;
    let completed = chrono::DateTime::parse_from_rfc3339(completed_at).ok()?;
    let duration = completed.signed_duration_since(started).num_milliseconds();
    if duration < 0 {
        None
    } else {
        u64::try_from(duration).ok()
    }
}

fn cached_streaming_task_from_started_payload(
    payload: &AgentTaskStartedPayload,
) -> CachedStreamingTask {
    CachedStreamingTask {
        tool_use_id: payload.tool_use_id.clone(),
        description: payload.description.clone(),
        subagent_type: payload.subagent_type.clone(),
        model: payload
            .model
            .clone()
            .or_else(|| payload.effective_model_id.clone())
            .or_else(|| payload.logical_model.clone()),
        status: "running".to_string(),
        agent_id: payload.delegated_agent_run_id.clone(),
        teammate_name: payload.teammate_name.clone(),
        delegated_job_id: payload.delegated_job_id.clone(),
        delegated_session_id: payload.delegated_session_id.clone(),
        delegated_conversation_id: payload.delegated_conversation_id.clone(),
        delegated_agent_run_id: payload.delegated_agent_run_id.clone(),
        provider_harness: payload.provider_harness.clone(),
        provider_session_id: payload.provider_session_id.clone(),
        upstream_provider: payload.upstream_provider.clone(),
        provider_profile: payload.provider_profile.clone(),
        logical_model: payload.logical_model.clone(),
        effective_model_id: payload.effective_model_id.clone(),
        logical_effort: payload.logical_effort.clone(),
        effective_effort: payload.effective_effort.clone(),
        approval_policy: payload.approval_policy.clone(),
        sandbox_mode: payload.sandbox_mode.clone(),
        total_tokens: None,
        total_tool_uses: None,
        duration_ms: None,
        input_tokens: None,
        output_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        estimated_usd: None,
        text_output: None,
    }
}

fn cached_streaming_task_from_completed_payload(
    payload: &AgentTaskCompletedPayload,
) -> CachedStreamingTask {
    CachedStreamingTask {
        tool_use_id: payload.tool_use_id.clone(),
        description: None,
        subagent_type: Some("delegated".to_string()),
        model: payload
            .effective_model_id
            .clone()
            .or_else(|| payload.logical_model.clone()),
        status: payload
            .status
            .clone()
            .unwrap_or_else(|| "completed".to_string()),
        agent_id: payload
            .agent_id
            .clone()
            .or_else(|| payload.delegated_agent_run_id.clone()),
        teammate_name: payload.teammate_name.clone(),
        delegated_job_id: payload.delegated_job_id.clone(),
        delegated_session_id: payload.delegated_session_id.clone(),
        delegated_conversation_id: payload.delegated_conversation_id.clone(),
        delegated_agent_run_id: payload.delegated_agent_run_id.clone(),
        provider_harness: payload.provider_harness.clone(),
        provider_session_id: payload.provider_session_id.clone(),
        upstream_provider: payload.upstream_provider.clone(),
        provider_profile: payload.provider_profile.clone(),
        logical_model: payload.logical_model.clone(),
        effective_model_id: payload.effective_model_id.clone(),
        logical_effort: payload.logical_effort.clone(),
        effective_effort: payload.effective_effort.clone(),
        approval_policy: payload.approval_policy.clone(),
        sandbox_mode: payload.sandbox_mode.clone(),
        total_tokens: payload.total_tokens,
        total_tool_uses: payload.total_tool_use_count,
        duration_ms: payload.total_duration_ms,
        input_tokens: payload.input_tokens,
        output_tokens: payload.output_tokens,
        cache_creation_tokens: payload.cache_creation_tokens,
        cache_read_tokens: payload.cache_read_tokens,
        estimated_usd: payload.estimated_usd,
        text_output: payload.text_output.clone(),
    }
}

#[doc(hidden)]
pub fn build_delegated_task_started_payload(
    snapshot: &DelegationJobSnapshot,
    logical_model: Option<&str>,
    logical_effort: Option<&str>,
    approval_policy: Option<&str>,
    sandbox_mode: Option<&str>,
    seq: u64,
) -> Option<AgentTaskStartedPayload> {
    let parent_conversation_id = snapshot.parent_conversation_id.as_ref()?;
    let tool_use_id = snapshot
        .parent_tool_use_id
        .clone()
        .unwrap_or_else(|| format!("delegate-job:{}", snapshot.job_id));
    Some(AgentTaskStartedPayload {
        tool_use_id,
        run_id: None,
        tool_name: "delegate_start".to_string(),
        description: Some(snapshot.agent_name.clone()),
        subagent_type: Some("delegated".to_string()),
        model: snapshot
            .effective_model_id
            .clone()
            .or_else(|| snapshot.logical_model.clone())
            .or_else(|| logical_model.map(str::to_string)),
        teammate_name: None,
        delegated_job_id: Some(snapshot.job_id.clone()),
        delegated_session_id: Some(snapshot.delegated_session_id.clone()),
        delegated_conversation_id: snapshot.delegated_conversation_id.clone(),
        delegated_agent_run_id: snapshot.delegated_agent_run_id.clone(),
        provider_harness: Some(snapshot.harness.clone()),
        provider_session_id: snapshot.provider_session_id.clone(),
        upstream_provider: snapshot.upstream_provider.clone(),
        provider_profile: snapshot.provider_profile.clone(),
        logical_model: snapshot
            .logical_model
            .clone()
            .or_else(|| logical_model.map(str::to_string)),
        effective_model_id: snapshot.effective_model_id.clone(),
        logical_effort: snapshot
            .logical_effort
            .clone()
            .or_else(|| logical_effort.map(str::to_string)),
        effective_effort: snapshot.effective_effort.clone(),
        approval_policy: snapshot
            .approval_policy
            .clone()
            .or_else(|| approval_policy.map(str::to_string)),
        sandbox_mode: snapshot
            .sandbox_mode
            .clone()
            .or_else(|| sandbox_mode.map(str::to_string)),
        conversation_id: parent_conversation_id.clone(),
        context_type: snapshot.parent_context_type.clone(),
        context_id: snapshot.parent_context_id.clone(),
        seq,
    })
}

#[doc(hidden)]
pub fn build_delegated_task_completed_payload(
    snapshot: &DelegationJobSnapshot,
    latest_run: Option<&DelegatedRunSummary>,
    status: &str,
    text_output: Option<&str>,
    error: Option<&str>,
    seq: u64,
) -> Option<AgentTaskCompletedPayload> {
    let parent_conversation_id = snapshot.parent_conversation_id.as_ref()?;
    let tool_use_id = snapshot
        .parent_tool_use_id
        .clone()
        .unwrap_or_else(|| format!("delegate-job:{}", snapshot.job_id));
    let latest_run_id = latest_run.map(|run| run.agent_run_id.clone());
    Some(AgentTaskCompletedPayload {
        tool_use_id,
        run_id: None,
        agent_id: latest_run_id.or_else(|| snapshot.delegated_agent_run_id.clone()),
        status: Some(status.to_string()),
        total_duration_ms: latest_run.and_then(delegated_duration_ms),
        total_tokens: latest_run.and_then(delegated_total_tokens),
        total_tool_use_count: None,
        teammate_name: None,
        delegated_job_id: Some(snapshot.job_id.clone()),
        delegated_session_id: Some(snapshot.delegated_session_id.clone()),
        delegated_conversation_id: snapshot.delegated_conversation_id.clone(),
        delegated_agent_run_id: latest_run
            .map(|run| run.agent_run_id.clone())
            .or_else(|| snapshot.delegated_agent_run_id.clone()),
        provider_harness: latest_run
            .and_then(|run| run.harness.clone())
            .or_else(|| Some(snapshot.harness.clone())),
        provider_session_id: latest_run
            .and_then(|run| run.provider_session_id.clone())
            .or_else(|| snapshot.provider_session_id.clone()),
        upstream_provider: latest_run
            .and_then(|run| run.upstream_provider.clone())
            .or_else(|| snapshot.upstream_provider.clone()),
        provider_profile: latest_run
            .and_then(|run| run.provider_profile.clone())
            .or_else(|| snapshot.provider_profile.clone()),
        logical_model: latest_run
            .and_then(|run| run.logical_model.clone())
            .or_else(|| snapshot.logical_model.clone()),
        effective_model_id: latest_run
            .and_then(|run| run.effective_model_id.clone())
            .or_else(|| snapshot.effective_model_id.clone()),
        logical_effort: latest_run
            .and_then(|run| run.logical_effort.clone())
            .or_else(|| snapshot.logical_effort.clone()),
        effective_effort: latest_run
            .and_then(|run| run.effective_effort.clone())
            .or_else(|| snapshot.effective_effort.clone()),
        approval_policy: latest_run
            .and_then(|run| run.approval_policy.clone())
            .or_else(|| snapshot.approval_policy.clone()),
        sandbox_mode: latest_run
            .and_then(|run| run.sandbox_mode.clone())
            .or_else(|| snapshot.sandbox_mode.clone()),
        input_tokens: latest_run.and_then(|run| run.input_tokens),
        output_tokens: latest_run.and_then(|run| run.output_tokens),
        cache_creation_tokens: latest_run.and_then(|run| run.cache_creation_tokens),
        cache_read_tokens: latest_run.and_then(|run| run.cache_read_tokens),
        estimated_usd: latest_run.and_then(|run| run.estimated_usd),
        text_output: text_output.map(str::to_string),
        error: error.map(str::to_string),
        conversation_id: parent_conversation_id.clone(),
        context_type: snapshot.parent_context_type.clone(),
        context_id: snapshot.parent_context_id.clone(),
        seq,
    })
}

fn delegated_run_summary(run: AgentRun) -> DelegatedRunSummary {
    DelegatedRunSummary {
        agent_run_id: run.id.as_str(),
        status: run.status.to_string(),
        started_at: run.started_at.to_rfc3339(),
        completed_at: run.completed_at.map(|timestamp| timestamp.to_rfc3339()),
        error_message: run.error_message,
        harness: run.harness.map(|harness| harness.to_string()),
        provider_session_id: run.provider_session_id,
        upstream_provider: run.upstream_provider,
        provider_profile: run.provider_profile,
        logical_model: run.logical_model,
        effective_model_id: run.effective_model_id,
        logical_effort: run.logical_effort.map(|effort| effort.to_string()),
        effective_effort: run.effective_effort,
        approval_policy: run.approval_policy,
        sandbox_mode: run.sandbox_mode,
        input_tokens: run.input_tokens,
        output_tokens: run.output_tokens,
        cache_creation_tokens: run.cache_creation_tokens,
        cache_read_tokens: run.cache_read_tokens,
        estimated_usd: run.estimated_usd,
    }
}

fn latest_delegated_handoff_message(messages: Vec<ChatMessage>) -> Option<ChatMessage> {
    messages.into_iter().rev().find(|message| {
        !matches!(
            message.role,
            crate::domain::entities::MessageRole::User
                | crate::domain::entities::MessageRole::System
        ) && !message.content.trim().is_empty()
    })
}

fn delegated_handoff_message_summary(message: ChatMessage) -> ChatMessageSummary {
    ChatMessageSummary {
        role: message.role.to_string(),
        content: message.content.chars().take(500).collect(),
        created_at: message.created_at.to_rfc3339(),
    }
}

pub(super) async fn resolve_parent_conversation_id(
    state: &HttpServerState,
    req: &DelegateStartRequest,
    parent_session_id: &str,
) -> Result<Option<String>, JsonError> {
    if let Some(parent_conversation_id) = req.parent_conversation_id.as_ref() {
        return Ok(Some(parent_conversation_id.clone()));
    }

    if req.caller_context_type.as_deref() == Some("ideation") {
        if let Some(caller_context_id) = req.caller_context_id.as_deref() {
            return Ok(state
                .app_state
                .chat_conversation_repo
                .get_active_for_context(ChatContextType::Ideation, caller_context_id)
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to load caller conversation: {error}"),
                    )
                })?
                .map(|conversation| conversation.id.as_str()));
        }
    }

    Ok(state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Ideation, parent_session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load parent conversation: {error}"),
            )
        })?
        .map(|conversation| conversation.id.as_str()))
}

async fn ensure_delegated_conversation(
    state: &HttpServerState,
    delegated_session_id: &str,
    parent_conversation_id: Option<&str>,
    title: Option<&str>,
) -> Result<ChatConversation, JsonError> {
    if let Some(conversation) = state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Delegation, delegated_session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated conversation: {error}"),
            )
        })?
    {
        return Ok(conversation);
    }

    let mut conversation = ChatConversation::new_delegation(DelegatedSessionId::from_string(
        delegated_session_id.to_string(),
    ));
    conversation.parent_conversation_id = parent_conversation_id.map(str::to_string);
    conversation.title = title.map(str::to_string);
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create delegated conversation: {error}"),
            )
        })
}

pub(crate) async fn build_delegated_session_status_response(
    state: &HttpServerState,
    delegated_session_id: &str,
    include_messages: bool,
    message_limit: Option<u32>,
) -> Result<DelegatedSessionStatusResponse, JsonError> {
    let session_id = DelegatedSessionId::from_string(delegated_session_id.to_string());
    let session = state
        .app_state
        .delegated_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated session: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegated session not found"))?;

    let estimated_status = match session.status.as_str() {
        "running" => "running",
        "completed" => "completed",
        "failed" => "failed",
        "cancelled" => "cancelled",
        _ => "idle",
    };
    let agent_state = AgentStateInfo {
        is_running: session.status == "running",
        started_at: Some(session.created_at.to_rfc3339()),
        last_active_at: Some(session.updated_at.to_rfc3339()),
        pid: None,
        estimated_status: estimated_status.to_string(),
    };

    let recent_messages = if include_messages {
        let _ = message_limit;
        if let Some(conversation) = state
            .app_state
            .chat_conversation_repo
            .get_active_for_context(ChatContextType::Delegation, delegated_session_id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load delegated conversation: {error}"),
                )
            })?
        {
            let messages = state
                .app_state
                .chat_message_repo
                .get_by_conversation(&conversation.id)
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to load delegated messages: {error}"),
                    )
                })?;
            Some(
                latest_delegated_handoff_message(messages)
                    .map(delegated_handoff_message_summary)
                    .into_iter()
                    .collect(),
            )
        } else {
            Some(Vec::new())
        }
    } else {
        None
    };

    let (conversation_id, latest_run) = if let Some(conversation) = state
        .app_state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Delegation, delegated_session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated conversation: {error}"),
            )
        })? {
        let latest_run = state
            .app_state
            .agent_run_repo
            .get_latest_for_conversation(&conversation.id)
            .await
            .map_err(|error| {
                json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to load delegated run: {error}"),
                )
            })?
            .map(delegated_run_summary);
        (Some(conversation.id.as_str()), latest_run)
    } else {
        (None, None)
    };

    Ok(DelegatedSessionStatusResponse {
        session: DelegatedSessionSummary {
            id: session.id.as_str().to_string(),
            title: session.title,
            status: session.status,
            parent_context_type: session.parent_context_type,
            parent_context_id: session.parent_context_id,
            agent_name: session.agent_name,
            harness: session.harness.to_string(),
            provider_session_id: session.provider_session_id,
            created_at: session.created_at.to_rfc3339(),
            updated_at: session.updated_at.to_rfc3339(),
            completed_at: session.completed_at.map(|timestamp| timestamp.to_rfc3339()),
        },
        agent_state,
        conversation_id,
        latest_run,
        recent_messages,
    })
}

pub async fn get_delegated_session_status(
    State(state): State<HttpServerState>,
    axum::extract::Path(delegated_session_id): axum::extract::Path<String>,
) -> Result<Json<DelegatedSessionStatusResponse>, JsonError> {
    let status =
        build_delegated_session_status_response(&state, &delegated_session_id, false, None).await?;
    Ok(Json(status))
}

pub(crate) async fn start_delegate_impl(
    state: &HttpServerState,
    req: DelegateStartRequest,
) -> Result<DelegationJobSnapshot, JsonError> {
    let caller_agent_name = req.caller_agent_name.as_deref().ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "delegate_start requires caller_agent_name from the MCP transport",
        )
    })?;
    let parent = resolve_delegate_parent(state, &req).await?;
    let requested_harness = req.harness.or(parent.inherited_harness);
    let project = state
        .app_state
        .project_repo
        .get_by_id(&ProjectId::from_string(parent.project_id.clone()))
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load delegated project: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegated project not found"))?;
    let role = routing_role_for_delegated_launch(
        &req.agent_name,
        parent.context_type,
        parent.ideation_verification,
    );
    let resolved_spawn = resolve_manual_role_spawn_settings(
        &req.agent_name,
        Some(parent.project_id.as_str()),
        Some(std::path::Path::new(&project.working_directory)),
        role,
        requested_harness,
        req.model.as_deref(),
        &state.app_state.manual_role_default_service(),
    )
    .await
    .map_err(|error| {
        json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to resolve delegated agent defaults: {error}"),
        )
    })?;
    let harness = resolved_spawn.effective_harness;
    let delegated_model = req.model.clone();
    let plugin_dir = resolve_harness_plugin_dir(harness, &parent.working_directory);
    let project_root = resolve_project_root_from_plugin_dir(&plugin_dir);
    let (_caller_definition, definition) = resolve_delegation_policy(
        &project_root,
        caller_agent_name,
        req.caller_agent_profile.as_deref(),
        &req.agent_name,
    )?;
    let delegated_session_id = resolve_delegated_session_id(state, &req, &parent, harness).await?;
    let logical_effort = req.logical_effort.or(resolved_spawn.logical_effort);
    let approval_policy = req
        .approval_policy
        .clone()
        .or(resolved_spawn.approval_policy.clone());
    let sandbox_mode = req
        .sandbox_mode
        .clone()
        .or(resolved_spawn.sandbox_mode.clone());
    state
        .app_state
        .delegated_session_repo
        .update_status(
            &DelegatedSessionId::from_string(delegated_session_id.clone()),
            "running",
            None,
            None,
        )
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to update delegated session status: {error}"),
            )
        })?;

    let delegated_conversation = match ensure_delegated_conversation(
        state,
        &delegated_session_id,
        parent.parent_conversation_id.as_deref(),
        req.title.as_deref(),
    )
    .await
    {
        Ok(conversation) => conversation,
        Err(error) => {
            mark_delegated_launch_failed(state, &delegated_session_id, &json_error_detail(&error))
                .await?;
            return Err(error);
        }
    };

    let chat_service = state
        .app_state
        .build_chat_service_with_execution_state(Arc::clone(&state.execution_state));
    let send_result = chat_service
        .send_message(
            ChatContextType::Delegation,
            &delegated_session_id,
            &build_delegated_prompt(
                &definition.name,
                parent.context_type,
                &parent.context_id,
                req.parent_turn_id.as_deref(),
                req.parent_message_id.as_deref(),
                parent.parent_conversation_id.as_deref(),
                req.parent_tool_use_id.as_deref(),
                &delegated_session_id,
                &req.prompt,
            ),
            SendMessageOptions {
                routing_role_override: Some(role),
                harness_override: Some(harness),
                agent_name_override: Some(definition.name.clone()),
                model_override: delegated_model.clone(),
                working_directory_override: Some(parent.working_directory.clone()),
                logical_effort_override: logical_effort.clone(),
                approval_policy_override: approval_policy.clone(),
                sandbox_mode_override: sandbox_mode.clone(),
                is_external_mcp: true,
                ..Default::default()
            },
        )
        .await;
    let send_result = match send_result {
        Ok(result) => result,
        Err(error) => {
            let error_message = format!("Failed to start delegated chat run: {error}");
            mark_delegated_launch_failed(state, &delegated_session_id, &error_message).await?;
            return Err(json_error(StatusCode::INTERNAL_SERVER_ERROR, error_message));
        }
    };

    let launched_run = match state
        .app_state
        .agent_run_repo
        .get_by_id(&AgentRunId::from_string(send_result.agent_run_id.clone()))
        .await
    {
        Ok(run) => run,
        Err(error) => {
            warn!(
                agent_run_id = send_result.agent_run_id,
                %error,
                "Delegated run started but effective runtime attribution could not be loaded"
            );
            None
        }
    };

    let job_id = uuid::Uuid::new_v4().to_string();
    let snapshot = state
        .delegation_service
        .register_running(
            job_id.clone(),
            parent.context_type.to_string(),
            parent.context_id.clone(),
            req.parent_turn_id.clone(),
            req.parent_message_id.clone(),
            parent.parent_conversation_id.clone(),
            req.parent_tool_use_id.clone(),
            delegated_session_id.clone(),
            Some(delegated_conversation.id.as_str()),
            Some(send_result.agent_run_id.clone()),
            definition.name.clone(),
            harness.to_string(),
            launched_run
                .as_ref()
                .and_then(|run| run.provider_session_id.clone()),
            launched_run
                .as_ref()
                .and_then(|run| run.upstream_provider.clone()),
            launched_run
                .as_ref()
                .and_then(|run| run.provider_profile.clone()),
            launched_run
                .as_ref()
                .and_then(|run| run.logical_model.clone())
                .clone()
                .or_else(|| delegated_model.clone()),
            launched_run
                .as_ref()
                .and_then(|run| run.effective_model_id.clone()),
            launched_run
                .as_ref()
                .and_then(|run| run.logical_effort.map(|value| value.to_string()))
                .or_else(|| logical_effort.map(|value| value.to_string())),
            launched_run
                .as_ref()
                .and_then(|run| run.effective_effort.clone()),
            launched_run
                .as_ref()
                .and_then(|run| run.approval_policy.clone())
                .or_else(|| approval_policy.clone()),
            launched_run
                .as_ref()
                .and_then(|run| run.sandbox_mode.clone())
                .or_else(|| sandbox_mode.clone()),
        )
        .await;

    let logical_effort_label = logical_effort.as_ref().map(|value| value.to_string());
    if let Some(payload) = build_delegated_task_started_payload(
        &snapshot,
        delegated_model.as_deref(),
        logical_effort_label.as_deref(),
        approval_policy.as_deref(),
        sandbox_mode.as_deref(),
        delegated_event_seq(),
    ) {
        state
            .app_state
            .streaming_state_cache
            .add_task(
                &payload.conversation_id,
                cached_streaming_task_from_started_payload(&payload),
            )
            .await;
        crate::http_server::emit_serialized_http_event(state, events::AGENT_TASK_STARTED, &payload);
    }

    let delegation_service = state.delegation_service.clone();
    let delegated_session_repo = state.app_state.delegated_session_repo.clone();
    let chat_message_repo = state.app_state.chat_message_repo.clone();
    let chat_timeline_repo = state.app_state.chat_timeline_repo.clone();
    let agent_run_repo = state.app_state.agent_run_repo.clone();
    let app_events = Arc::clone(&state.app_state.events);
    let streaming_state_cache = state.app_state.streaming_state_cache.clone();
    let snapshot_for_events = snapshot.clone();
    let agent_run_id = send_result.agent_run_id.clone();
    let conversation_id = delegated_conversation.id;
    let delegated_session_id_for_task = delegated_session_id.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let run = match agent_run_repo
                .get_by_id(&crate::domain::entities::AgentRunId::from_string(
                    agent_run_id.clone(),
                ))
                .await
            {
                Ok(Some(run)) => run,
                Ok(None) => continue,
                Err(error) => {
                    let settled_snapshot = delegation_service
                        .mark_failed(&job_id, error.to_string())
                        .await
                        .unwrap_or_else(|| snapshot_for_events.clone());
                    if let Err(update_error) = delegated_session_repo
                        .update_status(
                            &DelegatedSessionId::from_string(delegated_session_id_for_task.clone()),
                            "failed",
                            Some(error.to_string()),
                            Some(Utc::now()),
                        )
                        .await
                    {
                        warn!(job_id, %update_error, "Failed to persist delegated session failure");
                    }
                    if let Err(projection_error) =
                        persist_terminal_projection(&chat_timeline_repo, &settled_snapshot, None)
                            .await
                    {
                        warn!(job_id, %projection_error, "Failed to persist delegated terminal projection");
                    }
                    if let Some(payload) = build_delegated_task_completed_payload(
                        &settled_snapshot,
                        None,
                        "failed",
                        None,
                        Some(&error.to_string()),
                        delegated_event_seq(),
                    ) {
                        streaming_state_cache
                            .add_task(
                                &payload.conversation_id,
                                cached_streaming_task_from_completed_payload(&payload),
                            )
                            .await;
                        if let Err(error) = ralphx_events::emit_serialized(
                            app_events.as_ref(),
                            events::AGENT_TASK_COMPLETED,
                            &payload,
                        ) {
                            tracing::warn!(
                                event = events::AGENT_TASK_COMPLETED,
                                %error,
                                "Failed to serialize delegated task completion event payload"
                            );
                        }
                    }
                    break;
                }
            };

            if run.status == crate::domain::entities::AgentRunStatus::Running {
                continue;
            }

            let latest_run = delegated_run_summary(run.clone());

            match run.status {
                crate::domain::entities::AgentRunStatus::Completed => {
                    let mut content = String::new();
                    for _ in 0..10 {
                        content = chat_message_repo
                            .get_by_conversation(&conversation_id)
                            .await
                            .ok()
                            .and_then(latest_delegated_handoff_message)
                            .map(|message| message.content)
                            .unwrap_or_default();
                        if !content.is_empty() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                    let settled_snapshot = delegation_service
                        .mark_completed(&job_id, content.clone())
                        .await
                        .unwrap_or_else(|| snapshot_for_events.clone());
                    if let Err(update_error) = delegated_session_repo
                        .update_status(
                            &DelegatedSessionId::from_string(delegated_session_id_for_task.clone()),
                            "completed",
                            None,
                            Some(Utc::now()),
                        )
                        .await
                    {
                        warn!(job_id, %update_error, "Failed to persist delegated session completion");
                    }
                    if let Err(projection_error) = persist_terminal_projection(
                        &chat_timeline_repo,
                        &settled_snapshot,
                        Some(&latest_run),
                    )
                    .await
                    {
                        warn!(job_id, %projection_error, "Failed to persist delegated terminal projection");
                    }
                    if let Some(payload) = build_delegated_task_completed_payload(
                        &settled_snapshot,
                        Some(&latest_run),
                        "completed",
                        Some(&content),
                        None,
                        delegated_event_seq(),
                    ) {
                        streaming_state_cache
                            .add_task(
                                &payload.conversation_id,
                                cached_streaming_task_from_completed_payload(&payload),
                            )
                            .await;
                        if let Err(error) = ralphx_events::emit_serialized(
                            app_events.as_ref(),
                            events::AGENT_TASK_COMPLETED,
                            &payload,
                        ) {
                            tracing::warn!(
                                event = events::AGENT_TASK_COMPLETED,
                                %error,
                                "Failed to serialize delegated task completion event payload"
                            );
                        }
                    }
                }
                crate::domain::entities::AgentRunStatus::Failed => {
                    let detail = run
                        .error_message
                        .unwrap_or_else(|| "Delegated run failed".to_string());
                    let settled_snapshot = delegation_service
                        .mark_failed(&job_id, detail.clone())
                        .await
                        .unwrap_or_else(|| snapshot_for_events.clone());
                    if let Err(update_error) = delegated_session_repo
                        .update_status(
                            &DelegatedSessionId::from_string(delegated_session_id_for_task.clone()),
                            "failed",
                            Some(detail.clone()),
                            Some(Utc::now()),
                        )
                        .await
                    {
                        warn!(job_id, %update_error, "Failed to persist delegated session failure");
                    }
                    if let Err(projection_error) = persist_terminal_projection(
                        &chat_timeline_repo,
                        &settled_snapshot,
                        Some(&latest_run),
                    )
                    .await
                    {
                        warn!(job_id, %projection_error, "Failed to persist delegated terminal projection");
                    }
                    if let Some(payload) = build_delegated_task_completed_payload(
                        &settled_snapshot,
                        Some(&latest_run),
                        "failed",
                        None,
                        Some(&detail),
                        delegated_event_seq(),
                    ) {
                        streaming_state_cache
                            .add_task(
                                &payload.conversation_id,
                                cached_streaming_task_from_completed_payload(&payload),
                            )
                            .await;
                        if let Err(error) = ralphx_events::emit_serialized(
                            app_events.as_ref(),
                            events::AGENT_TASK_COMPLETED,
                            &payload,
                        ) {
                            tracing::warn!(
                                event = events::AGENT_TASK_COMPLETED,
                                %error,
                                "Failed to serialize delegated task completion event payload"
                            );
                        }
                    }
                }
                crate::domain::entities::AgentRunStatus::Cancelled => {
                    let settled_snapshot = match delegation_service.cancel(&job_id).await {
                        Some(snapshot) => snapshot,
                        None => delegation_service
                            .snapshot(&job_id)
                            .await
                            .unwrap_or_else(|| snapshot_for_events.clone()),
                    };
                    if let Err(update_error) = delegated_session_repo
                        .update_status(
                            &DelegatedSessionId::from_string(delegated_session_id_for_task.clone()),
                            "cancelled",
                            None,
                            Some(Utc::now()),
                        )
                        .await
                    {
                        warn!(job_id, %update_error, "Failed to persist delegated session cancellation");
                    }
                    if let Err(projection_error) = persist_terminal_projection(
                        &chat_timeline_repo,
                        &settled_snapshot,
                        Some(&latest_run),
                    )
                    .await
                    {
                        warn!(job_id, %projection_error, "Failed to persist delegated terminal projection");
                    }
                    if let Some(payload) = build_delegated_task_completed_payload(
                        &settled_snapshot,
                        Some(&latest_run),
                        "cancelled",
                        None,
                        None,
                        delegated_event_seq(),
                    ) {
                        streaming_state_cache
                            .add_task(
                                &payload.conversation_id,
                                cached_streaming_task_from_completed_payload(&payload),
                            )
                            .await;
                        if let Err(error) = ralphx_events::emit_serialized(
                            app_events.as_ref(),
                            events::AGENT_TASK_COMPLETED,
                            &payload,
                        ) {
                            tracing::warn!(
                                event = events::AGENT_TASK_COMPLETED,
                                %error,
                                "Failed to serialize delegated task completion event payload"
                            );
                        }
                    }
                }
                crate::domain::entities::AgentRunStatus::Running => {}
            }
            break;
        }
    });

    Ok(snapshot)
}

pub async fn start_delegate(
    State(state): State<HttpServerState>,
    Json(req): Json<DelegateStartRequest>,
) -> Result<Json<DelegationJobSnapshot>, JsonError> {
    Ok(Json(start_delegate_impl(&state, req).await?))
}

pub async fn wait_delegate(
    State(state): State<HttpServerState>,
    Json(req): Json<DelegateWaitRequest>,
) -> Result<Json<DelegationJobSnapshot>, JsonError> {
    let mut snapshot = state
        .delegation_service
        .snapshot(&req.job_id)
        .await
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegation job not found"))?;
    if req
        .include_delegated_status
        .or(req.include_child_status)
        .unwrap_or(true)
    {
        match build_delegated_session_status_response(
            &state,
            &snapshot.delegated_session_id,
            req.include_messages.unwrap_or(false),
            req.message_limit,
        )
        .await
        {
            Ok(delegated_status) => {
                snapshot.delegated_status = Some(delegated_status);
            }
            Err((status, error)) => {
                warn!(
                    job_id = snapshot.job_id,
                    delegated_session_id = snapshot.delegated_session_id,
                    status = status.as_u16(),
                    error = %error.0["error"].as_str().unwrap_or("unknown error"),
                    "Failed to hydrate delegated session status"
                );
            }
        }
    }
    Ok(Json(snapshot))
}

pub async fn cancel_delegate(
    State(state): State<HttpServerState>,
    Json(req): Json<DelegateCancelRequest>,
) -> Result<Json<DelegationJobSnapshot>, JsonError> {
    cancel_delegate_impl(&state, &req.job_id).await.map(Json)
}

pub(crate) async fn cancel_delegate_impl(
    state: &HttpServerState,
    job_id: &str,
) -> Result<DelegationJobSnapshot, JsonError> {
    let snapshot = state
        .delegation_service
        .snapshot(job_id)
        .await
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegation job not found"))?;
    if snapshot.status != "running" {
        return Err(json_error(
            StatusCode::NOT_FOUND,
            "Delegation job is no longer cancellable",
        ));
    }
    let chat_service = state
        .app_state
        .build_chat_service_with_execution_state(Arc::clone(&state.execution_state));
    let stopped = chat_service
        .stop_agent(ChatContextType::Delegation, &snapshot.delegated_session_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to stop delegated agent: {error}"),
            )
        })?;
    if !stopped {
        return Err(json_error(
            StatusCode::NOT_FOUND,
            "Delegation job is no longer running",
        ));
    }
    let snapshot = match state.delegation_service.cancel(job_id).await {
        Some(snapshot) => snapshot,
        None => state
            .delegation_service
            .snapshot(job_id)
            .await
            .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegation job not found"))?,
    };
    if snapshot.status != "cancelled" {
        return Ok(snapshot);
    }
    state
        .app_state
        .delegated_session_repo
        .update_status(
            &DelegatedSessionId::from_string(snapshot.delegated_session_id.clone()),
            "cancelled",
            None,
            Some(Utc::now()),
        )
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to update delegated session cancellation: {error}"),
            )
        })?;
    Ok(snapshot)
}
