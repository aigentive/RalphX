use serde::{Deserialize, Serialize};
use tauri::State;

pub(crate) use crate::application::agent_task_pipeline_service::validate_complete_task_pipeline_proposal_selection;
use crate::application::{
    agent_task_pipeline_service::{
        activate_agent_task_pipeline as activate_agent_task_pipeline_service,
        validate_supervised_task_pipeline,
    },
    AppState,
};
use crate::commands::agent_composer_commands::plan_references::session_can_reference_plan;
use crate::commands::ideation_commands::{
    apply_supervised_proposals_to_kanban_for_state, ApplyProposalsInput,
    ApplyProposalsResultResponse,
};
use crate::commands::unified_chat_commands::{
    agent_workspace_response_for_state, ensure_plan_workspace_planning_session_link_for_send,
    switch_agent_conversation_mode_for_state_allowing_running, AgentConversationResponse,
    AgentConversationWorkspaceResponse, ModeSwitchInitiator, SwitchAgentConversationModeInput,
};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, Artifact, ArtifactBucketId, ArtifactContent, ArtifactId,
    ArtifactMetadata, ArtifactRelation, ArtifactType, ChatContextType, ChatConversationId,
    IdeationSession, IdeationSessionFlow, IdeationSessionId, IdeationSessionStatus,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::{
    SqliteArtifactRepository as ArtifactRepo, SqliteIdeationSessionRepository as SessionRepo,
};

