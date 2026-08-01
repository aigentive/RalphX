use super::*;
use crate::domain::entities::DelegationParkState;

fn delegated_event_seq() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).unwrap_or_default()
}

fn delegated_total_tokens(latest_run: &DelegatedRunSummary) -> Option<u64> {
    latest_run.processed_tokens
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

fn delegation_assignment_summary(
    assignment: &AgentTaskAssignmentView,
) -> DelegationAssignmentSummary {
    DelegationAssignmentSummary {
        task_number: assignment.task.task_number,
        title: assignment.task.title.clone(),
        task_state: assignment.task.state.as_str().to_string(),
        assignment_state: assignment.assignment.state.as_str().to_string(),
        delegate_agent_name: assignment.assignment.delegate_agent_name.clone(),
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
        started_at: payload.started_at.clone(),
        completed_at: payload.completed_at.clone(),
        timestamp_provenance: payload.timestamp_provenance.clone(),
        seq: Some(payload.seq),
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
        started_at: payload.started_at.clone(),
        completed_at: payload.completed_at.clone(),
        timestamp_provenance: payload.timestamp_provenance.clone(),
        seq: Some(payload.seq),
    }
}

pub(crate) async fn fail_started_delegated_launch(
    state: &HttpServerState,
    chat_service: &dyn ChatService,
    delegated_session_id: &str,
    delegated_agent_run_id: &str,
    error_message: &str,
) -> Result<(), JsonError> {
    let stop_result = chat_service
        .stop_agent(ChatContextType::Delegation, delegated_session_id)
        .await;
    let cancel_result = state
        .app_state
        .agent_run_repo
        .cancel(&AgentRunId::from_string(delegated_agent_run_id.to_string()))
        .await;

    match (stop_result, cancel_result) {
        (Ok(true), Ok(())) => {
            mark_delegated_launch_failed(state, delegated_session_id, error_message).await
        }
        (stop_result, cancel_result) => {
            let stop_detail = match stop_result {
                Ok(false) => "no running delegated process was found".to_string(),
                Ok(true) => "delegated process stop succeeded".to_string(),
                Err(error) => format!("delegated process stop failed: {error}"),
            };
            let cancel_detail = match cancel_result {
                Ok(()) => "durable run cancellation succeeded".to_string(),
                Err(error) => format!("durable run cancellation failed: {error}"),
            };
            warn!(
                delegated_session_id,
                delegated_agent_run_id,
                stop_detail,
                cancel_detail,
                "Delegated launch cleanup could not prove both process termination and durable cancellation; keeping task assignment reserved"
            );
            Err(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "{error_message}; delegated launch cleanup was incomplete ({stop_detail}; {cancel_detail}); the task assignment remains reserved for recovery"
                ),
            ))
        }
    }
}

