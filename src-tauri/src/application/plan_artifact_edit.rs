use crate::application::AppState;
use crate::domain::entities::{
    Artifact, ArtifactContent, ArtifactId, ArtifactMetadata, ArtifactRelation,
    ArtifactRelationType, IdeationSession, VerificationStatus,
};
use crate::domain::repositories::IdeationSessionRepository;
use crate::domain::services::running_agent_registry::{RunningAgentKey, RunningAgentRegistry};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::{
    SqliteArtifactRepository as ArtifactRepo, SqliteIdeationSessionRepository as SessionRepo,
    SqliteTaskProposalRepository as ProposalRepo,
};
use rusqlite::Connection;
use std::collections::HashSet;
#[derive(Debug, Clone)]
pub(crate) struct ArtifactMutationAuthority {
    pub(crate) agent_run_id: String,
    pub(crate) conversation_id: String,
}

impl ArtifactMutationAuthority {
    pub(crate) fn plan_approval_authority(
        &self,
    ) -> Option<crate::application::plan_approval_notification_service::PlanApprovalPublishAuthority>
    {
        Some(
            crate::application::plan_approval_notification_service::PlanApprovalPublishAuthority::new(
                self.agent_run_id.parse().ok()?,
                self.conversation_id.parse().ok()?,
            ),
        )
    }
}
pub(crate) fn retarget_verification_authority_sync(
    conn: &Connection,
    authority: Option<&ArtifactMutationAuthority>,
    session_id: &str,
    old_target: Option<&str>,
    updated_session: &IdeationSession,
) -> Result<(), AppError> {
    let (Some(authority), Some(old_target)) = (authority, old_target) else {
        return Ok(());
    };
    let bundle = updated_session
        .plan_artifact_bundle()
        .ok_or_else(|| AppError::Validation("Plan bundle became incomplete".to_string()))?;
    let new_target = bundle.action_target_id();
    let retargeted = conn.execute(
        "UPDATE agent_runs
         SET action_target_id = ?1
         WHERE id = ?2
           AND conversation_id = ?3
           AND status = 'running'
           AND action_kind = 'verify_plan'
           AND action_context_id = ?4
           AND action_target_id = ?5",
        rusqlite::params![
            new_target,
            authority.agent_run_id,
            authority.conversation_id,
            session_id,
            old_target,
        ],
    )?;
    if retargeted == 1 {
        conn.execute(
            "UPDATE deferred_plan_approval_notifications
             SET artifact_id = ?1, plan_target_id = ?2,
                 created_at = datetime('now')
             WHERE session_id = ?3
               AND COALESCE(plan_target_id, artifact_id) = ?4",
            rusqlite::params![
                bundle.overview_id.as_str(),
                new_target,
                session_id,
                old_target,
            ],
        )?;
    }
    Ok(())
}

