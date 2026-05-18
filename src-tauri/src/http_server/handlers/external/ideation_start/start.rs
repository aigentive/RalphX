use super::*;
use crate::application::ideation_service::build_default_ideation_session_title;
use crate::application::ideation_workspace::{
    prepare_ideation_analysis_state, prepare_ideation_analysis_state_from_agent_workspace,
    IdeationAnalysisBaseSelection,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisState, Project,
};
use crate::http_server::handlers::external_auth::TAURI_MCP_HEADER;

const PARENT_CONVERSATION_HEADER: &str = "x-ralphx-parent-conversation-id";

#[derive(Debug, Deserialize)]
pub struct StartIdeationRequest {
    pub project_id: String,
    pub title: Option<String>,
    pub prompt: Option<String>,
    pub initial_prompt: Option<String>,
    pub idempotency_key: Option<String>,
}

/// Lightweight summary of an active external session for dedup awareness.
#[derive(Debug, Serialize, Clone)]
pub struct ExternalSessionSummary {
    pub session_id: String,
    pub title: Option<String>,
    pub status: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_activity_phase: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StartIdeationResponse {
    pub session_id: String,
    pub status: String,
    pub agent_spawned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_spawn_blocked_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_initial_prompt: Option<String>,
    /// All active external sessions for the project (for agent visibility)
    pub existing_active_sessions: Vec<ExternalSessionSummary>,
    /// True if this response reuses an existing session due to idempotency key match
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exists: Option<bool>,
    /// True if this response reuses an existing session due to Jaccard similarity match
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_detected: Option<bool>,
    /// Jaccard similarity score when duplicate_detected is true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity_score: Option<f64>,
    /// Behavioral hint for the caller
    pub next_action: String,
    /// Human-readable hint message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_branch: Option<String>,
}

struct ParentWorkspaceBinding {
    conversation_id: ChatConversationId,
    workspace: AgentConversationWorkspace,
    analysis: IdeationAnalysisState,
}

fn is_tauri_mcp_request(headers: &HeaderMap) -> bool {
    headers
        .get(TAURI_MCP_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|value| value == "1")
        .unwrap_or(false)
}

fn parent_conversation_id_from_headers(headers: &HeaderMap) -> Option<ChatConversationId> {
    if !is_tauri_mcp_request(headers) {
        return None;
    }

    headers
        .get(PARENT_CONVERSATION_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| ChatConversationId::from_string(value.to_string()))
}

async fn resolve_parent_workspace_binding(
    state: &HttpServerState,
    project: &Project,
    parent_conversation_id: Option<ChatConversationId>,
) -> Result<Option<ParentWorkspaceBinding>, HttpError> {
    let Some(conversation_id) = parent_conversation_id else {
        return Ok(None);
    };

    let conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| {
            error!(
                "Failed to load parent conversation {} for external ideation workspace binding: {}",
                conversation_id.as_str(),
                error
            );
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to load parent conversation".to_string()),
            }
        })?
        .ok_or_else(|| HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("Parent conversation not found".to_string()),
        })?;

    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != project.id.as_str()
    {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(
                "Parent conversation does not belong to the requested project".to_string(),
            ),
        });
    }

    let workspace = state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| {
            error!(
                "Failed to load parent conversation workspace {}: {}",
                conversation_id.as_str(),
                error
            );
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to load parent conversation workspace".to_string()),
            }
        })?
        .ok_or_else(|| HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("Parent conversation has no agent workspace".to_string()),
        })?;

    if workspace.mode != AgentConversationWorkspaceMode::Ideation {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("Parent conversation workspace is not in ideation mode".to_string()),
        });
    }

    let analysis = prepare_ideation_analysis_state_from_agent_workspace(project, &workspace)
        .await
        .map_err(|error| HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(error.to_string()),
        })?;

    Ok(Some(ParentWorkspaceBinding {
        conversation_id,
        workspace,
        analysis,
    }))
}

