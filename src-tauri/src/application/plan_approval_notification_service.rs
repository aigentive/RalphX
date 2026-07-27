use rusqlite::OptionalExtension;

use crate::application::interactive_notification_producer::{
    plan_notification_key, InteractiveNotificationProducer,
};
use crate::application::plan_verification_service::get_plan_verification_status;
use crate::application::{AppState, NotificationContextResolver};
use crate::domain::entities::ideation::PlanArtifactBundle;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus, AgentRunActionKind,
    AgentRunId, AgentRunStatus, ChatConversationId, IdeationSession, IdeationSessionFlow,
    IdeationSessionId, NotificationTargetKind,
};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanApprovalNotificationDisposition {
    Deferred,
    Recorded,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlanApprovalPublishAuthority {
    pub(crate) run_id: AgentRunId,
    pub(crate) conversation_id: ChatConversationId,
}

impl PlanApprovalPublishAuthority {
    pub(crate) fn new(run_id: AgentRunId, conversation_id: ChatConversationId) -> Self {
        Self {
            run_id,
            conversation_id,
        }
    }
}

async fn set_deferred_marker(
    state: &AppState,
    session_id: &IdeationSessionId,
    artifact_id: &str,
    plan_target_id: &str,
) -> AppResult<()> {
    let session_id = session_id.as_str().to_string();
    let artifact_id = artifact_id.to_string();
    let plan_target_id = plan_target_id.to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT INTO deferred_plan_approval_notifications
                    (session_id, artifact_id, plan_target_id, created_at)
                 VALUES (?1, ?2, ?3, datetime('now'))
                 ON CONFLICT(session_id) DO UPDATE SET
                    artifact_id = excluded.artifact_id,
                    plan_target_id = excluded.plan_target_id,
                    created_at = excluded.created_at",
                rusqlite::params![session_id, artifact_id, plan_target_id],
            )?;
            Ok(())
        })
        .await
}

async fn deferred_artifact_id(
    state: &AppState,
    session_id: &IdeationSessionId,
) -> AppResult<Option<String>> {
    let session_id = session_id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            let mut statement = conn.prepare(
                "SELECT artifact_id FROM deferred_plan_approval_notifications WHERE session_id = ?1",
            )?;
            let mut rows = statement.query([session_id])?;
            Ok(rows.next()?.map(|row| row.get(0)).transpose()?)
        })
        .await
}

async fn deferred_plan_marker(
    state: &AppState,
    session_id: &IdeationSessionId,
) -> AppResult<Option<(String, String)>> {
    let session_id = session_id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.query_row(
                "SELECT artifact_id, COALESCE(plan_target_id, artifact_id)
                 FROM deferred_plan_approval_notifications
                 WHERE session_id = ?1",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(Into::into)
        })
        .await
}

async fn clear_deferred_marker(state: &AppState, session_id: &IdeationSessionId) -> AppResult<()> {
    let session_id = session_id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "DELETE FROM deferred_plan_approval_notifications WHERE session_id = ?1",
                [session_id],
            )?;
            Ok(())
        })
        .await
}

pub async fn has_deferred_plan_approval(
    state: &AppState,
    session_id: &IdeationSessionId,
    artifact_id: &str,
) -> AppResult<bool> {
    Ok(deferred_artifact_id(state, session_id).await?.as_deref() == Some(artifact_id))
}

pub async fn has_deferred_plan_approval_in_db(
    db: &crate::infrastructure::sqlite::DbConnection,
    session_id: &IdeationSessionId,
    artifact_id: &str,
) -> AppResult<bool> {
    let session_id = session_id.as_str().to_string();
    let artifact_id = artifact_id.to_string();
    db.run(move |conn| {
        Ok(conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM deferred_plan_approval_notifications
                WHERE session_id = ?1 AND artifact_id = ?2
             )",
            rusqlite::params![session_id, artifact_id],
            |row| row.get::<_, bool>(0),
        )?)
    })
    .await
}

pub async fn has_deferred_plan_target_in_db(
    db: &crate::infrastructure::sqlite::DbConnection,
    session_id: &IdeationSessionId,
    plan_target_id: &str,
) -> AppResult<bool> {
    let session_id = session_id.as_str().to_string();
    let plan_target_id = plan_target_id.to_string();
    db.run(move |conn| {
        Ok(conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM deferred_plan_approval_notifications
                WHERE session_id = ?1
                  AND COALESCE(plan_target_id, artifact_id) = ?2
             )",
            rusqlite::params![session_id, plan_target_id],
            |row| row.get::<_, bool>(0),
        )?)
    })
    .await
}

