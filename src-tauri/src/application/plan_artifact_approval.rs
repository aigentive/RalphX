use std::str::FromStr;

use async_trait::async_trait;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};

use crate::domain::entities::ideation::PlanArtifactBundle;
use crate::domain::entities::{
    Artifact, ArtifactId, IdeationSessionFlow, IdeationSessionId, IdeationSessionStatus,
};
use crate::domain::repositories::{PlanApprovalActor, PlanArtifactApproval};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::{DbConnection, SqliteArtifactRepository};

#[derive(Debug, Clone)]
pub(crate) struct ApprovedPlanArtifact {
    pub session_id: IdeationSessionId,
    pub artifact: Artifact,
    pub blueprint_artifact: Option<Artifact>,
    pub approved_at: String,
}

pub(crate) async fn approve_plan_artifact_for_state(
    app_state: &crate::application::AppState,
    session_id: String,
    artifact_id: Option<String>,
    blueprint_artifact_id: Option<String>,
    blueprint_artifact_version: Option<u32>,
) -> AppResult<ApprovedPlanArtifact> {
    let approved = app_state
        .db
        .run_transaction(move |conn| {
            approve_current_plan_artifact_for_displayed_bundle_sync(
                conn,
                IdeationSessionId::from_string(session_id),
                artifact_id.as_deref(),
                blueprint_artifact_id.as_deref(),
                blueprint_artifact_version,
                PlanApprovalActor::User,
            )
        })
        .await?;

    let blueprint_id = approved
        .blueprint_artifact
        .as_ref()
        .map(|artifact| artifact.id.to_string());
    let blueprint_version = approved
        .blueprint_artifact
        .as_ref()
        .map(|artifact| artifact.metadata.version);
    let plan_bundle = PlanArtifactBundle {
        overview_id: approved.artifact.id.clone(),
        blueprint_id: approved
            .blueprint_artifact
            .as_ref()
            .map(|artifact| artifact.id.clone()),
        contract_version: if approved.blueprint_artifact.is_some() {
            2
        } else {
            1
        },
        is_inherited: false,
    };
    app_state.events.emit(
        "plan_artifact:approved",
        serde_json::json!({
            "sessionId": approved.session_id.as_str(),
            "artifactId": approved.artifact.id.as_str(),
            "version": approved.artifact.metadata.version,
            "blueprintArtifactId": blueprint_id,
            "blueprintVersion": blueprint_version,
            "approvedAt": approved.approved_at,
        }),
    );
    let approval_notification_deps = crate::application::plan_approval_notification_service::PlanApprovalNotificationDeps::from_app_state(app_state);
    crate::application::plan_approval_notification_service::resolve_plan_approval_notifications_with_deps(
        &approval_notification_deps,
        &approved.session_id,
        &plan_bundle,
    )
    .await;

    let tasks_enabled = app_state
        .ideation_settings_repo
        .get_settings()
        .await
        .map(|settings| settings.tasks_enabled)
        .unwrap_or(false);
    if tasks_enabled {
        crate::application::plan_complexity_assessment::spawn_plan_complexity_assessor_after_approval(
            std::sync::Arc::new(app_state.clone()),
            approved.session_id.as_str().to_string(),
            approved.artifact.id.to_string(),
            approved.artifact.metadata.version,
        );
    }

    Ok(approved)
}

#[async_trait]
pub(crate) trait PlanArtifactApprovalWriter: Send + Sync {
    async fn approve_current_plan_artifact(
        &self,
        session_id: IdeationSessionId,
        requested_artifact_id: Option<String>,
        approved_by: PlanApprovalActor,
    ) -> AppResult<PlanArtifactApproval>;
}

#[derive(Clone)]
pub(crate) struct DbPlanArtifactApprovalWriter {
    db: DbConnection,
}

impl DbPlanArtifactApprovalWriter {
    pub(crate) fn new(db: DbConnection) -> Self {
        Self { db }
    }
}

