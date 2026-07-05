use crate::application::agent_conversation_start_service::ensure_plan_workspace_planning_session_link_with_analysis;
use crate::application::agent_conversation_workspace::{
    agent_conversation_branch_name, resolve_agent_conversation_workspace_path,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, Artifact, ArtifactBucketId,
    ArtifactContent, ArtifactId, ArtifactMetadata, ArtifactRelation, ArtifactType, ChatContextType,
    ChatConversation, IdeationAnalysisBaseRefKind, IdeationAnalysisState,
    IdeationAnalysisWorkspaceKind, IdeationSession, IdeationSessionFlow, IdeationSessionId,
    IdeationSessionStatus, Project,
};
use crate::error::AppError;
use crate::infrastructure::sqlite::{
    sqlite_artifact_repo::SqliteArtifactRepository as ArtifactRepo,
    sqlite_ideation_session_repo::SqliteIdeationSessionRepository as SessionRepo,
};

#[derive(Debug, Clone)]
pub struct AgentConversationPlanCopyRequest {
    pub conversation_id: String,
    pub source_session_id: String,
    pub source_artifact_id: String,
    pub source_version: u32,
}

#[derive(Debug, Clone)]
pub struct AgentConversationMarkdownImportRequest {
    pub conversation_id: String,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct AgentConversationPlanDraft {
    pub conversation_id: String,
    pub project_id: String,
    pub planning_session_id: String,
    pub plan_artifact_id: String,
    pub plan_artifact_version: u32,
    pub source_artifact_id: Option<String>,
    pub source_version: Option<u32>,
}

pub async fn copy_agent_conversation_plan(
    state: &AppState,
    input: AgentConversationPlanCopyRequest,
) -> Result<AgentConversationPlanDraft, String> {
    let source = resolve_source_plan_copy(state, &input).await?;
    let (conversation, project) = load_project_conversation(state, &input.conversation_id).await?;
    if source.session.project_id != project.id {
        return Err("Source session belongs to a different project".to_string());
    }

    let (workspace, planning_session_id) =
        ensure_agent_plan_workspace(state, &conversation, &project).await?;
    create_target_plan_draft(
        state,
        workspace,
        planning_session_id,
        DraftPlanContent {
            title: source.artifact.name.clone(),
            content: source.content,
            source_artifact: Some(source.artifact),
            source_version: Some(input.source_version),
        },
    )
    .await
}

pub async fn import_agent_conversation_plan_markdown(
    state: &AppState,
    input: AgentConversationMarkdownImportRequest,
) -> Result<AgentConversationPlanDraft, String> {
    let (conversation, project) = load_project_conversation(state, &input.conversation_id).await?;
    let title = input.title.trim();
    if title.is_empty() {
        return Err("Plan title is required".to_string());
    }
    if input.content.trim().is_empty() {
        return Err("Plan content is required".to_string());
    }

    let (workspace, planning_session_id) =
        ensure_agent_plan_workspace(state, &conversation, &project).await?;
    create_target_plan_draft(
        state,
        workspace,
        planning_session_id,
        DraftPlanContent {
            title: title.to_string(),
            content: input.content,
            source_artifact: None,
            source_version: None,
        },
    )
    .await
}

struct ResolvedSourcePlanCopy {
    session: IdeationSession,
    artifact: Artifact,
    content: String,
}

struct DraftPlanContent {
    title: String,
    content: String,
    source_artifact: Option<Artifact>,
    source_version: Option<u32>,
}

async fn load_project_conversation(
    state: &AppState,
    conversation_id: &str,
) -> Result<(ChatConversation, Project), String> {
    let conversation_id = crate::domain::entities::ChatConversationId::from_string(conversation_id);
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Conversation not found: {}", conversation_id))?;
    if conversation.context_type != ChatContextType::Project {
        return Err("Only project agent conversations can import plans".to_string());
    }

    let project_id =
        crate::domain::entities::ProjectId::from_string(conversation.context_id.clone());
    let project = state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {}", project_id))?;
    Ok((conversation, project))
}

async fn resolve_source_plan_copy(
    state: &AppState,
    input: &AgentConversationPlanCopyRequest,
) -> Result<ResolvedSourcePlanCopy, String> {
    if input.source_version == 0 {
        return Err("Source plan version is required".to_string());
    }

    let source_session_id = IdeationSessionId::from_string(input.source_session_id.clone());
    let source_session = state
        .ideation_session_repo
        .get_by_id(&source_session_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Source session not found: {}", source_session_id))?;

    if source_session.status == IdeationSessionStatus::Archived {
        return Err("Cannot copy from an archived source session".to_string());
    }
    if source_session.session_purpose == crate::domain::entities::SessionPurpose::Verification {
        return Err("Cannot copy from a verification child session".to_string());
    }

    let latest_source_id = source_session
        .plan_artifact_id
        .as_ref()
        .or(source_session.inherited_plan_artifact_id.as_ref())
        .ok_or_else(|| "Source session does not have a plan artifact".to_string())?;
    if latest_source_id.as_str() != input.source_artifact_id {
        return Err("Source artifact is stale for the selected source session".to_string());
    }

    let source_artifact_id = ArtifactId::from_string(input.source_artifact_id.clone());
    let source_artifact = state
        .artifact_repo
        .get_by_id_at_version(&source_artifact_id, input.source_version)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!(
                "Source plan version {} was not found for artifact {}",
                input.source_version, input.source_artifact_id
            )
        })?;
    if source_artifact.artifact_type != ArtifactType::Specification {
        return Err("Source artifact is not a specification/plan type".to_string());
    }
    let content = inline_artifact_content(&source_artifact)
        .ok_or_else(|| "Source plan artifact does not contain inline content".to_string())?;

