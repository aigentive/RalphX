use std::collections::HashSet;

use chrono::Utc;

use crate::application::tasks_feature_policy::authorize_tasks_session_sync;
use crate::application::AppState;
use crate::domain::agents::ManualRoleRuntimeOverride;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationSession, IdeationSessionFlow, IdeationSessionId, IdeationSessionStatus,
};
use crate::domain::repositories::PlanApprovalActor;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::{
    SqliteArtifactRepository as ArtifactRepo, SqliteIdeationSessionRepository as SessionRepo,
};

pub(crate) async fn validate_complete_task_pipeline_proposal_selection(
    state: &AppState,
    session_id: &str,
    requested_ids: &[String],
) -> Result<(), String> {
    let session_id = IdeationSessionId::from_string(session_id.to_string());
    let expected_ids = state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|proposal| proposal.archived_at.is_none() && proposal.created_task_id.is_none())
        .map(|proposal| proposal.id.as_str().to_string())
        .collect::<HashSet<_>>();
    let requested_ids = requested_ids.iter().cloned().collect::<HashSet<_>>();
    if expected_ids.is_empty() {
        return Err("Task pipeline has no proposals to start".to_string());
    }
    if requested_ids != expected_ids {
        return Err(
            "Start Tasks must apply the complete current proposal set; refresh and review the graph"
                .to_string(),
        );
    }
    Ok(())
}

pub(crate) async fn validate_supervised_task_pipeline(
    state: &AppState,
    conversation_id: &str,
    session_id: &str,
    required_mode: AgentConversationWorkspaceMode,
) -> Result<AgentConversationWorkspace, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id.to_string());
    let session_id = IdeationSessionId::from_string(session_id.to_string());
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Agent workspace not found".to_string())?;
    if workspace.mode != required_mode {
        return Err(format!("Agent workspace must be in {required_mode} mode"));
    }
    let attached_session = match required_mode {
        AgentConversationWorkspaceMode::Plan => workspace.linked_ideation_session_id.as_ref(),
        AgentConversationWorkspaceMode::Tasks => workspace.task_pipeline_session_id.as_ref(),
        _ => None,
    };
    if attached_session != Some(&session_id) {
        return Err("Task pipeline session does not belong to this conversation".to_string());
    }
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Task pipeline session not found".to_string())?;
    if session.project_id != workspace.project_id {
        return Err("Task pipeline session belongs to a different project".to_string());
    }
    if session.session_flow != IdeationSessionFlow::Planning
        || session.status != IdeationSessionStatus::Active
    {
        return Err("Task pipeline must be an active planning session".to_string());
    }
    let plan_id = session
        .plan_artifact_id
        .as_ref()
        .ok_or_else(|| "Task pipeline session has no current plan".to_string())?;
    let latest_id = state
        .artifact_repo
        .resolve_latest_artifact_id(plan_id)
        .await
        .map_err(|error| error.to_string())?;
    let latest = state
        .artifact_repo
        .get_by_id(&latest_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Current plan artifact not found".to_string())?;
    let approval = state
        .plan_approval_repo
        .get_by_session(&session_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Current plan is not approved".to_string())?;
    if approval.approved_by != PlanApprovalActor::User.as_str() {
        return Err("Current plan requires explicit user approval".to_string());
    }
    if approval.artifact_id != latest.id || approval.artifact_version != latest.metadata.version {
        return Err("Current plan version is not approved".to_string());
    }
    match session.plan_blueprint_artifact_id.as_ref() {
        Some(blueprint_id) => {
            let latest_blueprint_id = state
                .artifact_repo
                .resolve_latest_artifact_id(blueprint_id)
                .await
                .map_err(|error| error.to_string())?;
            let latest_blueprint = state
                .artifact_repo
                .get_by_id(&latest_blueprint_id)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Current plan blueprint artifact not found".to_string())?;
            if approval.blueprint_artifact_id.as_ref() != Some(&latest_blueprint.id)
                || approval.blueprint_artifact_version != Some(latest_blueprint.metadata.version)
            {
                return Err("Current plan blueprint version is not approved".to_string());
            }
        }
        None if session.plan_contract_version >= 2 => {
            return Err("Task pipeline session has no implementation blueprint".to_string());
        }
        None => {}
    }
    Ok(workspace)
}

pub(crate) async fn activate_agent_task_pipeline(
    state: &AppState,
    conversation_id: &str,
    session_id: &str,
    runtime_override: Option<&ManualRoleRuntimeOverride>,
) -> Result<AgentConversationWorkspace, String> {
    let requested_session = IdeationSessionId::from_string(session_id.to_string());
    let conversation_id_typed = ChatConversationId::from_string(conversation_id.to_string());
    if let Some(existing) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id_typed)
        .await
        .map_err(|error| error.to_string())?
    {
        if existing.mode == AgentConversationWorkspaceMode::Tasks
            && existing.task_pipeline_session_id.as_ref() == Some(&requested_session)
        {
            validate_supervised_task_pipeline(
                state,
                conversation_id,
                session_id,
                AgentConversationWorkspaceMode::Tasks,
            )
            .await?;
            return Ok(existing);
        }
    }

    let workspace = validate_supervised_task_pipeline(
        state,
        conversation_id,
        session_id,
        AgentConversationWorkspaceMode::Plan,
    )
    .await?;
    if workspace.task_pipeline_session_id.is_some() {
        return Err("This conversation already has a different task pipeline".to_string());
    }

    let tx_conversation_id = conversation_id.to_string();
    let tx_session_id = session_id.to_string();
    let runtime_bindings = runtime_override.map(|runtime| {
        (
            runtime.coordination_mode.unwrap_or_default().to_string(),
            runtime
                .persona_id
                .as_ref()
                .map(|persona_id| persona_id.to_string()),
        )
    });
    state
        .db
        .run_transaction(move |conn| {
            validate_activation_authority_sync(conn, &tx_conversation_id, &tx_session_id)?;
            let updated_workspace = conn.execute(
                "UPDATE agent_conversation_workspaces
                 SET mode = 'tasks', task_pipeline_session_id = ?2, updated_at = ?3
                 WHERE conversation_id = ?1 AND mode = 'plan'
                   AND task_pipeline_session_id IS NULL",
                rusqlite::params![tx_conversation_id, tx_session_id, Utc::now().to_rfc3339()],
            )?;
            if updated_workspace != 1 {
                return Err(AppError::Conflict(
                    "Task pipeline activation was already consumed or changed".to_string(),
                ));
            }
            let now = Utc::now().to_rfc3339();
            let updated_conversation = match runtime_bindings {
                Some((coordination_mode, persona_id)) => conn.execute(
                    "UPDATE chat_conversations
                     SET agent_mode = 'tasks', coordination_mode = ?2,
                         persona_id = ?3, updated_at = ?4
                     WHERE id = ?1 AND agent_mode = 'plan'",
                    rusqlite::params![tx_conversation_id, coordination_mode, persona_id, now],
                )?,
                None => conn.execute(
                    "UPDATE chat_conversations
                     SET agent_mode = 'tasks', updated_at = ?2
                     WHERE id = ?1 AND agent_mode = 'plan'",
                    rusqlite::params![tx_conversation_id, now],
                )?,
            };
            if updated_conversation != 1 {
                return Err(AppError::Conflict(
                    "Task pipeline conversation projection changed before activation".to_string(),
                ));
            }
            Ok(())
        })
        .await
        .map_err(|error| error.to_string())?;

    state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id_typed)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Activated task workspace was not found".to_string())
}