async fn cache_delegated_parent_task(
    cache: &StreamingStateCache,
    conversation_id: &str,
    parent_run_id: Option<&str>,
    task: CachedStreamingTask,
) {
    if let Some(parent_run_id) = parent_run_id {
        let _ = cache
            .add_task_for_run(conversation_id, parent_run_id, task)
            .await;
    } else {
        cache.add_task(conversation_id, task).await;
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
        run_id: snapshot.parent_agent_run_id.clone(),
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
        started_at: Some(snapshot.started_at.clone()),
        completed_at: snapshot.completed_at.clone(),
        timestamp_provenance: Some("delegation_job".to_string()),
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
    let (started_at, completed_at, timestamp_provenance) = if let Some(run) = latest_run {
        (
            Some(run.started_at.clone()),
            run.completed_at.clone(),
            Some("delegated_run".to_string()),
        )
    } else {
        (
            Some(snapshot.started_at.clone()),
            snapshot.completed_at.clone(),
            Some("delegation_job".to_string()),
        )
    };
    Some(AgentTaskCompletedPayload {
        tool_use_id,
        run_id: snapshot.parent_agent_run_id.clone(),
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
        started_at,
        completed_at,
        timestamp_provenance,
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
    let processed_tokens = run.processed_tokens();
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
        processed_tokens,
        estimated_usd: run.estimated_usd,
    }
}

async fn resolve_current_delegated_run(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    launch_run_id: Option<&str>,
) -> crate::error::AppResult<Option<AgentRun>> {
    match state
        .app_state
        .agent_run_repo
        .get_latest_for_conversation(conversation_id)
        .await?
    {
        Some(run) => Ok(Some(run)),
        None => match launch_run_id {
            Some(run_id) => {
                state
                    .app_state
                    .agent_run_repo
                    .get_by_id(&AgentRunId::from_string(run_id.to_string()))
                    .await
            }
            None => Ok(None),
        },
    }
}

async fn settle_delegation_from_run(
    state: &HttpServerState,
    job_id: &str,
    mut run: AgentRun,
    mut completed_content: Option<String>,
) -> crate::error::AppResult<Option<DelegationJobSnapshot>> {
    if run.status == crate::domain::entities::AgentRunStatus::Running {
        return Ok(None);
    }
    let registered = state.delegation_service.snapshot(job_id).await;
    if let Some(snapshot) = registered.as_ref() {
        if snapshot.status != "running" {
            return Ok(Some(snapshot.clone()));
        }
    }

    // Nested delegation: a delegate remains unfinished through arm, wake dispatch, and the gap
    // before its resumed run exists. Fail closed on park/run reads so stale launch output can never
    // authorize parent settlement.
    if let Some(delegated_conversation_id) = registered
        .as_ref()
        .and_then(|snapshot| snapshot.delegated_conversation_id.as_deref())
    {
        let conversation_id =
            ChatConversationId::from_string(delegated_conversation_id.to_string());
        if let Some(park) = state
            .app_state
            .delegation_park_repo
            .get_settlement_blocking_for_conversation(&conversation_id)
            .await?
        {
            match park.state {
                DelegationParkState::Armed | DelegationParkState::Waking => return Ok(None),
                DelegationParkState::Woken => {
                    let Some(current_run) = resolve_current_delegated_run(
                        state,
                        &conversation_id,
                        registered
                            .as_ref()
                            .and_then(|snapshot| snapshot.delegated_agent_run_id.as_deref()),
                    )
                    .await?
                    else {
                        return Ok(None);
                    };
                    let parked_run_id = park.parent_agent_run_id.as_str();
                    let is_resumed_run = current_run.id != park.parent_agent_run_id
                        && (current_run.parent_run_id.as_deref() == Some(parked_run_id.as_str())
                            || park
                                .wake_claimed_at
                                .is_some_and(|claimed_at| current_run.started_at >= claimed_at));
                    if current_run.status == crate::domain::entities::AgentRunStatus::Running {
                        return Ok(None);
                    }
                    if !is_resumed_run {
                        if !park.is_expired(Utc::now()) {
                            return Ok(None);
                        }
                        tracing::warn!(
                            park_id = %park.id,
                            parent_conversation_id = %park.parent_conversation_id,
                            current_run_id = %current_run.id,
                            "expired woken delegation park no longer blocks parent settlement"
                        );
                    }
                    if current_run.id != run.id {
                        completed_content = if current_run.status
                            == crate::domain::entities::AgentRunStatus::Completed
                        {
                            state
                                .app_state
                                .chat_message_repo
                                .get_by_conversation(&conversation_id)
                                .await
                                .ok()
                                .and_then(latest_delegated_handoff_message)
                                .map(|message| message.content)
                        } else {
                            None
                        };
                        run = current_run;
                    }
                }
                DelegationParkState::Superseded
                | DelegationParkState::Expired
                | DelegationParkState::Failed => {}
            }
        }
    }

    let (status, error) = match run.status {
        crate::domain::entities::AgentRunStatus::Running => return Ok(None),
        crate::domain::entities::AgentRunStatus::Completed => ("completed", None),
        crate::domain::entities::AgentRunStatus::Failed => (
            "failed",
            Some(
                run.error_message
                    .clone()
                    .unwrap_or_else(|| "Delegated run failed".to_string()),
            ),
        ),
        crate::domain::entities::AgentRunStatus::Cancelled => ("cancelled", None),
    };

    let terminal_status = match run.status {
        crate::domain::entities::AgentRunStatus::Completed => {
            AgentTaskAssignmentTerminalStatus::Completed
        }
        crate::domain::entities::AgentRunStatus::Failed => {
            AgentTaskAssignmentTerminalStatus::Failed
        }
        crate::domain::entities::AgentRunStatus::Cancelled => {
            AgentTaskAssignmentTerminalStatus::Cancelled
        }
        crate::domain::entities::AgentRunStatus::Running => return Ok(None),
    };
    let latest_run = delegated_run_summary(run);
    let Some(mut candidate) = state
        .delegation_service
        .terminal_candidate(job_id, status, completed_content, error.clone())
        .await
    else {
        return Ok(None);
    };

    let assignment_service = AgentTaskService::new(state.app_state.agent_task_repo.clone());
    let assignment = if let Some(settlement) = assignment_service
        .settle_assignment_for_run(
            &AgentRunId::from_string(latest_run.agent_run_id.clone()),
            terminal_status,
            error.as_deref(),
        )
        .await?
    {
        Some(settlement.assignment)
    } else {
        assignment_service
            .get_assignment_for_run(&AgentRunId::from_string(latest_run.agent_run_id.clone()))
            .await?
    };
    if let Some(assignment) = assignment {
        state
            .app_state
            .managed_team
            .settle_member_assignment(&assignment, terminal_status, error.as_deref())
            .await?;
        candidate.assignment = Some(delegation_assignment_summary(&assignment));
    }
    persist_terminal_projection(
        &state.app_state.chat_timeline_repo,
        &candidate,
        Some(&latest_run),
    )
    .await?;
    state
        .app_state
        .delegated_session_repo
        .update_status(
            &DelegatedSessionId::from_string(candidate.delegated_session_id.clone()),
            status,
            error.clone(),
            Some(Utc::now()),
        )
        .await?;

    if !state
        .delegation_service
        .commit_terminal(candidate.clone())
        .await
    {
        return Ok(None);
    }

    if let Some(payload) = build_delegated_task_completed_payload(
        &candidate,
        Some(&latest_run),
        status,
        candidate.content.as_deref(),
        error.as_deref(),
        delegated_event_seq(),
    ) {
        cache_delegated_parent_task(
            &state.app_state.streaming_state_cache,
            &payload.conversation_id,
            payload.run_id.as_deref(),
            cached_streaming_task_from_completed_payload(&payload),
        )
        .await;
        if let Err(error) = ralphx_events::emit_serialized(
            state.app_state.events.as_ref(),
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

    // Effects strictly after authority: the wake is dispatched only once `commit_terminal` has
    // accepted this terminal above. Failures here are non-fatal because startup reconciliation
    // re-derives the same decision from durable park + agent_run state.
    // Key the wake on the run id the park was ARMED against (the registered launch run), not the
    // conversation's newest run. A delegate that itself parked and resumed carries a newer run id,
    // which would match no park row and silently drop the parent's wake.
    let parked_run_id = candidate
        .delegated_agent_run_id
        .clone()
        .unwrap_or_else(|| latest_run.agent_run_id.clone());
    // Wake DELIVERY is decoupled from settlement. Delivering a wake launches the parked
    // coordinator's next turn and, on failure, retries with backoff for up to
    // `park_wake_retry_max * park_wake_retry_backoff_secs`. Awaiting that here would stall the
    // 100ms settlement monitor and any in-flight `delegate_wait` request for minutes.
    //
    // Exactly-once is preserved regardless of ordering: `dispatch_wake` claims the park with an
    // `armed -> waking` CAS, and a dispatcher that dies mid-wake is recovered by the stale-claim
    // sweep. Spawning happens strictly after `commit_terminal` accepted above.
    let park_service = state.app_state.build_delegation_park_service();
    let settled_status = status.to_string();
    let wake_job_id = job_id.to_string();
    tokio::spawn(async move {
        if let Err(error) = park_service
            .note_job_settled(
                &AgentRunId::from_string(parked_run_id.clone()),
                &settled_status,
            )
            .await
        {
            warn!(
                job_id = wake_job_id,
                delegated_agent_run_id = parked_run_id,
                %error,
                "Parked coordinator wake dispatch failed; startup reconciliation will retry"
            );
        }
    });

    Ok(Some(candidate))
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

pub(crate) async fn ensure_delegated_conversation(
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
    delegated_agent_run_id: Option<&str>,
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
        let latest_run = if let Some(agent_run_id) = delegated_agent_run_id {
            let run = state
                .app_state
                .agent_run_repo
                .get_by_id(&AgentRunId::from_string(agent_run_id.to_string()))
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to load delegated run: {error}"),
                    )
                })?
                .ok_or_else(|| {
                    json_error(StatusCode::NOT_FOUND, "Delegated agent run not found")
                })?;
            if run.conversation_id != conversation.id {
                return Err(json_error(
                    StatusCode::CONFLICT,
                    "Delegated agent run does not belong to the delegated conversation",
                ));
            }
            Some(delegated_run_summary(run))
        } else {
            state
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
                .map(delegated_run_summary)
        };
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
        build_delegated_session_status_response(&state, &delegated_session_id, false, None, None)
            .await?;
    Ok(Json(status))
}

async fn resolve_trusted_caller_agent_run_id(
    state: &HttpServerState,
    parent: &ResolvedDelegateParent,
    trusted_caller_conversation_id: Option<&str>,
    trusted_parent_run_id: Option<&str>,
) -> Result<Option<String>, JsonError> {
    let caller_conversation_id = if parent.context_type == ChatContextType::Delegation {
        let resolved = parent.caller_conversation_id.as_deref().ok_or_else(|| {
            json_error(
                StatusCode::NOT_FOUND,
                "Active caller delegated conversation not found",
            )
        })?;
        let trusted = trusted_caller_conversation_id.ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                "Nested delegate_start requires trusted caller conversation context",
            )
        })?;
        if trusted != resolved {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "Trusted caller conversation does not match the resolved caller conversation",
            ));
        }
        Some(resolved)
    } else {
        match (
            trusted_caller_conversation_id,
            parent.caller_conversation_id.as_deref(),
        ) {
            (Some(trusted), Some(resolved)) if trusted != resolved => {
                return Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "Trusted caller conversation does not match the resolved caller conversation",
                ));
            }
            (Some(trusted), _) => Some(trusted),
            (None, resolved) => resolved,
        }
    };
    let Some(caller_conversation_id) = caller_conversation_id else {
        if trusted_parent_run_id.is_some() {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "Trusted caller run requires a resolved caller conversation",
            ));
        }
        return Ok(None);
    };
    let caller_conversation_id = ChatConversationId::from_string(caller_conversation_id);
    let active_run = state
        .app_state
        .agent_run_repo
        .get_active_for_conversation(&caller_conversation_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to resolve current caller agent run: {error}"),
            )
        })?;

    let Some(trusted_parent_run_id) = trusted_parent_run_id else {
        return Ok(active_run.map(|run| run.id.as_str()));
    };
    let trusted_run = state
        .app_state
        .agent_run_repo
        .get_by_id(&AgentRunId::from_string(trusted_parent_run_id.to_string()))
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to validate trusted caller agent run: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Trusted caller agent run not found"))?;
    if trusted_run.conversation_id != caller_conversation_id {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Trusted caller agent run does not belong to the caller conversation",
        ));
    }
    if active_run.as_ref().map(|run| &run.id) != Some(&trusted_run.id) {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Trusted caller agent run is not the active caller run",
        ));
    }
    Ok(Some(trusted_run.id.as_str()))
}