    Ok(ResolvedSourcePlanCopy {
        session: source_session,
        artifact: source_artifact,
        content,
    })
}

async fn ensure_agent_plan_workspace(
    state: &AppState,
    conversation: &ChatConversation,
    project: &Project,
) -> Result<(AgentConversationWorkspace, IdeationSessionId), String> {
    let mut workspace = match state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .map_err(|error| error.to_string())?
    {
        Some(mut workspace) => {
            if workspace.project_id != project.id {
                return Err(
                    "Agent conversation workspace belongs to a different project".to_string(),
                );
            }
            if workspace.linked_plan_branch_id.is_some() {
                return Err(
                    "Cannot import a plan into an execution-owned agent workspace".to_string(),
                );
            }
            if workspace.mode != AgentConversationWorkspaceMode::Plan {
                workspace.mode = AgentConversationWorkspaceMode::Plan;
                workspace.updated_at = chrono::Utc::now();
            }
            workspace
        }
        None => new_plan_workspace(project, conversation)?,
    };

    let analysis = plan_workspace_analysis(&workspace);
    ensure_plan_workspace_planning_session_link_with_analysis(state, &mut workspace, analysis)
        .await?;
    let planning_session_id = workspace
        .linked_ideation_session_id
        .clone()
        .ok_or_else(|| "Failed to link a planning session".to_string())?;

    let workspace = state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .map_err(|error| error.to_string())?;
    state
        .chat_conversation_repo
        .update_agent_mode(&conversation.id, Some(AgentConversationWorkspaceMode::Plan))
        .await
        .map_err(|error| error.to_string())?;

    Ok((workspace, planning_session_id))
}

fn plan_workspace_analysis(workspace: &AgentConversationWorkspace) -> IdeationAnalysisState {
    IdeationAnalysisState {
        base_ref_kind: Some(workspace.base_ref_kind),
        base_ref: Some(workspace.base_ref.clone()),
        base_display_name: workspace.base_display_name.clone(),
        workspace_kind: IdeationAnalysisWorkspaceKind::IdeationWorktree,
        workspace_path: Some(workspace.worktree_path.clone()),
        base_commit: workspace.base_commit.clone(),
        base_locked_at: Some(chrono::Utc::now()),
    }
}

fn new_plan_workspace(
    project: &Project,
    conversation: &ChatConversation,
) -> Result<AgentConversationWorkspace, String> {
    let base_ref = project
        .base_branch
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "main".to_string());
    let worktree_path = resolve_agent_conversation_workspace_path(project, &conversation.id)
        .map_err(|error| error.to_string())?;
    Ok(AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        base_ref.clone(),
        Some(format!("Project default ({base_ref})")),
        None,
        agent_conversation_branch_name(project, &conversation.id),
        worktree_path.to_string_lossy().to_string(),
    ))
}