fn validate_activation_authority_sync(
    conn: &rusqlite::Connection,
    conversation_id: &str,
    session_id: &str,
) -> AppResult<()> {
    authorize_tasks_session_sync(
        conn,
        None,
        crate::domain::ideation::TasksFeatureAction::Progress,
    )?;
    let session = SessionRepo::get_by_id_sync(conn, session_id)?
        .ok_or_else(|| AppError::NotFound("Task pipeline session not found".to_string()))?;
    if session.session_flow != IdeationSessionFlow::Planning
        || session.status != IdeationSessionStatus::Active
    {
        return Err(AppError::Validation(
            "Task pipeline must be an active planning session".to_string(),
        ));
    }
    let (project_id, mode, linked_session_id, task_pipeline_session_id): (
        String,
        String,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT project_id, mode, linked_ideation_session_id, task_pipeline_session_id
             FROM agent_conversation_workspaces WHERE conversation_id = ?1",
            [conversation_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => {
                AppError::NotFound("Agent workspace not found".to_string())
            }
            _ => AppError::Database(error.to_string()),
        })?;
    if project_id != session.project_id.as_str()
        || mode != AgentConversationWorkspaceMode::Plan.to_string()
        || linked_session_id.as_deref() != Some(session_id)
        || task_pipeline_session_id.is_some()
    {
        return Err(AppError::Conflict(
            "Task pipeline activation authority changed before it was consumed".to_string(),
        ));
    }
    validate_current_user_approved_plan_sync(conn, &session).map(|_| ())
}