pub(crate) async fn start_delegate_impl_with_parent_run(
    state: &HttpServerState,
    req: DelegateStartRequest,
    trusted_caller_conversation_id: Option<&str>,
    trusted_parent_run_id: Option<&str>,
) -> Result<DelegationJobSnapshot, JsonError> {
    let caller_agent_name = req.caller_agent_name.clone().ok_or_else(|| {
        json_error(
            StatusCode::BAD_REQUEST,
            "delegate_start requires caller_agent_name from the MCP transport",
        )
    })?;
    let parent = resolve_delegate_parent(state, &req).await?;
    let parent_agent_run_id = resolve_trusted_caller_agent_run_id(
        state,
        &parent,
        trusted_caller_conversation_id,
        trusted_parent_run_id,
    )
    .await?;
    if req.task_ref.is_some() && parent_agent_run_id.is_none() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "delegate_start task_ref requires a trusted active caller run",
        ));
    }
    let reusable_delegated_session =
        preflight_requested_delegated_session(state, &req, &parent).await?;
    let launch = NativeDelegationLauncher::new(state)
        .launch(NativeDelegationLaunchRequest {
            caller_agent_name,
            caller_agent_profile: req.caller_agent_profile.clone(),
            parent: NativeDelegationLaunchParent {
                context_type: parent.context_type,
                context_id: parent.context_id,
                project_id: parent.project_id,
                working_directory: parent.working_directory,
                caller_conversation_id: parent.caller_conversation_id,
                parent_conversation_id: parent.parent_conversation_id,
                ideation_verification: parent.ideation_verification,
            },
            caller_agent_run_id: parent_agent_run_id,
            target_agent_name: req.agent_name.clone(),
            reusable_delegated_session,
            task_ref: req.task_ref.clone(),
            preallocated_agent_run_id: None,
            prompt: req.prompt.clone(),
            title: req.title.clone(),
            parent_turn_id: req.parent_turn_id.clone(),
            parent_message_id: req.parent_message_id.clone(),
            parent_tool_use_id: req.parent_tool_use_id.clone(),
            harness: req.harness,
            model: req.model.clone(),
            logical_effort: req.logical_effort,
            approval_policy: req.approval_policy.clone(),
            sandbox_mode: req.sandbox_mode.clone(),
        })
        .await?;
    let parent = launch.parent.clone();
    let parent_agent_run_id = launch.caller_agent_run_id.clone();
    let delegated_session_id = launch.delegated_session_id.clone();
    let delegated_conversation_id = launch.delegated_conversation_id.clone();
    let delegated_agent_run_id = launch.delegated_agent_run_id.clone();
    let bound_assignment = launch.assignment.clone();
    let harness = launch.harness;
    let launched_run = launch.launched_run.clone();
    let delegated_model = launch.logical_model.clone();
    let logical_effort = launch.logical_effort;
    let approval_policy = launch.approval_policy.clone();
    let sandbox_mode = launch.sandbox_mode.clone();

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
            parent_agent_run_id,
            req.parent_tool_use_id.clone(),
            delegated_session_id.clone(),
            Some(delegated_conversation_id.clone()),
            Some(delegated_agent_run_id.clone()),
            launch.agent_name.clone(),
            bound_assignment.as_ref().map(delegation_assignment_summary),
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
        if let Some(parent_run_id) = payload.run_id.clone() {
            state
                .app_state
                .streaming_state_cache
                .set_run_id(&payload.conversation_id, Some(parent_run_id))
                .await;
        }
        cache_delegated_parent_task(
            &state.app_state.streaming_state_cache,
            &payload.conversation_id,
            payload.run_id.as_deref(),
            cached_streaming_task_from_started_payload(&payload),
        )
        .await;
        crate::http_server::emit_serialized_http_event(state, events::AGENT_TASK_STARTED, &payload);
    }

    let monitor_state = state.clone();
    let launch_run_id = delegated_agent_run_id;
    let conversation_id = ChatConversationId::from_string(delegated_conversation_id);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            // Resolve the delegated conversation's CURRENT run, not the launch-time run id.
            // A delegate that parks and is later woken resumes on a NEW run; watching the
            // launch run would settle this job on a stale, already-terminal attempt while the
            // delegate is still working.
            let run = match resolve_current_delegated_run(
                &monitor_state,
                &conversation_id,
                Some(&launch_run_id),
            )
            .await
            {
                Ok(Some(run)) => run,
                Ok(None) => continue,
                Err(error) => {
                    warn!(job_id, %error, "Delegated run state read failed; settlement remains pending");
                    continue;
                }
            };

            if run.status == crate::domain::entities::AgentRunStatus::Running {
                continue;
            }

            let completed_content =
                if run.status == crate::domain::entities::AgentRunStatus::Completed {
                    let mut content = String::new();
                    for _ in 0..10 {
                        content = monitor_state
                            .app_state
                            .chat_message_repo
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
                    Some(content)
                } else {
                    None
                };

            match settle_delegation_from_run(&monitor_state, &job_id, run, completed_content).await
            {
                Ok(Some(_)) => break,
                Ok(None) => continue,
                Err(error) => {
                    warn!(job_id, %error, "Delegated terminal settlement remains pending");
                    continue;
                }
            }
        }
    });

    Ok(snapshot)
}
pub(crate) async fn start_delegate_impl(
    state: &HttpServerState,
    req: DelegateStartRequest,
) -> Result<DelegationJobSnapshot, JsonError> {
    start_delegate_impl_with_parent_run(state, req, None, None).await
}