#[async_trait]
impl PlanArtifactApprovalWriter for DbPlanArtifactApprovalWriter {
    async fn approve_current_plan_artifact(
        &self,
        session_id: IdeationSessionId,
        requested_artifact_id: Option<String>,
        approved_by: PlanApprovalActor,
    ) -> AppResult<PlanArtifactApproval> {
        self.db
            .run_transaction(move |conn| {
                let approved = approve_current_plan_artifact_sync(
                    conn,
                    session_id,
                    requested_artifact_id.as_deref(),
                    approved_by,
                )?;
                Ok(PlanArtifactApproval {
                    session_id: approved.session_id,
                    artifact_id: approved.artifact.id,
                    artifact_version: approved.artifact.metadata.version,
                    blueprint_artifact_id: approved
                        .blueprint_artifact
                        .as_ref()
                        .map(|artifact| artifact.id.clone()),
                    blueprint_artifact_version: approved
                        .blueprint_artifact
                        .as_ref()
                        .map(|artifact| artifact.metadata.version),
                    approved_at: approved.approved_at,
                    approved_by: approved_by.as_str().to_string(),
                })
            })
            .await
    }
}

pub(crate) fn approve_current_plan_artifact_sync(
    conn: &Connection,
    session_id: IdeationSessionId,
    requested_artifact_id: Option<&str>,
    approved_by: PlanApprovalActor,
) -> AppResult<ApprovedPlanArtifact> {
    approve_current_plan_artifact_for_bundle_sync(
        conn,
        session_id,
        requested_artifact_id,
        None,
        None,
        false,
        approved_by,
    )
}

pub(crate) fn approve_current_plan_artifact_for_displayed_bundle_sync(
    conn: &Connection,
    session_id: IdeationSessionId,
    requested_artifact_id: Option<&str>,
    requested_blueprint_artifact_id: Option<&str>,
    requested_blueprint_artifact_version: Option<u32>,
    approved_by: PlanApprovalActor,
) -> AppResult<ApprovedPlanArtifact> {
    approve_current_plan_artifact_for_bundle_sync(
        conn,
        session_id,
        requested_artifact_id,
        requested_blueprint_artifact_id,
        requested_blueprint_artifact_version,
        true,
        approved_by,
    )
}

fn approve_current_plan_artifact_for_bundle_sync(
    conn: &Connection,
    session_id: IdeationSessionId,
    requested_artifact_id: Option<&str>,
    requested_blueprint_artifact_id: Option<&str>,
    requested_blueprint_artifact_version: Option<u32>,
    require_displayed_blueprint: bool,
    approved_by: PlanApprovalActor,
) -> AppResult<ApprovedPlanArtifact> {
    let session = get_session_for_approval_sync(conn, session_id.as_str())?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;

    assert_session_mutable_for_plan_approval(session.status)?;

    if session.session_flow != IdeationSessionFlow::Planning {
        return Err(AppError::Validation(
            "Plan approval is only available for planning sessions".to_string(),
        ));
    }

    let plan_artifact_id = session.plan_artifact_id.ok_or_else(|| {
        AppError::Validation(
            "Cannot approve plan: planning session does not have an owned plan artifact"
                .to_string(),
        )
    })?;

    if let Some(requested) = requested_artifact_id {
        if requested != plan_artifact_id.as_str() {
            return Err(AppError::Conflict(
                "Plan changed before approval. Refresh the current plan and approve again."
                    .to_string(),
            ));
        }
    }

    let artifact = SqliteArtifactRepository::get_by_id_sync(conn, plan_artifact_id.as_str())?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Plan artifact {} not found",
                plan_artifact_id.as_str()
            ))
        })?;
    let blueprint_artifact = match session.plan_blueprint_artifact_id {
        Some(blueprint_id) => {
            if require_displayed_blueprint
                && (requested_blueprint_artifact_id.is_none()
                    || requested_blueprint_artifact_version.is_none())
            {
                return Err(AppError::Conflict(
                    "Plan changed before approval. Refresh the current plan and approve again."
                        .to_string(),
                ));
            }
            if requested_blueprint_artifact_id
                .is_some_and(|requested| requested != blueprint_id.as_str())
            {
                return Err(AppError::Conflict(
                    "Plan changed before approval. Refresh the current plan and approve again."
                        .to_string(),
                ));
            }
            let blueprint = SqliteArtifactRepository::get_by_id_sync(conn, blueprint_id.as_str())?
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "Plan blueprint artifact {} not found",
                        blueprint_id.as_str()
                    ))
                })?;
            if requested_blueprint_artifact_version
                .is_some_and(|version| version != blueprint.metadata.version)
            {
                return Err(AppError::Conflict(
                    "Plan changed before approval. Refresh the current plan and approve again."
                        .to_string(),
                ));
            }
            Some(blueprint)
        }
        None if session.plan_contract_version >= 2 => {
            return Err(AppError::Validation(
                "Cannot approve plan: implementation blueprint has not been created".to_string(),
            ));
        }
        None => {
            if requested_blueprint_artifact_id.is_some()
                || requested_blueprint_artifact_version.is_some()
            {
                return Err(AppError::Conflict(
                    "Plan changed before approval. Refresh the current plan and approve again."
                        .to_string(),
                ));
            }
            None
        }
    };
    let approved_at = Utc::now().to_rfc3339();
    upsert_plan_approval_sync(
        conn,
        session_id.as_str(),
        &artifact,
        blueprint_artifact.as_ref(),
        &approved_at,
        approved_by,
    )?;

    Ok(ApprovedPlanArtifact {
        session_id,
        artifact,
        blueprint_artifact,
        approved_at,
    })
}

