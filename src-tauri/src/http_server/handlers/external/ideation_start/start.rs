use super::*;
use crate::application::ideation_service::build_default_ideation_session_title;
use crate::application::ideation_workspace::{
    prepare_ideation_analysis_state, prepare_ideation_analysis_state_from_agent_workspace,
    IdeationAnalysisBaseSelection,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Artifact, ArtifactId,
    ArtifactMetadata, ArtifactRelation, ArtifactType, ChatConversationId, ChatMessage,
    IdeationAnalysisState, IdeationSessionStatus, MessageRole, Project, SessionPurpose,
};
use crate::domain::services::message_queue::ComposerArtifactReference;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_imported: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_plan_artifact_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloned_plan_artifact_id: Option<String>,
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

fn external_session_summary(session: &IdeationSession) -> ExternalSessionSummary {
    ExternalSessionSummary {
        session_id: session.id.to_string(),
        title: session.title.clone(),
        status: session.status.to_string(),
        created_at: session.created_at.to_rfc3339(),
        external_activity_phase: session.external_activity_phase.clone(),
    }
}

async fn active_external_session_summaries(
    state: &HttpServerState,
    project_id: &ProjectId,
) -> Vec<ExternalSessionSummary> {
    state
        .app_state
        .ideation_session_repo
        .list_active_external_by_project(project_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|session| external_session_summary(&session))
        .collect()
}

async fn parent_workspace_reuse_response(
    state: &HttpServerState,
    project_id: &ProjectId,
    binding: &ParentWorkspaceBinding,
) -> Result<Option<StartIdeationResponse>, HttpError> {
    let Some(linked_session_id) = binding.workspace.linked_ideation_session_id.as_ref() else {
        return Ok(None);
    };

    let linked_session = state
        .app_state
        .ideation_session_repo
        .get_by_id(linked_session_id)
        .await
        .map_err(|error| {
            error!(
                "Failed to load linked ideation session {} for parent conversation {}: {}",
                linked_session_id.as_str(),
                binding.conversation_id.as_str(),
                error
            );
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to load linked ideation session".to_string()),
            }
        })?;

    let Some(session) = linked_session else {
        return Ok(None);
    };
    if session.project_id != *project_id || session.status != IdeationSessionStatus::Active {
        return Ok(None);
    }

    Ok(Some(StartIdeationResponse {
        session_id: session.id.to_string(),
        status: session.status.to_string(),
        agent_spawned: false,
        agent_spawn_blocked_reason: None,
        pending_initial_prompt: session.pending_initial_prompt.clone(),
        existing_active_sessions: active_external_session_summaries(state, project_id).await,
        exists: Some(true),
        duplicate_detected: None,
        similarity_score: None,
        next_action: "use_existing_session".to_string(),
        hint: Some(
            "Parent conversation already has an active ideation session. Reuse this session; send follow-up work with v1_send_ideation_message when the agent is ready."
                .to_string(),
        ),
        parent_conversation_id: Some(binding.conversation_id.as_str().to_string()),
        workspace_branch: Some(binding.workspace.branch_name.clone()),
        plan_imported: None,
        source_plan_artifact_id: None,
        cloned_plan_artifact_id: None,
    }))
}

struct ResolvedPlanImport {
    source_artifact: Artifact,
    source_session_id: String,
    source_session_status: IdeationSessionStatus,
}

fn extract_plan_references_from_metadata(
    metadata: &Option<String>,
) -> Vec<ComposerArtifactReference> {
    let Some(meta_str) = metadata else {
        return Vec::new();
    };
    let Ok(meta_value) = serde_json::from_str::<serde_json::Value>(meta_str) else {
        return Vec::new();
    };
    let Some(refs_value) = meta_value.get("composer_artifact_references") else {
        return Vec::new();
    };
    serde_json::from_value::<Vec<ComposerArtifactReference>>(refs_value.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.kind == "plan")
        .collect()
}