pub async fn start_delegate(
    State(state): State<HttpServerState>,
    Json(req): Json<DelegateStartRequest>,
) -> Result<Json<DelegationJobSnapshot>, JsonError> {
    Ok(Json(start_delegate_impl(&state, req).await?))
}

/// Fail-closed 400 for delegation calls whose MCP transport carries no run identity.
/// This indicates the caller's spawn lane did not inject `--agent-run-id` into the MCP
/// runtime context (a spawn-time injection gap), not a delegation policy denial.
pub const DELEGATION_MISSING_RUN_IDENTITY_ERROR: &str = "Delegation start requires trusted parent agent run context, but this agent's MCP runtime context has no run identity: the spawn lane did not inject --agent-run-id at launch. This is a spawn-lane injection gap (fail-closed by design), not a delegation policy denial for this agent";

pub const DELEGATION_INVALID_RUN_IDENTITY_ERROR: &str =
    "Trusted parent agent run identity header is invalid";

pub async fn start_delegate_with_runtime_context(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(req): Json<DelegateStartRequest>,
) -> Result<Json<DelegationJobSnapshot>, JsonError> {
    let trusted_caller_conversation_id = headers
        .get("x-ralphx-conversation-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let trusted_parent_run_id = headers
        .get("x-ralphx-agent-run-id")
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                DELEGATION_MISSING_RUN_IDENTITY_ERROR,
            )
        })?
        .to_str()
        .map_err(|_| {
            json_error(
                StatusCode::BAD_REQUEST,
                DELEGATION_INVALID_RUN_IDENTITY_ERROR,
            )
        })?
        .trim();
    if trusted_parent_run_id.is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            DELEGATION_MISSING_RUN_IDENTITY_ERROR,
        ));
    }
    Ok(Json(
        start_delegate_impl_with_parent_run(
            &state,
            req,
            trusted_caller_conversation_id,
            Some(trusted_parent_run_id),
        )
        .await?,
    ))
}