async fn create_target_plan_draft(
    state: &AppState,
    workspace: AgentConversationWorkspace,
    planning_session_id: IdeationSessionId,
    content: DraftPlanContent,
) -> Result<AgentConversationPlanDraft, String> {
    let source_artifact_id = content
        .source_artifact
        .as_ref()
        .map(|artifact| artifact.id.as_str().to_string());
    let source_version = content.source_version;
    let source_artifact_for_tx = content.source_artifact.clone();
    let planning_session_id_for_tx = planning_session_id.as_str().to_string();
    let workspace_project_id_for_tx = workspace.project_id.as_str().to_string();
    let workspace_conversation_id_for_tx = workspace.conversation_id.as_str();
    let title = content.title;
    let body = content.content;

    let created = state
        .db
        .run_transaction(move |conn| {
            let session = SessionRepo::get_by_id_sync(conn, &planning_session_id_for_tx)?
                .ok_or_else(|| AppError::NotFound("Planning session not found".to_string()))?;
            if session.session_flow != IdeationSessionFlow::Planning {
                return Err(AppError::Validation(
                    "Linked session is not a planning session".to_string(),
                ));
            }
            if session.status == IdeationSessionStatus::Archived {
                return Err(AppError::Validation(
                    "Linked planning session is archived".to_string(),
                ));
            }
            if session.project_id.as_str() != workspace_project_id_for_tx
                || session.source_context_type.as_deref() != Some("agent_conversation")
                || session.source_context_id.as_deref()
                    != Some(workspace_conversation_id_for_tx.as_str())
            {
                return Err(AppError::Validation(
                    "Linked planning session does not belong to this agent conversation"
                        .to_string(),
                ));
            }

            let existing_target = match session.plan_artifact_id.as_ref() {
                Some(existing_id) => Some(
                    ArtifactRepo::get_by_id_sync(conn, existing_id.as_str())?.ok_or_else(|| {
                        AppError::NotFound(format!(
                            "Target plan artifact not found: {}",
                            existing_id
                        ))
                    })?,
                ),
                None => None,
            };
            let version = existing_target
                .as_ref()
                .map(|artifact| artifact.metadata.version + 1)
                .unwrap_or(1);
            let mut new_artifact = Artifact {
                id: ArtifactId::new(),
                artifact_type: ArtifactType::Specification,
                name: title,
                content: ArtifactContent::inline(body),
                metadata: ArtifactMetadata::new("agent_plan_import").with_version(version),
                derived_from: source_artifact_for_tx
                    .as_ref()
                    .map(|artifact| vec![artifact.id.clone()])
                    .unwrap_or_default(),
                bucket_id: Some(ArtifactBucketId::from_string("prd-library")),
                archived_at: None,
            };

            let created = if let Some(existing) = existing_target {
                ArtifactRepo::create_with_previous_version_sync(
                    conn,
                    new_artifact,
                    existing.id.as_str(),
                )?
            } else {
                ArtifactRepo::create_sync(conn, new_artifact)?
            };
            new_artifact = created;

            if let Some(source_artifact) = source_artifact_for_tx {
                ArtifactRepo::add_relation_sync(
                    conn,
                    ArtifactRelation::derived_from(
                        new_artifact.id.clone(),
                        source_artifact.id.clone(),
                    ),
                )?;
            }

            SessionRepo::update_plan_artifact_id_sync(
                conn,
                &planning_session_id_for_tx,
                Some(new_artifact.id.as_str()),
            )?;
            SessionRepo::update_plan_version_last_read_sync(
                conn,
                &planning_session_id_for_tx,
                new_artifact.metadata.version as i32,
            )?;
            SessionRepo::reset_verification_sync(conn, &planning_session_id_for_tx)?;

            Ok(new_artifact)
        })
        .await
        .map_err(|error| error.to_string())?;

    Ok(AgentConversationPlanDraft {
        conversation_id: workspace.conversation_id.as_str(),
        project_id: workspace.project_id.as_str().to_string(),
        planning_session_id: planning_session_id.as_str().to_string(),
        plan_artifact_id: created.id.as_str().to_string(),
        plan_artifact_version: created.metadata.version,
        source_artifact_id,
        source_version,
    })
}

fn inline_artifact_content(artifact: &Artifact) -> Option<String> {
    match &artifact.content {
        ArtifactContent::Inline { text } => Some(text.clone()),
        ArtifactContent::File { .. } => None,
    }
}