async fn resolve_plan_import(
    state: &HttpServerState,
    parent_conversation_id: &ChatConversationId,
    project_id: &ProjectId,
) -> Result<Option<ResolvedPlanImport>, HttpError> {
    let messages: Vec<ChatMessage> = state
        .app_state
        .chat_message_repo
        .get_by_conversation(parent_conversation_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to load messages for parent conversation {}: {}",
                parent_conversation_id.as_str(),
                e
            );
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to load parent conversation messages".to_string()),
            }
        })?;

    let plan_refs: Vec<ComposerArtifactReference> = messages
        .iter()
        .rev()
        .filter(|m| m.role == MessageRole::User)
        .flat_map(|m| extract_plan_references_from_metadata(&m.metadata))
        .collect();

    if plan_refs.is_empty() {
        return Ok(None);
    }

    if plan_refs.len() > 1 {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(
                "Multiple plan references found. Exactly one plan reference is required for plan import.".to_string(),
            ),
        });
    }

    let plan_ref = &plan_refs[0];
    let source_artifact_id = ArtifactId::from_string(&plan_ref.artifact_id);

    let source_artifact = state
        .app_state
        .artifact_repo
        .get_by_id(&source_artifact_id)
        .await
        .map_err(|e| {
            error!(
                "Failed to load source plan artifact {}: {}",
                plan_ref.artifact_id, e
            );
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to load source plan artifact".to_string()),
            }
        })?
        .ok_or_else(|| HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(format!(
                "Source plan artifact not found: {}",
                plan_ref.artifact_id
            )),
        })?;

    if !matches!(source_artifact.artifact_type, ArtifactType::Specification) {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("Source artifact is not a specification/plan type".to_string()),
        });
    }

    let source_session_id = plan_ref.session_id.as_deref().ok_or_else(|| HttpError {
        status: StatusCode::BAD_REQUEST,
        message: Some("Plan reference is missing session_id".to_string()),
    })?;

    let source_session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&IdeationSessionId::from_string(
            source_session_id.to_string(),
        ))
        .await
        .map_err(|e| {
            error!("Failed to load source session {}: {}", source_session_id, e);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to load source session".to_string()),
            }
        })?
        .ok_or_else(|| HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some(format!("Source session not found: {}", source_session_id)),
        })?;

    if source_session.project_id != *project_id {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("Source session belongs to a different project".to_string()),
        });
    }

    if source_session.status == IdeationSessionStatus::Archived {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("Cannot import from an archived session".to_string()),
        });
    }

    if source_session.session_purpose == SessionPurpose::Verification {
        return Err(HttpError {
            status: StatusCode::BAD_REQUEST,
            message: Some("Cannot import from a verification child session".to_string()),
        });
    }

    Ok(Some(ResolvedPlanImport {
        source_artifact,
        source_session_id: source_session_id.to_string(),
        source_session_status: source_session.status,
    }))
}

async fn clone_plan_artifact(
    state: &HttpServerState,
    source: &Artifact,
) -> Result<Artifact, HttpError> {
    let new_artifact = Artifact {
        id: ArtifactId::new(),
        artifact_type: source.artifact_type,
        name: source.name.clone(),
        content: source.content.clone(),
        metadata: ArtifactMetadata::new("plan_import").with_version(1),
        derived_from: vec![source.id.clone()],
        bucket_id: source.bucket_id.clone(),
        archived_at: None,
    };

    let created = state
        .app_state
        .artifact_repo
        .create(new_artifact)
        .await
        .map_err(|e| {
            error!("Failed to create cloned plan artifact: {}", e);
            HttpError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: Some("Failed to clone plan artifact".to_string()),
            }
        })?;

    let relation = ArtifactRelation::derived_from(created.id.clone(), source.id.clone());
    if let Err(e) = state.app_state.artifact_repo.add_relation(relation).await {
        tracing::warn!(
            "Failed to record derived_from relation for cloned artifact: {}",
            e
        );
    }

    Ok(created)
}