/// Resolves the caller's watch set, rejecting ambiguous or empty requests.
///
/// Exactly one of `job_id` / `job_ids` must be present so a caller can never silently
/// wait on a different set than it named.
fn resolve_wait_job_ids(req: &DelegateWaitRequest) -> Result<Vec<String>, JsonError> {
    match (req.job_id.as_deref(), req.job_ids.as_deref()) {
        (Some(_), Some(_)) => Err(json_error(
            StatusCode::BAD_REQUEST,
            "delegate_wait accepts either job_id or job_ids, not both",
        )),
        (None, None) => Err(json_error(
            StatusCode::BAD_REQUEST,
            "delegate_wait requires job_id or job_ids",
        )),
        (Some(job_id), None) => Ok(vec![job_id.to_string()]),
        (None, Some(job_ids)) => {
            if job_ids.is_empty() {
                return Err(json_error(
                    StatusCode::BAD_REQUEST,
                    "delegate_wait job_ids must not be empty",
                ));
            }
            Ok(job_ids.to_vec())
        }
    }
}

/// Effective block ceiling: the caller's request clamped to the configured hard cap, which is
/// itself held strictly below the stream parse-stall guard so a legitimate block can never be
/// mistaken for a stalled coordinator stream.
#[doc(hidden)]
pub fn effective_wait_block(requested_ms: u64) -> Duration {
    let delegation = crate::infrastructure::agents::claude::delegation_config();
    let stall_guard_secs =
        crate::infrastructure::agents::claude::stream_timeouts().default_parse_stall_secs;
    // Never let config drift hand out a block that outlives the guard that would kill the caller.
    let safe_cap_secs = delegation
        .wait_block_max_secs
        .min(stall_guard_secs.saturating_sub(30).max(1));
    let requested_ms = if requested_ms == 0 {
        delegation.wait_block_secs.saturating_mul(1_000)
    } else {
        requested_ms
    };
    Duration::from_millis(requested_ms.min(safe_cap_secs.saturating_mul(1_000)))
}