pub(crate) fn validate_start_authority_sync(
    conn: &rusqlite::Connection,
    conversation_id: &str,
    session_id: &str,
    requested_proposal_ids: &[String],
) -> AppResult<()> {
    authorize_tasks_session_sync(
        conn,
        Some(session_id),
        crate::domain::ideation::TasksFeatureAction::Progress,
    )?;
    let session = SessionRepo::get_by_id_sync(conn, session_id)?
        .ok_or_else(|| AppError::NotFound("Task pipeline session not found".to_string()))?;
    if session.session_flow != IdeationSessionFlow::Planning
        || session.status != IdeationSessionStatus::Active
    {
        return Err(AppError::Conflict(
            "Task pipeline is no longer an active planning session".to_string(),
        ));
    }
    let owns_pipeline = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM agent_conversation_workspaces
                WHERE conversation_id = ?1 AND mode = 'tasks'
                  AND task_pipeline_session_id = ?2
             )",
            rusqlite::params![conversation_id, session_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    if !owns_pipeline {
        return Err(AppError::Conflict(
            "Tasks conversation no longer owns this pipeline".to_string(),
        ));
    }
    validate_current_user_approved_plan_sync(conn, &session)?;

    let mut statement = conn.prepare(
        "SELECT id FROM task_proposals
         WHERE session_id = ?1 AND archived_at IS NULL AND created_task_id IS NULL",
    )?;
    let current_ids = statement
        .query_map([session_id], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()?;
    let requested_ids = requested_proposal_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    if current_ids.is_empty() || current_ids != requested_ids {
        return Err(AppError::Conflict(
            "Task proposals changed after review; refresh before starting Tasks".to_string(),
        ));
    }
    if current_ids.len() >= 2 && !session.dependencies_acknowledged {
        return Err(AppError::Validation(
            "Task dependencies must be reviewed before starting Tasks".to_string(),
        ));
    }
    Ok(())
}

pub(crate) struct ApprovedPlanBundleSnapshot {
    pub overview: crate::domain::entities::Artifact,
    pub blueprint: Option<crate::domain::entities::Artifact>,
}

fn validate_current_user_approved_plan_sync(
    conn: &rusqlite::Connection,
    session: &IdeationSession,
) -> AppResult<ApprovedPlanBundleSnapshot> {
    let plan_id = session
        .plan_artifact_id
        .as_ref()
        .ok_or_else(|| AppError::Validation("Task pipeline has no current plan".to_string()))?;
    let latest_id = ArtifactRepo::resolve_latest_sync(conn, plan_id.as_str())?;
    let latest = ArtifactRepo::get_by_id_sync(conn, &latest_id)?
        .ok_or_else(|| AppError::NotFound("Current plan artifact not found".to_string()))?;
    let approval = conn
        .query_row(
            "SELECT artifact_id, artifact_version, approved_by,
                    blueprint_artifact_id, blueprint_artifact_version
             FROM plan_artifact_approvals WHERE session_id = ?1",
            [session.id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => {
                AppError::Validation("Current plan is not approved".to_string())
            }
            _ => AppError::Database(error.to_string()),
        })?;
    if approval.0 != latest.id.as_str()
        || approval.1 != i64::from(latest.metadata.version)
        || approval.2 != PlanApprovalActor::User.as_str()
    {
        return Err(AppError::Validation(
            "Current plan version requires explicit user approval".to_string(),
        ));
    }
    let blueprint = match session.plan_blueprint_artifact_id.as_ref() {
        Some(blueprint_id) => {
            let latest_blueprint_id =
                ArtifactRepo::resolve_latest_sync(conn, blueprint_id.as_str())?;
            let latest_blueprint = ArtifactRepo::get_by_id_sync(conn, &latest_blueprint_id)?
                .ok_or_else(|| {
                    AppError::NotFound("Current plan blueprint artifact not found".to_string())
                })?;
            if approval.3.as_deref() != Some(latest_blueprint.id.as_str())
                || approval.4 != Some(i64::from(latest_blueprint.metadata.version))
            {
                return Err(AppError::Validation(
                    "Current plan blueprint version requires explicit user approval".to_string(),
                ));
            }
            Some(latest_blueprint)
        }
        None if session.plan_contract_version >= 2 => {
            return Err(AppError::Validation(
                "Task pipeline has no implementation blueprint".to_string(),
            ));
        }
        None => None,
    };
    Ok(ApprovedPlanBundleSnapshot {
        overview: latest,
        blueprint,
    })
}

pub(crate) fn validate_direct_implementation_authority_sync(
    conn: &rusqlite::Connection,
    conversation_id: &str,
    session_id: &str,
    retry: bool,
) -> AppResult<ApprovedPlanBundleSnapshot> {
    let session = SessionRepo::get_by_id_sync(conn, session_id)?
        .ok_or_else(|| AppError::NotFound("Planning session not found".to_string()))?;
    let owns_plan = conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM agent_conversation_workspaces
                WHERE conversation_id = ?1 AND mode = ?3
                  AND linked_ideation_session_id = ?2
             )",
            rusqlite::params![
                conversation_id,
                session_id,
                if retry { "edit" } else { "plan" }
            ],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    if !owns_plan {
        return Err(AppError::Conflict(
            if retry {
                "Direct implementation retry is not in the authorized Edit workspace"
            } else {
                "Plan conversation no longer owns this planning session"
            }
            .to_string(),
        ));
    }
    validate_current_user_approved_plan_sync(conn, &session)
}