async fn session_is_notification_eligible(
    state: &AppState,
    session: &IdeationSession,
) -> AppResult<bool> {
    if session.session_flow != IdeationSessionFlow::Planning {
        return Ok(false);
    }
    let resolver = NotificationContextResolver::from_app_state(state);
    Ok(!resolver.session_is_automation_owned(session).await?
        && !resolver.session_has_implementation_task(session).await?)
}

async fn should_defer_on_publish(
    state: &AppState,
    session: &IdeationSession,
    artifact_id: &str,
    authority: Option<&PlanApprovalPublishAuthority>,
) -> bool {
    let Some(authority) = authority else {
        return false;
    };
    let settings = match state.ideation_settings_repo.get_settings().await {
        Ok(settings) => settings,
        Err(error) => {
            tracing::warn!(error = %error, session_id = %session.id, "Failed to inspect plan auto-verification setting; publishing approval attention immediately");
            return false;
        }
    };
    if !settings.auto_verify_draft_plans {
        return false;
    }
    let workspace = match state
        .agent_conversation_workspace_repo
        .get_by_linked_ideation_session_id(&session.id)
        .await
    {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return false,
        Err(error) => {
            tracing::warn!(error = %error, session_id = %session.id, "Failed to inspect plan workspace authority; publishing approval attention immediately");
            return false;
        }
    };
    if workspace.status != AgentConversationWorkspaceStatus::Active
        || workspace.mode != AgentConversationWorkspaceMode::Plan
        || workspace.conversation_id != authority.conversation_id
    {
        return false;
    }
    let run = match state.agent_run_repo.get_by_id(&authority.run_id).await {
        Ok(Some(run)) => run,
        Ok(None) => return false,
        Err(error) => {
            tracing::warn!(error = %error, run_id = %authority.run_id, "Failed to inspect plan publish run authority; publishing approval attention immediately");
            return false;
        }
    };
    if run.conversation_id != authority.conversation_id || run.status != AgentRunStatus::Running {
        return false;
    }
    match run.action_kind {
        None => true,
        Some(AgentRunActionKind::VerifyPlan) => {
            run.action_context_id.as_deref() == Some(session.id.as_str())
                && run.action_target_id.as_deref() == Some(artifact_id)
        }
        Some(AgentRunActionKind::PrAutofix | AgentRunActionKind::WorkspaceReviewFixer) => false,
    }
}

async fn record_plan_approval(
    state: &AppState,
    session: &IdeationSession,
) -> AppResult<PlanApprovalNotificationDisposition> {
    if !session_is_notification_eligible(state, session).await? {
        clear_deferred_marker(state, &session.id).await?;
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    }
    if state
        .plan_approval_repo
        .get_by_session(&session.id)
        .await?
        .is_some_and(|approval| {
            session.plan_artifact_bundle().is_some_and(|bundle| {
                approval.artifact_id == bundle.overview_id
                    && approval.blueprint_artifact_id == bundle.blueprint_id
            })
        })
    {
        clear_deferred_marker(state, &session.id).await?;
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    }
    let resolver = NotificationContextResolver::from_app_state(state);
    let resolved = resolver.resolve_ideation_session_target(session).await?;
    if resolved.target.kind == NotificationTargetKind::None {
        return Err(AppError::Infrastructure(
            "Plan approval notification has no navigable target".to_string(),
        ));
    }
    let plan_target_id = session
        .plan_artifact_bundle()
        .ok_or_else(|| AppError::Validation("Plan bundle is incomplete".to_string()))?
        .action_target_id();
    state
        .notification_service()
        .record_result(InteractiveNotificationProducer::plan_approval(
            session.project_id.to_string(),
            session.id.as_str(),
            &plan_target_id,
            session.title.as_deref(),
            resolved.target,
        ))
        .await?;
    clear_deferred_marker(state, &session.id).await?;
    Ok(PlanApprovalNotificationDisposition::Recorded)
}

pub(crate) async fn resolve_plan_approval_notifications(
    state: &AppState,
    session_id: &IdeationSessionId,
    bundle: &PlanArtifactBundle,
) {
    let plan_target_id = bundle.action_target_id();
    state
        .notification_service()
        .resolve_workflow_notification(&plan_notification_key(session_id.as_str(), &plan_target_id))
        .await;
    if plan_target_id != bundle.overview_id.as_str() {
        state
            .notification_service()
            .resolve_workflow_notification(&plan_notification_key(
                session_id.as_str(),
                bundle.overview_id.as_str(),
            ))
            .await;
    }
}