#[doc(hidden)]
pub async fn check_verification_freeze(
    owning_sessions: &[IdeationSession],
    caller_session_id: Option<&str>,
    running_registry: &dyn RunningAgentRegistry,
    session_repo: &dyn IdeationSessionRepository,
) -> Result<(), AppError> {
    for session in owning_sessions {
        let verification_in_progress = session_repo
            .get_verification_status(&session.id)
            .await?
            .map(|(_, in_progress)| in_progress)
            .unwrap_or(session.verification_in_progress);

        if !verification_in_progress {
            continue;
        }

        let children = session_repo.get_verification_children(&session.id).await?;
        for child in &children {
            if Some(child.id.as_str()) == caller_session_id {
                continue;
            }

            let running_key = RunningAgentKey::new("ideation", child.id.as_str());
            if running_registry.is_running(&running_key).await {
                return Err(AppError::Conflict(format!(
                    "Plan is frozen — verification agent is actively working \
                     (child session: {}). Wait for the verification round to \
                     complete before editing.",
                    child.id.as_str()
                )));
            }
        }
    }
    Ok(())
}
pub(crate) fn finalize_plan_update(
    conn: &Connection,
    old_artifact: &Artifact,
    new_content: String,
    authority: Option<&ArtifactMutationAuthority>,
) -> Result<(Artifact, String, Vec<IdeationSession>, Vec<String>, bool), AppError> {
    let old_id = old_artifact.id.as_str().to_string();

    let new_artifact = Artifact {
        id: ArtifactId::new(),
        artifact_type: old_artifact.artifact_type.clone(),
        name: old_artifact.name.clone(),
        content: ArtifactContent::Inline { text: new_content },
        metadata: ArtifactMetadata::new(&old_artifact.metadata.created_by)
            .with_version(old_artifact.metadata.version + 1),
        derived_from: vec![],
        bucket_id: old_artifact.bucket_id.clone(),
        archived_at: None,
    };
    let created = ArtifactRepo::create_with_previous_version_sync(conn, new_artifact, &old_id)?;

    let mut owning_sessions = SessionRepo::get_by_plan_artifact_id_sync(conn, &old_id)?;
    let is_blueprint = owning_sessions.is_empty();
    if is_blueprint {
        owning_sessions = SessionRepo::get_by_plan_blueprint_artifact_id_sync(conn, &old_id)?;
    }
    if owning_sessions
        .iter()
        .any(|session| session.plan_contract_version == 1)
    {
        return Err(AppError::Validation(
            "Legacy plans cannot be revised one document at a time. Generate the overview and blueprint together in the planning conversation."
                .to_string(),
        ));
    }
    let old_targets: Vec<(String, String)> = owning_sessions
        .iter()
        .filter_map(|session| {
            session
                .plan_artifact_bundle()
                .map(|bundle| (session.id.to_string(), bundle.action_target_id()))
        })
        .collect();
    let session_ids: Vec<String> = owning_sessions
        .iter()
        .map(|s| s.id.as_str().to_string())
        .collect();
    if is_blueprint {
        for session_id in &session_ids {
            SessionRepo::update_plan_blueprint_artifact_id_sync(
                conn,
                session_id,
                created.id.as_str(),
            )?;
        }
    } else {
        SessionRepo::batch_update_artifact_id_sync(conn, &session_ids, created.id.as_str())?;
    }

    let mut refreshed_relations = HashSet::new();
    for session_id in &session_ids {
        let updated_session = SessionRepo::get_by_id_sync(conn, session_id)?
            .ok_or_else(|| AppError::NotFound(format!("Session {session_id} not found")))?;
        let Some(bundle) = updated_session.plan_artifact_bundle() else {
            continue;
        };
        if bundle.contract_version != 2 {
            continue;
        }
        let relation_key = (
            bundle.overview_id.to_string(),
            bundle
                .blueprint_id
                .as_ref()
                .expect("complete v2 bundle has a blueprint")
                .to_string(),
        );
        if !refreshed_relations.insert(relation_key.clone()) {
            continue;
        }
        conn.execute(
            "DELETE FROM artifact_relations
             WHERE relation_type = 'related_to'
               AND ((from_artifact_id = ?1 AND to_artifact_id = ?2)
                 OR (from_artifact_id = ?2 AND to_artifact_id = ?1))",
            rusqlite::params![old_id, relation_key.0],
        )?;
        conn.execute(
            "DELETE FROM artifact_relations
             WHERE relation_type = 'related_to'
               AND ((from_artifact_id = ?1 AND to_artifact_id = ?2)
                 OR (from_artifact_id = ?2 AND to_artifact_id = ?1))",
            rusqlite::params![old_id, relation_key.1],
        )?;
        ArtifactRepo::add_relation_sync(
            conn,
            ArtifactRelation::new(
                ArtifactId::from_string(relation_key.0),
                ArtifactId::from_string(relation_key.1),
                ArtifactRelationType::RelatedTo,
            ),
        )?;
    }

    if let Some(authority) = authority {
        for (session_id, old_target) in &old_targets {
            let updated_session = SessionRepo::get_by_id_sync(conn, session_id)?
                .ok_or_else(|| AppError::NotFound(format!("Session {session_id} not found")))?;
            retarget_verification_authority_sync(
                conn,
                Some(authority),
                session_id,
                Some(old_target),
                &updated_session,
            )?;
        }
    }

    let linked_proposals = if is_blueprint {
        ProposalRepo::get_by_blueprint_artifact_id_sync(conn, &old_id)?
    } else {
        ProposalRepo::get_by_plan_artifact_id_sync(conn, &old_id)?
    };
    let linked_proposal_ids: Vec<String> =
        linked_proposals.iter().map(|p| p.id.to_string()).collect();

    if is_blueprint {
        ProposalRepo::batch_update_blueprint_artifact_id_sync(conn, &old_id, created.id.as_str())?;
    } else {
        ProposalRepo::batch_update_artifact_id_sync(conn, &old_id, created.id.as_str())?;
    }

    let verification_reset = if let Some(session) = owning_sessions.first() {
        SessionRepo::reset_verification_sync(conn, session.id.as_str())?
    } else {
        false
    };

    Ok((
        created,
        old_id,
        owning_sessions,
        linked_proposal_ids,
        verification_reset,
    ))
}