pub(crate) fn upsert_plan_approval_sync(
    conn: &Connection,
    session_id: &str,
    artifact: &Artifact,
    blueprint_artifact: Option<&Artifact>,
    approved_at: &str,
    approved_by: PlanApprovalActor,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO plan_artifact_approvals (
            session_id, artifact_id, artifact_version,
            blueprint_artifact_id, blueprint_artifact_version,
            status, approved_at, approved_by
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'approved', ?6, ?7)
         ON CONFLICT(session_id) DO UPDATE SET
            artifact_id = excluded.artifact_id,
            artifact_version = excluded.artifact_version,
            blueprint_artifact_id = excluded.blueprint_artifact_id,
            blueprint_artifact_version = excluded.blueprint_artifact_version,
            status = excluded.status,
            approved_at = excluded.approved_at,
            approved_by = excluded.approved_by",
        rusqlite::params![
            session_id,
            artifact.id.as_str(),
            i64::from(artifact.metadata.version),
            blueprint_artifact.map(|artifact| artifact.id.as_str()),
            blueprint_artifact.map(|artifact| i64::from(artifact.metadata.version)),
            approved_at,
            approved_by.as_str(),
        ],
    )?;
    Ok(())
}

fn assert_session_mutable_for_plan_approval(status: IdeationSessionStatus) -> AppResult<()> {
    match status {
        IdeationSessionStatus::Archived | IdeationSessionStatus::Accepted => {
            Err(AppError::Validation(format!(
                "Cannot modify {} session. Reopen it first.",
                status
            )))
        }
        IdeationSessionStatus::Active => Ok(()),
    }
}

#[derive(Debug)]
struct ApprovalSessionRow {
    status: IdeationSessionStatus,
    session_flow: IdeationSessionFlow,
    plan_artifact_id: Option<ArtifactId>,
    plan_blueprint_artifact_id: Option<ArtifactId>,
    plan_contract_version: i32,
}

fn get_session_for_approval_sync(
    conn: &Connection,
    session_id: &str,
) -> AppResult<Option<ApprovalSessionRow>> {
    conn.query_row(
        "SELECT status, session_flow, plan_artifact_id,
                plan_blueprint_artifact_id, plan_contract_version
         FROM ideation_sessions
         WHERE id = ?1",
        [session_id],
        |row| {
            let status_raw: String = row.get(0)?;
            let flow_raw: String = row.get(1)?;
            let plan_artifact_id: Option<String> = row.get(2)?;
            let plan_blueprint_artifact_id: Option<String> = row.get(3)?;
            Ok(ApprovalSessionRow {
                status: IdeationSessionStatus::from_str(&status_raw)
                    .map_err(|error| rusqlite::Error::InvalidParameterName(error.to_string()))?,
                session_flow: IdeationSessionFlow::from_str(&flow_raw)
                    .map_err(rusqlite::Error::InvalidParameterName)?,
                plan_artifact_id: plan_artifact_id.map(ArtifactId::from_string),
                plan_blueprint_artifact_id: plan_blueprint_artifact_id.map(ArtifactId::from_string),
                plan_contract_version: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(AppError::from)
}

#[cfg(test)]
#[path = "plan_artifact_approval_tests.rs"]
mod tests;