/// POST /api/external/start_ideation
/// Create a new ideation session for a project.
pub async fn start_ideation_http(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    headers: HeaderMap,
    Json(req): Json<StartIdeationRequest>,
) -> Result<Json<StartIdeationResponse>, HttpError> {
    let project_id = ProjectId::from_string(req.project_id.clone());

    let project = state
        .app_state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|e| {
            error!("Failed to get project {}: {}", project_id.as_str(), e);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to get project".to_string()),
            }
        })?
        .ok_or(HttpError {
            status: StatusCode::NOT_FOUND,
            message: Some("Project not found".to_string()),
        })?;

    project
        .assert_project_scope(&scope)
        .map_err(|e| HttpError {
            status: e.status,
            message: e.message,
        })?;

    let parent_workspace_binding = resolve_parent_workspace_binding(
        &state,
        &project,
        parent_conversation_id_from_headers(&headers),
    )
    .await?;

    let api_key_id = headers
        .get(crate::http_server::handlers::external_auth::EXTERNAL_KEY_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if let (Some(ref key_id), Some(ref idem_key)) = (&api_key_id, &req.idempotency_key) {
        if let Ok(Some(existing)) = state
            .app_state
            .ideation_session_repo
            .get_by_idempotency_key(key_id, idem_key)
            .await
        {
            let active_sessions = state
                .app_state
                .ideation_session_repo
                .list_active_external_by_project(&project_id)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|s| ExternalSessionSummary {
                    session_id: s.id.to_string(),
                    title: s.title.clone(),
                    status: s.status.to_string(),
                    created_at: s.created_at.to_rfc3339(),
                    external_activity_phase: s.external_activity_phase.clone(),
                })
                .collect::<Vec<_>>();
            return Ok(Json(StartIdeationResponse {
                session_id: existing.id.to_string(),
                status: existing.status.to_string(),
                agent_spawned: false,
                agent_spawn_blocked_reason: None,
                pending_initial_prompt: existing.pending_initial_prompt.clone(),
                existing_active_sessions: active_sessions,
                exists: Some(true),
                duplicate_detected: None,
                similarity_score: None,
                next_action: "poll_status".to_string(),
                hint: Some("Idempotent retry: returning existing session.".to_string()),
                parent_conversation_id: None,
                workspace_branch: None,
            }));
        }
    }

    let active_sessions = state
        .app_state
        .ideation_session_repo
        .list_active_external_by_project(&project_id)
        .await
        .unwrap_or_default();

    let effective_prompt = req.prompt.clone().or_else(|| req.initial_prompt.clone());
    let has_candidate_text = req.prompt.is_some() || req.title.is_some();

    if has_candidate_text && !active_sessions.is_empty() {
        let candidate_text = format!(
            "{} {}",
            req.prompt.as_deref().unwrap_or(""),
            req.title.as_deref().unwrap_or("")
        );
        let candidate_tokens = tokenize_for_similarity(&candidate_text);
        let similarity_threshold =
            crate::application::harness_runtime_registry::default_external_session_similarity_threshold();

        let mut best_match: Option<(f64, &crate::domain::entities::ideation::IdeationSession)> =
            None;
        for session in &active_sessions {
            let session_title = session.title.as_deref().unwrap_or("");
            let first_msg = state
                .app_state
                .chat_message_repo
                .get_first_user_message_by_context("ideation", session.id.as_str())
                .await
                .unwrap_or_default()
                .unwrap_or_default();
            let comparison_text = format!("{} {}", session_title, first_msg);
            let comparison_tokens = tokenize_for_similarity(&comparison_text);
            let score = jaccard_similarity(&candidate_tokens, &comparison_tokens);
            if score >= similarity_threshold && best_match.map(|(s, _)| score > s).unwrap_or(true) {
                best_match = Some((score, session));
            }
        }

        if let Some((score, matched_session)) = best_match {
            let active_summaries = active_sessions
                .iter()
                .map(|s| ExternalSessionSummary {
                    session_id: s.id.to_string(),
                    title: s.title.clone(),
                    status: s.status.to_string(),
                    created_at: s.created_at.to_rfc3339(),
                    external_activity_phase: s.external_activity_phase.clone(),
                })
                .collect::<Vec<_>>();
            let hint_msg = format!(
                "A similar session already exists ('{}', {:.0}% match). Reusing it instead of creating a duplicate.",
                matched_session.title.as_deref().unwrap_or("untitled"),
                score * 100.0
            );
            return Ok(Json(StartIdeationResponse {
                session_id: matched_session.id.to_string(),
                status: matched_session.status.to_string(),
                agent_spawned: false,
                agent_spawn_blocked_reason: None,
                pending_initial_prompt: matched_session.pending_initial_prompt.clone(),
                existing_active_sessions: active_summaries,
                exists: None,
                duplicate_detected: Some(true),
                similarity_score: Some(score),
                next_action: "use_existing_session".to_string(),
                hint: Some(hint_msg),
                parent_conversation_id: None,
                workspace_branch: None,
            }));
        }
    }

    let session_id = IdeationSessionId::new();
    let analysis = match parent_workspace_binding.as_ref() {
        Some(binding) => binding.analysis.clone(),
        None => prepare_ideation_analysis_state(
            &project,
            &session_id,
            IdeationAnalysisBaseSelection::default(),
        )
        .await
        .map_err(|error| HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(error.to_string()),
        })?,
    };

    let mut session_builder = match req.title.clone() {
        None => IdeationSession::new_with_title(
            project_id.clone(),
            build_default_ideation_session_title(),
        ),
        Some(t) => IdeationSession::new_with_title(project_id.clone(), t),
    };
    session_builder.id = session_id;
    session_builder.analysis = analysis;
    session_builder.origin = SessionOrigin::External;
    session_builder.external_activity_phase = Some("created".to_string());
    if let Some(ref key_id) = api_key_id {
        session_builder.api_key_id = Some(key_id.clone());
    }
    if let Some(ref idem_key) = req.idempotency_key {
        session_builder.idempotency_key = Some(idem_key.clone());
    }
    let created = match state
        .app_state
        .ideation_session_repo
        .create(session_builder)
        .await
    {
        Ok(session) => session,
        Err(e)
            if e.to_string()
                .to_lowercase()
                .contains(SQLITE_UNIQUE_VIOLATION)
                && api_key_id.is_some()
                && req.idempotency_key.is_some() =>
        {
            if let (Some(ref key_id), Some(ref idem_key)) = (&api_key_id, &req.idempotency_key) {
                if let Ok(Some(existing)) = state
                    .app_state
                    .ideation_session_repo
                    .get_by_idempotency_key(key_id, idem_key)
                    .await
                {
                    let active_summaries = active_sessions
                        .iter()
                        .map(|s| ExternalSessionSummary {
                            session_id: s.id.to_string(),
                            title: s.title.clone(),
                            status: s.status.to_string(),
                            created_at: s.created_at.to_rfc3339(),
                            external_activity_phase: s.external_activity_phase.clone(),
                        })
                        .collect::<Vec<_>>();
                    return Ok(Json(StartIdeationResponse {
                        session_id: existing.id.to_string(),
                        status: existing.status.to_string(),
                        agent_spawned: false,
                        agent_spawn_blocked_reason: None,
                        pending_initial_prompt: existing.pending_initial_prompt.clone(),
                        existing_active_sessions: active_summaries,
                        exists: Some(true),
                        duplicate_detected: None,
                        similarity_score: None,
                        next_action: "poll_status".to_string(),
                        hint: Some(
                            "Idempotent retry (concurrent): returning existing session."
                                .to_string(),
                        ),
                        parent_conversation_id: None,
                        workspace_branch: None,
                    }));
                }
            }
            error!(
                "Failed to create ideation session (unique conflict, re-query failed): {}",
                e
            );
            return Err(HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to create ideation session".to_string()),
            });
        }
        Err(e) => {
            error!("Failed to create ideation session: {}", e);
            return Err(HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to create ideation session".to_string()),
            });
        }
    };

    let session_id_str = created.id.to_string();

    if let Some(binding) = parent_workspace_binding.as_ref() {
        state
            .app_state
            .agent_conversation_workspace_repo
            .update_links(&binding.conversation_id, Some(&created.id), None)
            .await
            .map_err(|error| {
                error!(
                    "Failed to link parent conversation workspace {} to external ideation session {}: {}",
                    binding.conversation_id.as_str(),
                    created.id.as_str(),
                    error
                );
                HttpError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: Some("Failed to link parent workspace".to_string()),
                }
            })?;
    }

    {
        let repo = Arc::clone(&state.app_state.ideation_session_repo);
        let sid = IdeationSessionId::from_string(session_id_str.clone());
        tokio::spawn(async move {
            if let Err(e) = repo
                .update_external_activity_phase(&sid, Some("created"))
                .await
            {
                error!(
                    "Failed to set activity phase 'created' for session {}: {}",
                    sid.as_str(),
                    e
                );
            }
        });
    }

    let session_created_payload = serde_json::json!({
        "sessionId": session_id_str,
        "projectId": project_id.to_string(),
    });
    if let Some(ref handle) = state.app_state.app_handle {
        let _ = handle.emit("ideation:session_created", &session_created_payload);
    }

    if let Err(e) = state
        .app_state
        .external_events_repo
        .insert_event(
            "ideation:session_created",
            &project_id.to_string(),
            &session_created_payload.to_string(),
        )
        .await
    {
        tracing::warn!(error = %e, "Failed to persist IdeationSessionCreated event");
    }

    if let Some(ref publisher) = state.app_state.webhook_publisher {
        let _ = publisher
            .publish(
                EventType::IdeationSessionCreated,
                &project_id.to_string(),
                session_created_payload,
            )
            .await;
    }

    let existing_summaries = {
        let mut summaries: Vec<ExternalSessionSummary> = active_sessions
            .iter()
            .map(|s| ExternalSessionSummary {
                session_id: s.id.to_string(),
                title: s.title.clone(),
                status: s.status.to_string(),
                created_at: s.created_at.to_rfc3339(),
                external_activity_phase: s.external_activity_phase.clone(),
            })
            .collect();
        summaries.insert(
            0,
            ExternalSessionSummary {
                session_id: session_id_str.clone(),
                title: created.title.clone(),
                status: created.status.to_string(),
                created_at: created.created_at.to_rfc3339(),
                external_activity_phase: created.external_activity_phase.clone(),
            },
        );
        summaries
    };

    let mut agent_spawned = false;
    let mut agent_spawn_blocked_reason: Option<String> = None;
    let mut pending_initial_prompt: Option<String> = None;
    if let Some(ref prompt_str) = effective_prompt {
        let chat_service = build_chat_service(&state.app_state, &state.execution_state);

        match chat_service
            .send_message(
                ChatContextType::Ideation,
                &session_id_str,
                prompt_str,
                SendMessageOptions {
                    is_external_mcp: true,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(result) if result.was_queued => {
                if result.queued_as_pending {
                    pending_initial_prompt = Some(prompt_str.clone());
                    agent_spawn_blocked_reason =
                        Some("execution paused; ideation prompt saved for resume".to_string());
                } else {
                    agent_spawned = true;
                }
            }
            Ok(_) => {
                agent_spawned = true;
                spawn_session_namer(
                    &state.app_state,
                    project_id.as_str(),
                    session_id_str.clone(),
                    prompt_str.clone(),
                )
                .await;
            }
            Err(e) => {
                error!(
                    "Failed to auto-spawn agent on external ideation session {}: {}",
                    session_id_str, e
                );
                agent_spawn_blocked_reason = Some(e.to_string());
            }
        }
    }

    let deferred_for_resume = pending_initial_prompt.is_some();

    Ok(Json(StartIdeationResponse {
        session_id: session_id_str,
        status: "ideating".to_string(),
        agent_spawned,
        agent_spawn_blocked_reason,
        pending_initial_prompt,
        existing_active_sessions: existing_summaries,
        exists: None,
        duplicate_detected: None,
        similarity_score: None,
        next_action: if agent_spawned {
            "poll_status".to_string()
        } else if deferred_for_resume {
            "wait_for_resume".to_string()
        } else {
            "poll_status".to_string()
        },
        hint: Some(if agent_spawned {
            "Poll v1_get_ideation_status to track agent progress.".to_string()
        } else if deferred_for_resume {
            "The ideation prompt is saved, but execution is paused. Resume execution to launch the run.".to_string()
        } else {
            "Poll v1_get_ideation_status to track agent progress.".to_string()
        }),
        parent_conversation_id: parent_workspace_binding
            .as_ref()
            .map(|binding| binding.conversation_id.as_str().to_string()),
        workspace_branch: parent_workspace_binding
            .as_ref()
            .map(|binding| binding.workspace.branch_name.clone()),
    }))
}