const PLAN_BUCKET_ID: &str = "prd-library";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyAgentConversationPlanInput {
    pub conversation_id: String,
    pub source_session_id: String,
    pub source_artifact_id: String,
    pub source_version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAgentConversationPlanInput {
    pub conversation_id: String,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateAgentTaskPipelineInput {
    pub conversation_id: String,
    pub session_id: String,
    pub runtime_override: Option<crate::domain::agents::ManualRoleRuntimeOverride>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartAgentTaskPipelineInput {
    pub conversation_id: String,
    pub session_id: String,
    pub proposal_ids: Vec<String>,
    pub base_branch_override: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AgentConversationPlanSeedResponse {
    pub conversation: AgentConversationResponse,
    pub workspace: AgentConversationWorkspaceResponse,
    pub session_id: String,
    pub artifact: AgentPlanArtifactResponse,
}

#[derive(Debug, Serialize)]
pub struct AgentPlanArtifactResponse {
    pub id: String,
    pub artifact_type: String,
    pub name: String,
    pub content_type: String,
    pub content: String,
    pub version: u32,
    pub created_at: String,
    pub created_by: String,
    pub bucket_id: Option<String>,
    pub task_id: Option<String>,
    pub process_id: Option<String>,
    pub derived_from: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_approval_status: Option<String>,
}

impl From<Artifact> for AgentPlanArtifactResponse {
    fn from(artifact: Artifact) -> Self {
        let (content_type, content) = match &artifact.content {
            ArtifactContent::Inline { text } => ("inline".to_string(), text.clone()),
            ArtifactContent::File { path } => ("file".to_string(), path.clone()),
        };

        Self {
            id: artifact.id.as_str().to_string(),
            artifact_type: artifact.artifact_type.to_string(),
            name: artifact.name,
            content_type,
            content,
            version: artifact.metadata.version,
            created_at: artifact.metadata.created_at.to_rfc3339(),
            created_by: artifact.metadata.created_by,
            bucket_id: artifact.bucket_id.map(|id| id.as_str().to_string()),
            task_id: artifact.metadata.task_id.map(|id| id.as_str().to_string()),
            process_id: artifact
                .metadata
                .process_id
                .map(|id| id.as_str().to_string()),
            derived_from: artifact
                .derived_from
                .iter()
                .map(|id| id.as_str().to_string())
                .collect(),
            session_id: None,
            plan_approval_status: None,
        }
    }
}

#[derive(Debug, Clone)]
struct PlanSeed {
    title: String,
    content: String,
    source_artifact_id: Option<ArtifactId>,
}

#[tauri::command]
pub async fn copy_agent_conversation_plan(
    input: CopyAgentConversationPlanInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationPlanSeedResponse, String> {
    copy_agent_conversation_plan_for_state(input, state.inner()).await
}

#[tauri::command]
pub async fn import_agent_conversation_plan(
    input: ImportAgentConversationPlanInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationPlanSeedResponse, String> {
    import_agent_conversation_plan_for_state(input, state.inner()).await
}

#[tauri::command]
pub async fn activate_agent_task_pipeline(
    input: ActivateAgentTaskPipelineInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationWorkspaceResponse, String> {
    activate_agent_task_pipeline_for_state(input, state.inner()).await
}

#[tauri::command]
pub async fn start_agent_task_pipeline(
    input: StartAgentTaskPipelineInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ApplyProposalsResultResponse, String> {
    validate_supervised_task_pipeline(
        state.inner(),
        &input.conversation_id,
        &input.session_id,
        AgentConversationWorkspaceMode::Tasks,
    )
    .await?;
    validate_complete_task_pipeline_proposal_selection(
        state.inner(),
        &input.session_id,
        &input.proposal_ids,
    )
    .await?;
    apply_supervised_proposals_to_kanban_for_state(
        ApplyProposalsInput {
            session_id: input.session_id,
            proposal_ids: input.proposal_ids,
            target_column: "auto".to_string(),
            base_branch_override: input.base_branch_override,
        },
        input.conversation_id,
        &state,
        &app,
    )
    .await
}

#[doc(hidden)]
pub(crate) async fn activate_agent_task_pipeline_for_state(
    input: ActivateAgentTaskPipelineInput,
    state: &AppState,
) -> Result<AgentConversationWorkspaceResponse, String> {
    if let Some(runtime_override) = input.runtime_override.as_ref() {
        let conversation_id = ChatConversationId::from_string(input.conversation_id.clone());
        let conversation = state
            .chat_conversation_repo
            .get_by_id(&conversation_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Conversation not found: {conversation_id}"))?;
        let project = state
            .project_repo
            .get_by_id(&crate::domain::entities::ProjectId::from_string(
                conversation.context_id,
            ))
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Project not found for plan continuation".to_string())?;
        crate::application::agent_lane_resolution::resolve_manual_role_spawn_settings(
            crate::infrastructure::agents::claude::agent_names::AGENT_ORCHESTRATOR_IDEATION,
            Some(project.id.as_str()),
            Some(std::path::Path::new(&project.working_directory)),
            crate::domain::agents::RoutingRole::IdeationPrimary,
            Some(runtime_override),
            None,
            None,
            &state.manual_role_default_service(),
        )
        .await
        .map_err(|error| error.to_string())?;
    }
    let workspace = activate_agent_task_pipeline_service(
        state,
        &input.conversation_id,
        &input.session_id,
        input.runtime_override.as_ref(),
    )
    .await?;
    agent_workspace_response_for_state(state, workspace).await
}

#[doc(hidden)]
pub(crate) async fn copy_agent_conversation_plan_for_state(
    input: CopyAgentConversationPlanInput,
    state: &AppState,
) -> Result<AgentConversationPlanSeedResponse, String> {
    if input.source_version == 0 {
        return Err("Source plan version must be greater than zero".to_string());
    }

    let conversation_id = ChatConversationId::from_string(input.conversation_id.clone());
    let target_project_id = project_id_for_target_conversation(state, &conversation_id).await?;
    let source_artifact = resolve_source_plan_artifact(
        state,
        &target_project_id,
        &input.source_session_id,
        &input.source_artifact_id,
        input.source_version,
    )
    .await?;
    let (title, content, source_artifact_id) = inline_plan_seed_from_source(source_artifact)?;

    seed_agent_conversation_plan(
        conversation_id,
        PlanSeed {
            title,
            content,
            source_artifact_id: Some(source_artifact_id),
        },
        state,
    )
    .await
}

#[doc(hidden)]
pub(crate) async fn import_agent_conversation_plan_for_state(
    input: ImportAgentConversationPlanInput,
    state: &AppState,
) -> Result<AgentConversationPlanSeedResponse, String> {
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err("Plan title is required".to_string());
    }
    let content = input.content.trim().to_string();
    if content.is_empty() {
        return Err("Plan content is required".to_string());
    }

    seed_agent_conversation_plan(
        ChatConversationId::from_string(input.conversation_id),
        PlanSeed {
            title,
            content,
            source_artifact_id: None,
        },
        state,
    )
    .await
}

async fn project_id_for_target_conversation(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<crate::domain::entities::ProjectId, String> {
    let conversation = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Conversation not found: {}", conversation_id))?;
    if conversation.context_type != ChatContextType::Project {
        return Err("Only project agent conversations can seed plans".to_string());
    }
    Ok(crate::domain::entities::ProjectId::from_string(
        conversation.context_id,
    ))
}

async fn resolve_source_plan_artifact(
    state: &AppState,
    target_project_id: &crate::domain::entities::ProjectId,
    source_session_id: &str,
    source_artifact_id: &str,
    source_version: u32,
) -> Result<Artifact, String> {
    let source_session_id = IdeationSessionId::from_string(source_session_id.to_string());
    let source_session = state
        .ideation_session_repo
        .get_by_id(&source_session_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Source session not found: {}", source_session_id))?;

    if source_session.project_id != *target_project_id {
        return Err("Source session belongs to a different project".to_string());
    }
    if !session_can_reference_plan(&source_session) {
        return Err("Source session does not have an importable plan".to_string());
    }

    let Some(seed_plan_id) = source_session
        .plan_artifact_id
        .as_ref()
        .or(source_session.inherited_plan_artifact_id.as_ref())
    else {
        return Err("Source session does not have an importable plan".to_string());
    };

    let requested_artifact_id = ArtifactId::from_string(source_artifact_id.to_string());
    let latest_requested_id = state
        .artifact_repo
        .resolve_latest_artifact_id(&requested_artifact_id)
        .await
        .map_err(|error| error.to_string())?;
    let latest_seed_id = state
        .artifact_repo
        .resolve_latest_artifact_id(seed_plan_id)
        .await
        .map_err(|error| error.to_string())?;
    if latest_requested_id != latest_seed_id {
        return Err("Source artifact does not belong to the selected source session".to_string());
    }

    let source_artifact = state
        .artifact_repo
        .get_by_id_at_version(&latest_requested_id, source_version)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Source plan version not found: {}", source_version))?;
    if source_artifact.artifact_type != ArtifactType::Specification {
        return Err("Source artifact is not a plan".to_string());
    }
    if source_artifact.archived_at.is_some() {
        return Err("Source plan version is archived".to_string());
    }

    Ok(source_artifact)
}

fn inline_plan_seed_from_source(
    source_artifact: Artifact,
) -> Result<(String, String, ArtifactId), String> {
    let content = match source_artifact.content {
        ArtifactContent::Inline { text } => text,
        ArtifactContent::File { .. } => {
            return Err(
                "File-backed source plans cannot be copied from the agent Plan tab".to_string(),
            )
        }
    };
    Ok((source_artifact.name, content, source_artifact.id))
}

async fn seed_agent_conversation_plan(
    conversation_id: ChatConversationId,
    seed: PlanSeed,
    state: &AppState,
) -> Result<AgentConversationPlanSeedResponse, String> {
    let switch_response = switch_agent_conversation_mode_for_state_allowing_running(
        SwitchAgentConversationModeInput {
            conversation_id: conversation_id.as_str().to_string(),
            mode: AgentConversationWorkspaceMode::Plan.to_string(),
            base_ref_kind: None,
            base_branch_mode: None,
            base_ref: None,
            base_display_name: None,
            base_source_pull_request: None,
            runtime_override: None,
        },
        state,
        ModeSwitchInitiator::User,
    )
    .await?;

    ensure_plan_workspace_planning_session_link_for_send(state, &conversation_id).await?;

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Agent workspace not found: {}", conversation_id))?;
    if workspace.mode != AgentConversationWorkspaceMode::Plan {
        return Err("Agent workspace is not in plan mode".to_string());
    }
    let target_session_id = workspace
        .linked_ideation_session_id
        .as_ref()
        .ok_or_else(|| "Plan workspace is missing a linked planning session".to_string())?
        .as_str()
        .to_string();

    let created = create_or_version_target_plan(state, target_session_id.clone(), seed).await?;
    let workspace_response = agent_workspace_response_for_state(state, workspace).await?;
    let mut artifact_response = AgentPlanArtifactResponse::from(created);
    artifact_response.session_id = Some(target_session_id.clone());
    artifact_response.plan_approval_status = Some("draft".to_string());

    Ok(AgentConversationPlanSeedResponse {
        conversation: switch_response.conversation,
        workspace: workspace_response,
        session_id: target_session_id,
        artifact: artifact_response,
    })
}

async fn create_or_version_target_plan(
    state: &AppState,
    target_session_id: String,
    seed: PlanSeed,
) -> Result<Artifact, String> {
    state
        .db
        .run_transaction(move |conn| {
            create_or_version_target_plan_sync(conn, &target_session_id, seed)
        })
        .await
        .map_err(|error| error.to_string())
}

fn create_or_version_target_plan_sync(
    conn: &rusqlite::Connection,
    target_session_id: &str,
    seed: PlanSeed,
) -> AppResult<Artifact> {
    let session = SessionRepo::get_by_id_sync(conn, target_session_id)?.ok_or_else(|| {
        AppError::NotFound(format!("Planning session not found: {target_session_id}"))
    })?;
    assert_planning_session_mutable(&session)?;
    if session.session_flow != IdeationSessionFlow::Planning {
        return Err(AppError::Validation(
            "Linked session is not a planning session".to_string(),
        ));
    }

    let previous_artifact = match session.plan_artifact_id.as_ref() {
        Some(plan_id) => {
            let latest_id = ArtifactRepo::resolve_latest_sync(conn, plan_id.as_str())?;
            ArtifactRepo::get_by_id_sync(conn, &latest_id)?
        }
        None => None,
    };

    let source_artifact_id = seed.source_artifact_id;
    let derived_from = source_artifact_id
        .as_ref()
        .map(|id| vec![id.clone()])
        .unwrap_or_default();
    let version = previous_artifact
        .as_ref()
        .map(|artifact| artifact.metadata.version + 1)
        .unwrap_or(1);
    let new_artifact = Artifact {
        id: ArtifactId::new(),
        artifact_type: ArtifactType::Specification,
        name: seed.title,
        content: ArtifactContent::inline(seed.content),
        metadata: ArtifactMetadata::new("user").with_version(version),
        derived_from,
        bucket_id: Some(ArtifactBucketId::from_string(PLAN_BUCKET_ID)),
        archived_at: None,
    };

    let created = if let Some(previous) = previous_artifact {
        ArtifactRepo::create_with_previous_version_sync(conn, new_artifact, previous.id.as_str())?
    } else {
        ArtifactRepo::create_sync(conn, new_artifact)?
    };

    SessionRepo::update_plan_artifact_id_sync(conn, target_session_id, Some(created.id.as_str()))?;
    SessionRepo::update_plan_version_last_read_sync(
        conn,
        target_session_id,
        created.metadata.version as i32,
    )?;

    if let Some(source_id) = source_artifact_id {
        let relation = ArtifactRelation::derived_from(created.id.clone(), source_id);
        conn.execute(
            "INSERT INTO artifact_relations (id, from_artifact_id, to_artifact_id, relation_type)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                relation.id.as_str(),
                relation.from_artifact_id.as_str(),
                relation.to_artifact_id.as_str(),
                relation.relation_type.as_str(),
            ],
        )?;
    }

    Ok(created)
}

fn assert_planning_session_mutable(session: &IdeationSession) -> AppResult<()> {
    match session.status {
        IdeationSessionStatus::Archived | IdeationSessionStatus::Accepted => {
            Err(AppError::Validation(format!(
                "Cannot modify {} session. Reopen it first.",
                session.status
            )))
        }
        IdeationSessionStatus::Active => Ok(()),
    }
}