/// Reconcile-then-read for a single job: mirrors the historical `delegate_wait` body so a
/// blocking wait and an immediate wait return identical snapshots.
async fn resolve_or_settle_job(
    state: &HttpServerState,
    job_id: &str,
) -> Result<DelegationJobSnapshot, JsonError> {
    let mut snapshot = state
        .delegation_service
        .snapshot(job_id)
        .await
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegation job not found"))?;
    if snapshot.status == "running" {
        if let Some(launch_run_id) = snapshot.delegated_agent_run_id.as_deref() {
            let run =
                if let Some(conversation_id) = snapshot.delegated_conversation_id.as_deref() {
                    resolve_current_delegated_run(
                        state,
                        &ChatConversationId::from_string(conversation_id.to_string()),
                        Some(launch_run_id),
                    )
                    .await
                } else {
                    state
                        .app_state
                        .agent_run_repo
                        .get_by_id(&AgentRunId::from_string(launch_run_id.to_string()))
                        .await
                }
                .map_err(|error| {
                    json_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("Delegated run reconciliation is pending: {error}"),
                    )
                })?;
            if let Some(run) = run {
                if run.status != crate::domain::entities::AgentRunStatus::Running {
                    let completed_content = if run.status
                        == crate::domain::entities::AgentRunStatus::Completed
                    {
                        if let Some(conversation_id) = snapshot.delegated_conversation_id.as_deref()
                        {
                            state
                                .app_state
                                .chat_message_repo
                                .get_by_conversation(&ChatConversationId::from_string(
                                    conversation_id.to_string(),
                                ))
                                .await
                                .ok()
                                .and_then(latest_delegated_handoff_message)
                                .map(|message| message.content)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(settled) =
                        settle_delegation_from_run(state, job_id, run, completed_content)
                            .await
                            .map_err(|error| {
                                json_error(
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    format!("Delegated run settlement is pending: {error}"),
                                )
                            })?
                    {
                        snapshot = settled;
                    }
                }
            }
        }
    }
    Ok(snapshot)
}

pub async fn wait_delegate(
    State(state): State<HttpServerState>,
    Json(req): Json<DelegateWaitRequest>,
) -> Result<Json<DelegationJobSnapshot>, JsonError> {
    let job_ids = resolve_wait_job_ids(&req)?;

    // Subscribe BEFORE the reconcile read so a settlement racing this call is never missed.
    let mut receivers = Vec::with_capacity(job_ids.len());
    for job_id in &job_ids {
        let receiver = state
            .delegation_service
            .subscribe_settlement(job_id)
            .await
            .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegation job not found"))?;
        receivers.push(receiver);
    }

    let mut snapshot: Option<DelegationJobSnapshot> = None;
    for job_id in &job_ids {
        let resolved = resolve_or_settle_job(&state, job_id).await?;
        let is_terminal = resolved.status != "running";
        if snapshot.is_none() || is_terminal {
            snapshot = Some(resolved);
        }
        if is_terminal {
            break;
        }
    }
    let mut snapshot =
        snapshot.ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Delegation job not found"))?;

    // Default is unchanged: without `wait_timeout_ms` this returns immediately, exactly as before.
    if snapshot.status == "running" {
        if let Some(requested_ms) = req.wait_timeout_ms {
            let deadline = effective_wait_block(requested_ms);
            match block_until_settlement(receivers, deadline).await {
                Some(index) => {
                    snapshot = resolve_or_settle_job(&state, &job_ids[index]).await?;
                }
                None => snapshot.timed_out = Some(true),
            }
        }
    }

    if req
        .include_delegated_status
        .or(req.include_child_status)
        .unwrap_or(true)
    {
        let delegated_status = build_delegated_session_status_response(
            &state,
            &snapshot.delegated_session_id,
            req.include_messages.unwrap_or(false),
            req.message_limit,
            snapshot.delegated_agent_run_id.as_deref(),
        )
        .await?;
        snapshot.delegated_status = Some(delegated_status);
    }
    Ok(Json(snapshot))
}

/// Blocks until any watched job broadcasts a committed terminal, or the deadline elapses.
///
/// Returns the index of the settled job, or `None` on timeout. The settlement signal is only
/// ever sent after `commit_terminal` accepts, so a wake here is proof of durable settlement.
async fn block_until_settlement(
    receivers: Vec<tokio::sync::watch::Receiver<Option<String>>>,
    deadline: Duration,
) -> Option<usize> {
    let mut waits: futures::stream::FuturesUnordered<_> = receivers
        .into_iter()
        .enumerate()
        .map(|(index, mut receiver)| async move {
            // `changed()` resolves on the next send; a settlement that already landed before
            // subscription is caught by the reconcile read in the caller.
            let _ = receiver.changed().await;
            index
        })
        .collect();

    tokio::select! {
        settled = futures::StreamExt::next(&mut waits) => settled,
        () = tokio::time::sleep(deadline) => None,
    }
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
    let cancellation_already_pending = state
        .delegation_service
        .is_cancellation_pending(job_id)
        .await;
    if !cancellation_already_pending {
        state
            .delegation_service
            .begin_cancellation(job_id)
            .await
            .ok_or_else(|| {
                json_error(
                    StatusCode::CONFLICT,
                    "Delegation job cancellation is already being settled",
                )
            })?;

        let chat_service = state
            .app_state
            .build_chat_service_with_execution_state(Arc::clone(&state.execution_state));
        let stopped = match chat_service
            .stop_agent(ChatContextType::Delegation, &snapshot.delegated_session_id)
            .await
        {
            Ok(stopped) => stopped,
            Err(error) => {
                state.delegation_service.abort_cancellation(job_id).await;
                return Err(json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to stop delegated agent: {error}"),
                ));
            }
        };
        if !stopped {
            state.delegation_service.abort_cancellation(job_id).await;
            return Err(json_error(
                StatusCode::NOT_FOUND,
                "Delegation job is no longer running",
            ));
        }
    }

    let delegated_run_id = snapshot.delegated_agent_run_id.as_deref().ok_or_else(|| {
        json_error(
            StatusCode::CONFLICT,
            "Delegation job has no authoritative agent run",
        )
    })?;
    let run_id = AgentRunId::from_string(delegated_run_id.to_string());
    let run = state
        .app_state
        .agent_run_repo
        .get_by_id(&run_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Cancellation accepted; delegated run reconciliation is pending: {error}"),
            )
        })?
        .ok_or_else(|| {
            json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Cancellation accepted; delegated run reconciliation is pending",
            )
        })?;

    match run.status {
        crate::domain::entities::AgentRunStatus::Completed => {
            state.delegation_service.abort_cancellation(job_id).await;
            let settled = settle_delegation_from_run(state, job_id, run, None)
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("Delegated completion settlement is pending: {error}"),
                    )
                })?;
            return settled.ok_or_else(|| {
                json_error(
                    StatusCode::CONFLICT,
                    "Delegation completed before cancellation was accepted",
                )
            });
        }
        crate::domain::entities::AgentRunStatus::Failed
            if run.error_message.as_deref() != Some("Agent stopped by user") =>
        {
            state.delegation_service.abort_cancellation(job_id).await;
            let settled = settle_delegation_from_run(state, job_id, run, None)
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("Delegated failure settlement is pending: {error}"),
                    )
                })?;
            return settled.ok_or_else(|| {
                json_error(
                    StatusCode::CONFLICT,
                    "Delegation failed before cancellation was accepted",
                )
            });
        }
        crate::domain::entities::AgentRunStatus::Running
        | crate::domain::entities::AgentRunStatus::Failed => {
            state
                .app_state
                .agent_run_repo
                .cancel(&run_id)
                .await
                .map_err(|error| {
                    json_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!(
                            "Cancellation accepted; delegated run settlement is pending: {error}"
                        ),
                    )
                })?;
        }
        crate::domain::entities::AgentRunStatus::Cancelled => {}
    }

    let cancelled_run = state
        .app_state
        .agent_run_repo
        .get_by_id(&run_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Cancellation accepted; delegated run settlement is pending: {error}"),
            )
        })?
        .ok_or_else(|| {
            json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Cancellation accepted; delegated run settlement is pending",
            )
        })?;
    if cancelled_run.status != crate::domain::entities::AgentRunStatus::Cancelled {
        if matches!(
            cancelled_run.status,
            crate::domain::entities::AgentRunStatus::Completed
        ) || (cancelled_run.status == crate::domain::entities::AgentRunStatus::Failed
            && cancelled_run.error_message.as_deref() != Some("Agent stopped by user"))
        {
            state.delegation_service.abort_cancellation(job_id).await;
            return Err(json_error(
                StatusCode::CONFLICT,
                "Delegation reached a terminal state before cancellation settlement",
            ));
        }
        return Err(json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Cancellation accepted; delegated run has not reached cancelled state",
        ));
    }

    settle_delegation_from_run(state, job_id, cancelled_run, None)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("Cancellation accepted; terminal projection is pending: {error}"),
            )
        })?
        .ok_or_else(|| {
            json_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Cancellation accepted; terminal projection is pending",
            )
        })
}
