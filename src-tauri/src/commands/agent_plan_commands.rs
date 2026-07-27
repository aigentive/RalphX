use serde::{Deserialize, Serialize};
use tauri::State;

pub(crate) use crate::application::agent_task_pipeline_service::validate_complete_task_pipeline_proposal_selection;
use crate::application::{
    agent_task_pipeline_service::{
        activate_agent_task_pipeline as activate_agent_task_pipeline_service,
        validate_direct_implementation_authority_sync, validate_supervised_task_pipeline,
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
use crate::domain::services::ComposerArtifactReference;
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
pub struct ActivateAgentPlanDirectImplementationInput {
    pub conversation_id: String,
    pub session_id: String,
    #[serde(default)]
    pub retry: bool,
}

#[derive(Debug, Serialize)]
pub struct ActivateAgentPlanDirectImplementationResponse {
    pub workspace: AgentConversationWorkspaceResponse,
    pub artifact_references: Vec<ComposerArtifactReference>,
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
    pub blueprint_artifact: Option<AgentPlanArtifactResponse>,
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
struct PlanDocumentSeed {
    title: String,
    content: String,
    source_artifact_id: Option<ArtifactId>,
}

#[derive(Debug, Clone)]
struct PlanSeed {
    overview: PlanDocumentSeed,
    blueprint: Option<PlanDocumentSeed>,
}

struct ResolvedSourcePlan {
    overview: Artifact,
    blueprint: Option<Artifact>,
}

struct CreatedPlanSeed {
    overview: Artifact,
    blueprint: Option<Artifact>,
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
pub async fn activate_agent_plan_direct_implementation(
    input: ActivateAgentPlanDirectImplementationInput,
    state: State<'_, AppState>,
) -> Result<ActivateAgentPlanDirectImplementationResponse, String> {
    activate_agent_plan_direct_implementation_for_state(input, state.inner()).await
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
pub(crate) async fn activate_agent_plan_direct_implementation_for_state(
    input: ActivateAgentPlanDirectImplementationInput,
    state: &AppState,
) -> Result<ActivateAgentPlanDirectImplementationResponse, String> {
    let conversation_id = ChatConversationId::from_string(input.conversation_id.clone());
    let tx_conversation_id = input.conversation_id;
    let tx_session_id = input.session_id;
    let response_session_id = tx_session_id.clone();
    let retry = input.retry;
    let approved_bundle = state
        .db
        .run_transaction(move |conn| {
            let approved_bundle = validate_direct_implementation_authority_sync(
                conn,
                &tx_conversation_id,
                &tx_session_id,
                retry,
            )?;
            if retry {
                let conversation_is_edit = conn.query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM chat_conversations
                        WHERE id = ?1 AND agent_mode = 'edit'
                     )",
                    [&tx_conversation_id],
                    |row| row.get::<_, bool>(0),
                )?;
                if !conversation_is_edit {
                    return Err(AppError::Conflict(
                        "Direct implementation retry is not in Edit mode".to_string(),
                    ));
                }
                return Ok(approved_bundle);
            }
            let now = chrono::Utc::now().to_rfc3339();
            let workspace_updated = conn.execute(
                "UPDATE agent_conversation_workspaces
                 SET mode = 'edit', updated_at = ?2
                 WHERE conversation_id = ?1 AND mode = 'plan'
                   AND linked_ideation_session_id = ?3",
                rusqlite::params![tx_conversation_id, now, tx_session_id],
            )?;
            let conversation_updated = conn.execute(
                "UPDATE chat_conversations
                 SET agent_mode = 'edit', updated_at = ?2
                 WHERE id = ?1 AND agent_mode = 'plan'",
                rusqlite::params![tx_conversation_id, now],
            )?;
            if workspace_updated != 1 || conversation_updated != 1 {
                return Err(AppError::Conflict(
                    "Plan changed before direct implementation activation".to_string(),
                ));
            }
            Ok(approved_bundle)
        })
        .await
        .map_err(|error| error.to_string())?;

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Activated Edit workspace was not found".to_string())?;
    let workspace = agent_workspace_response_for_state(state, workspace).await?;
    let mut artifact_references = vec![approved_plan_composer_reference(
        approved_bundle.overview,
        &response_session_id,
        "Plan Overview",
    )];
    if let Some(blueprint) = approved_bundle.blueprint {
        artifact_references.push(approved_plan_composer_reference(
            blueprint,
            &response_session_id,
            "Implementation Blueprint",
        ));
    }
    Ok(ActivateAgentPlanDirectImplementationResponse {
        workspace,
        artifact_references,
    })
}

fn approved_plan_composer_reference(
    artifact: Artifact,
    session_id: &str,
    fallback_title: &str,
) -> ComposerArtifactReference {
    ComposerArtifactReference {
        artifact_id: artifact.id.as_str().to_string(),
        kind: "plan".to_string(),
        title: Some(if artifact.name.trim().is_empty() {
            fallback_title.to_string()
        } else {
            artifact.name
        }),
        session_id: Some(session_id.to_string()),
        version: Some(artifact.metadata.version),
        status: Some("approved".to_string()),
    }
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
    let source = resolve_source_plan_artifacts(
        state,
        &target_project_id,
        &input.source_session_id,
        &input.source_artifact_id,
        input.source_version,
    )
    .await?;
    let overview = inline_plan_seed_from_source(source.overview)?;
    let blueprint = source
        .blueprint
        .map(inline_plan_seed_from_source)
        .transpose()?;

    seed_agent_conversation_plan(
        conversation_id,
        PlanSeed {
            overview,
            blueprint,
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
            overview: PlanDocumentSeed {
                title,
                content,
                source_artifact_id: None,
            },
            blueprint: None,
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

async fn resolve_source_plan_artifacts(
    state: &AppState,
    target_project_id: &crate::domain::entities::ProjectId,
    source_session_id: &str,
    source_artifact_id: &str,
    source_version: u32,
) -> Result<ResolvedSourcePlan, String> {
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

    let blueprint = if source_session.plan_contract_version >= 2 {
        let bundle = source_session
            .plan_artifact_bundle()
            .ok_or_else(|| "Source session has an incomplete v2 plan bundle".to_string())?;
        if source_artifact.id != bundle.overview_id {
            return Err(
                "Historical v2 plan copies require selecting the current Overview and Blueprint pair"
                    .to_string(),
            );
        }
        let blueprint_id = bundle
            .blueprint_id
            .ok_or_else(|| "Source session has an incomplete v2 plan bundle".to_string())?;
        let blueprint = state
            .artifact_repo
            .get_by_id(&blueprint_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("Source plan blueprint not found: {}", blueprint_id.as_str()))?;
        if blueprint.artifact_type != ArtifactType::Specification {
            return Err("Source blueprint is not a plan specification".to_string());
        }
        if blueprint.archived_at.is_some() {
            return Err("Source blueprint is archived".to_string());
        }
        Some(blueprint)
    } else {
        None
    };

    Ok(ResolvedSourcePlan {
        overview: source_artifact,
        blueprint,
    })
}

fn inline_plan_seed_from_source(source_artifact: Artifact) -> Result<PlanDocumentSeed, String> {
    let content = match source_artifact.content {
        ArtifactContent::Inline { text } => text,
        ArtifactContent::File { .. } => {
            return Err(
                "File-backed source plans cannot be copied from the agent Plan tab".to_string(),
            )
        }
    };
    Ok(PlanDocumentSeed {
        title: source_artifact.name,
        content,
        source_artifact_id: Some(source_artifact.id),
    })
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
    let mut artifact_response = AgentPlanArtifactResponse::from(created.overview);
    artifact_response.session_id = Some(target_session_id.clone());
    artifact_response.plan_approval_status = Some("draft".to_string());
    let blueprint_artifact = created.blueprint.map(|artifact| {
        let mut response = AgentPlanArtifactResponse::from(artifact);
        response.session_id = Some(target_session_id.clone());
        response.plan_approval_status = Some("draft".to_string());
        response
    });

    Ok(AgentConversationPlanSeedResponse {
        conversation: switch_response.conversation,
        workspace: workspace_response,
        session_id: target_session_id,
        artifact: artifact_response,
        blueprint_artifact,
    })
}

async fn create_or_version_target_plan(
    state: &AppState,
    target_session_id: String,
    seed: PlanSeed,
) -> Result<CreatedPlanSeed, String> {
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
) -> AppResult<CreatedPlanSeed> {
    let session = SessionRepo::get_by_id_sync(conn, target_session_id)?.ok_or_else(|| {
        AppError::NotFound(format!("Planning session not found: {target_session_id}"))
    })?;
    assert_planning_session_mutable(&session)?;
    if session.session_flow != IdeationSessionFlow::Planning {
        return Err(AppError::Validation(
            "Linked session is not a planning session".to_string(),
        ));
    }

    let previous_overview = match session.plan_artifact_id.as_ref() {
        Some(plan_id) => {
            let latest_id = ArtifactRepo::resolve_latest_sync(conn, plan_id.as_str())?;
            ArtifactRepo::get_by_id_sync(conn, &latest_id)?
        }
        None => None,
    };
    let previous_blueprint = match session.plan_blueprint_artifact_id.as_ref() {
        Some(blueprint_id) => {
            let latest_id = ArtifactRepo::resolve_latest_sync(conn, blueprint_id.as_str())?;
            ArtifactRepo::get_by_id_sync(conn, &latest_id)?
        }
        None => None,
    };

    let overview = create_plan_seed_document_sync(conn, seed.overview, previous_overview.as_ref())?;
    let blueprint = seed
        .blueprint
        .map(|blueprint_seed| {
            create_plan_seed_document_sync(conn, blueprint_seed, previous_blueprint.as_ref())
        })
        .transpose()?;

    if let (Some(previous_overview), Some(previous_blueprint)) =
        (previous_overview.as_ref(), previous_blueprint.as_ref())
    {
        conn.execute(
            "DELETE FROM artifact_relations
             WHERE relation_type = 'related_to'
               AND ((from_artifact_id = ?1 AND to_artifact_id = ?2)
                 OR (from_artifact_id = ?2 AND to_artifact_id = ?1))",
            rusqlite::params![
                previous_overview.id.as_str(),
                previous_blueprint.id.as_str(),
            ],
        )?;
    }

    if let Some(blueprint) = blueprint.as_ref() {
        SessionRepo::update_plan_bundle_sync(
            conn,
            target_session_id,
            overview.id.as_str(),
            blueprint.id.as_str(),
            overview.metadata.version as i32,
            blueprint.metadata.version as i32,
        )?;
        ArtifactRepo::add_relation_sync(
            conn,
            ArtifactRelation::related_to(overview.id.clone(), blueprint.id.clone()),
        )?;
    } else {
        SessionRepo::update_plan_artifact_id_sync(
            conn,
            target_session_id,
            Some(overview.id.as_str()),
        )?;
        let changed = conn.execute(
            "UPDATE ideation_sessions
             SET plan_blueprint_artifact_id = NULL,
                 plan_contract_version = 2,
                 plan_version_last_read = ?2,
                 blueprint_version_last_read = NULL,
                 verified_plan_artifact_id = NULL,
                 verified_plan_blueprint_artifact_id = NULL,
                 updated_at = ?3
             WHERE id = ?1",
            rusqlite::params![
                target_session_id,
                i64::from(overview.metadata.version),
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound(format!(
                "Planning session not found: {target_session_id}"
            )));
        }
    }

    Ok(CreatedPlanSeed {
        overview,
        blueprint,
    })
}

fn create_plan_seed_document_sync(
    conn: &rusqlite::Connection,
    seed: PlanDocumentSeed,
    previous_artifact: Option<&Artifact>,
) -> AppResult<Artifact> {
    let version = previous_artifact
        .map(|artifact| artifact.metadata.version + 1)
        .unwrap_or(1);
    let derived_from = seed
        .source_artifact_id
        .as_ref()
        .map(|id| vec![id.clone()])
        .unwrap_or_default();
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

    if let Some(source_id) = seed.source_artifact_id {
        ArtifactRepo::add_relation_sync(
            conn,
            ArtifactRelation::derived_from(created.id.clone(), source_id),
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