pub struct PlanArtifactEditResult {
    pub artifact: Artifact,
    pub previous_artifact_id: String,
    pub sessions: Vec<IdeationSession>,
}

pub async fn update_plan_artifact_for_state(
    state: &AppState,
    artifact_id: String,
    content: String,
    caller_session_id: Option<&str>,
    authority: Option<&ArtifactMutationAuthority>,
) -> AppResult<PlanArtifactEditResult> {
    let id = artifact_id.clone();
    let latest = state
        .db
        .run(move |conn| ArtifactRepo::resolve_latest_sync(conn, &id))
        .await?;
    let lookup = latest.clone();
    let sessions = state
        .db
        .run(move |conn| {
            let mut rows = SessionRepo::get_by_plan_artifact_id_sync(conn, &lookup)?;
            rows.extend(SessionRepo::get_by_plan_blueprint_artifact_id_sync(
                conn, &lookup,
            )?);
            Ok(rows)
        })
        .await?;
    check_verification_freeze(
        &sessions,
        caller_session_id,
        state.running_agent_registry.as_ref(),
        state.ideation_session_repo.as_ref(),
    )
    .await?;
    let transaction_authority = authority.cloned();
    let (created, previous, sessions, linked, reset) = state
        .db
        .run_transaction(move |conn| {
            let old_id = ArtifactRepo::resolve_latest_sync(conn, &artifact_id)?;
            let old = ArtifactRepo::get_by_id_sync(conn, &old_id)?
                .ok_or_else(|| AppError::NotFound(format!("Artifact {old_id} not found")))?;
            let mut owning = SessionRepo::get_by_plan_artifact_id_sync(conn, &old_id)?;
            owning.extend(SessionRepo::get_by_plan_blueprint_artifact_id_sync(
                conn, &old_id,
            )?);
            if let Some(session) = owning.first() {
                if session.status != crate::domain::entities::IdeationSessionStatus::Active {
                    return Err(AppError::Validation(format!(
                        "Cannot modify {} session. Reopen it first.",
                        session.status
                    )));
                }
            }
            if owning.is_empty() {
                let inherited = SessionRepo::get_by_inherited_plan_artifact_id_sync(conn, &old_id)?;
                let blueprints =
                    SessionRepo::get_by_inherited_plan_blueprint_artifact_id_sync(conn, &old_id)?;
                if !inherited.is_empty() || !blueprints.is_empty() {
                    return Err(AppError::Validation(
                        "Cannot update inherited plan. Use create_plan_artifact to create a session-specific plan."
                            .into(),
                    ));
                }
            }
            finalize_plan_update(conn, &old, content, transaction_authority.as_ref())
        })
        .await?;
    if reset {
        if let Some(session) = sessions.first() {
            crate::application::verification_event_emitters::emit_verification_status_changed(
                state.events.as_ref(),
                session.id.as_str(),
                VerificationStatus::Unverified,
                false,
                None,
                None,
                Some(session.verification_generation),
            );
        }
    }
    let text = match &created.content {
        ArtifactContent::Inline { text } => text.clone(),
        ArtifactContent::File { path } => format!("[File: {path}]"),
    };
    state.events.emit("plan_artifact:updated",serde_json::json!({"artifactId":created.id.as_str(),"previousArtifactId":previous,"sessionId":sessions.first().map(|s|s.id.as_str()),"artifact":{"id":created.id.as_str(),"name":created.name,"content":text,"version":created.metadata.version}}));
    if !linked.is_empty() {
        state.events.emit("plan:proposals_may_need_update",serde_json::json!({"artifactId":created.id.as_str(),"previousArtifactId":previous,"proposalIds":linked,"newVersion":created.metadata.version,"sessionId":sessions.first().map(|s|s.id.as_str()),"proposalsRelinked":true}));
    }
    let publish = authority.and_then(|v| v.plan_approval_authority());
    let approval_notification_deps = crate::application::plan_approval_notification_service::PlanApprovalNotificationDeps::from_app_state(state);
    crate::application::plan_approval_notification_service::reconcile_plan_approval_on_publish_with_deps(
        &approval_notification_deps,
        None,
        created.id.as_str(),
        &sessions,
        publish.as_ref(),
    )
    .await;
    Ok(PlanArtifactEditResult {
        artifact: created,
        previous_artifact_id: previous,
        sessions,
    })
}