async fn clone_plan_approval_if_approved(
    state: &HttpServerState,
    source_session_id: &str,
    source_session_status: IdeationSessionStatus,
    new_session_id: &str,
    cloned_artifact: &Artifact,
) {
    let is_approved_source = source_session_status == IdeationSessionStatus::Accepted;

    let source_sid = source_session_id.to_string();
    let should_approve = if is_approved_source {
        true
    } else {
        state
            .app_state
            .db
            .run(move |conn| {
                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM plan_artifact_approvals
                         WHERE session_id = ?1 AND status = 'approved'",
                        [source_sid.as_str()],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                Ok(count > 0)
            })
            .await
            .unwrap_or(false)
    };

    if !should_approve {
        return;
    }

    let new_sid = new_session_id.to_string();
    let artifact = cloned_artifact.clone();
    let now = chrono::Utc::now().to_rfc3339();

    if let Err(e) = state
        .app_state
        .db
        .run(move |conn| {
            crate::application::plan_artifact_approval::upsert_plan_approval_sync(
                conn,
                &new_sid,
                &artifact,
                None,
                &now,
                crate::domain::repositories::PlanApprovalActor::PlanImport,
            )
        })
        .await
    {
        tracing::warn!("Failed to clone plan approval for imported session: {}", e);
    }
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

    let parent_conversation_id = parent_conversation_id_from_headers(&headers);
    let parent_workspace_binding =
        resolve_parent_workspace_binding(&state, &project, parent_conversation_id.clone()).await?;

    if let Some(binding) = parent_workspace_binding.as_ref() {
        if let Some(response) =
            parent_workspace_reuse_response(&state, &project_id, binding).await?
        {
            return Ok(Json(response));
        }
    }

    let plan_import = match parent_conversation_id.as_ref() {
        Some(conv_id) if is_tauri_mcp_request(&headers) => {
            resolve_plan_import(&state, conv_id, &project_id).await?
        }
        _ => None,
    };

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
                plan_imported: None,
                source_plan_artifact_id: None,
                cloned_plan_artifact_id: None,
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

    if has_candidate_text && !active_sessions.is_empty() && plan_import.is_none() {
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
                plan_imported: None,
                source_plan_artifact_id: None,
                cloned_plan_artifact_id: None,
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

    let cloned_artifact = if let Some(ref import) = plan_import {
        Some(clone_plan_artifact(&state, &import.source_artifact).await?)
    } else {
        None
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
    if let Some(ref import) = plan_import {
        if let Some(ref cloned) = cloned_artifact {
            session_builder.plan_artifact_id = Some(cloned.id.clone());
        }
        session_builder.session_flow = IdeationSessionFlow::Planning;
        session_builder.source_session_id = Some(import.source_session_id.clone());
        session_builder.source_project_id = Some(project_id.as_str().to_string());
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
                        plan_imported: None,
                        source_plan_artifact_id: None,
                        cloned_plan_artifact_id: None,
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

    if let (Some(ref import), Some(ref cloned)) = (&plan_import, &cloned_artifact) {
        clone_plan_approval_if_approved(
            &state,
            &import.source_session_id,
            import.source_session_status,
            &session_id_str,
            cloned,
        )
        .await;
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
    crate::http_server::emit_http_event(
        &state,
        "ideation:session_created",
        session_created_payload.clone(),
    );

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
        let chat_service = build_chat_service(&state);

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
        plan_imported: if plan_import.is_some() {
            Some(true)
        } else {
            None
        },
        source_plan_artifact_id: plan_import
            .as_ref()
            .map(|import| import.source_artifact.id.as_str().to_string()),
        cloned_plan_artifact_id: cloned_artifact.as_ref().map(|a| a.id.as_str().to_string()),
    }))
}