pub(crate) async fn reconcile_plan_approval_on_publish(
    state: &AppState,
    prior_artifact_id: Option<&str>,
    artifact_id: &str,
    sessions: &[IdeationSession],
    authority: Option<&PlanApprovalPublishAuthority>,
) {
    for session in sessions {
        let prior_bundle = session.plan_artifact_bundle();
        if let Some(prior_artifact_id) = prior_artifact_id {
            state
                .notification_service()
                .resolve_workflow_notification(&plan_notification_key(
                    session.id.as_str(),
                    prior_artifact_id,
                ))
                .await;
        } else if let Some(prior_bundle) = prior_bundle.as_ref() {
            resolve_plan_approval_notifications(state, &session.id, prior_bundle).await;
        }
        let result = async {
            let current_session = state
                .ideation_session_repo
                .get_by_id(&session.id)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session.id)))?;
            let Some(bundle) = current_session.plan_artifact_bundle() else {
                clear_deferred_marker(state, &session.id).await?;
                return Ok(PlanApprovalNotificationDisposition::Skipped);
            };
            let overview_id = bundle.overview_id.as_str();
            let plan_target_id = bundle.action_target_id();
            if !session_is_notification_eligible(state, &current_session).await? {
                clear_deferred_marker(state, &session.id).await?;
                return Ok(PlanApprovalNotificationDisposition::Skipped);
            }
            if should_defer_on_publish(state, &current_session, &plan_target_id, authority).await {
                set_deferred_marker(state, &session.id, overview_id, &plan_target_id).await?;
                return Ok(PlanApprovalNotificationDisposition::Deferred);
            }
            record_plan_approval(state, &current_session).await
        }
        .await;
        if let Err(error) = result {
            tracing::warn!(error = %error, session_id = %session.id, artifact_id, "Failed to reconcile plan approval notification");
        }
    }
}

pub async fn release_deferred_plan_approval(
    state: &AppState,
    session_id: &IdeationSessionId,
) -> AppResult<PlanApprovalNotificationDisposition> {
    let Some((_marker_artifact_id, marker_target_id)) =
        deferred_plan_marker(state, session_id).await?
    else {
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    };
    let session = state
        .ideation_session_repo
        .get_by_id(session_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;
    if session
        .plan_artifact_bundle()
        .map(|bundle| bundle.action_target_id())
        .as_deref()
        != Some(marker_target_id.as_str())
    {
        clear_deferred_marker(state, session_id).await?;
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    }
    record_plan_approval(state, &session).await
}

pub async fn release_deferred_plan_approval_for_conversation(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> AppResult<PlanApprovalNotificationDisposition> {
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    };
    if workspace.status != AgentConversationWorkspaceStatus::Active
        || workspace.mode != AgentConversationWorkspaceMode::Plan
    {
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    }
    let Some(session_id) = workspace.linked_ideation_session_id else {
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    };
    if deferred_artifact_id(state, &session_id).await?.is_none() {
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    }
    let status = get_plan_verification_status(state, &session_id).await?;
    if status.in_progress {
        return Ok(PlanApprovalNotificationDisposition::Deferred);
    }
    release_deferred_plan_approval(state, &session_id).await
}

pub async fn release_deferred_plan_approval_for_run(
    state: &AppState,
    run_id: &AgentRunId,
) -> AppResult<PlanApprovalNotificationDisposition> {
    let Some(run) = state.agent_run_repo.get_by_id(run_id).await? else {
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    };
    if run.action_kind != Some(AgentRunActionKind::VerifyPlan)
        || !matches!(
            run.status,
            AgentRunStatus::Completed | AgentRunStatus::Failed | AgentRunStatus::Cancelled
        )
    {
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    }
    let Some(session_id) = run.action_context_id.as_deref() else {
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    };
    let session_id = IdeationSessionId::from_string(session_id.to_string());
    let Some(session) = state.ideation_session_repo.get_by_id(&session_id).await? else {
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    };
    if session
        .plan_artifact_bundle()
        .map(|bundle| bundle.action_target_id())
        .as_deref()
        != run.action_target_id.as_deref()
    {
        return Ok(PlanApprovalNotificationDisposition::Skipped);
    }
    release_deferred_plan_approval(state, &session_id).await
}

pub async fn reconcile_deferred_plan_approvals_on_startup(state: &AppState) -> AppResult<()> {
    let session_ids = state
        .db
        .run(|conn| {
            let mut statement = conn.prepare(
                "SELECT session_id FROM deferred_plan_approval_notifications ORDER BY created_at",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            Ok(rows.filter_map(Result::ok).collect::<Vec<_>>())
        })
        .await?;
    for session_id in session_ids {
        let session_id = IdeationSessionId::from_string(session_id);
        match get_plan_verification_status(state, &session_id).await {
            Ok(status) if status.in_progress => {}
            Ok(_) => {
                if let Err(error) = release_deferred_plan_approval(state, &session_id).await {
                    tracing::warn!(error = %error, session_id = %session_id, "Failed to release deferred plan approval during startup reconciliation");
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, session_id = %session_id, "Failed to inspect deferred plan approval during startup reconciliation");
            }
        }
    }
    Ok(())
}
