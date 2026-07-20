use super::*;
use rusqlite::OptionalExtension;

pub(super) const CALLER_SESSION_ID_HEADER: &str = "x-ralphx-caller-session-id";
pub(super) const PLAN_APPROVAL_DRAFT: &str = "draft";
pub(super) const PLAN_APPROVAL_APPROVED: &str = "approved";

pub(super) struct PlanApprovalView {
    pub status: &'static str,
    pub approved_artifact_id: Option<String>,
    pub approved_version: Option<u32>,
    pub approved_at: Option<String>,
}

impl PlanApprovalView {
    pub fn draft() -> Self {
        Self {
            status: PLAN_APPROVAL_DRAFT,
            approved_artifact_id: None,
            approved_version: None,
            approved_at: None,
        }
    }

    pub fn approved(artifact_id: String, version: u32, approved_at: String) -> Self {
        Self {
            status: PLAN_APPROVAL_APPROVED,
            approved_artifact_id: Some(artifact_id),
            approved_version: Some(version),
            approved_at: Some(approved_at),
        }
    }
}

pub(super) fn attach_plan_approval(response: &mut ArtifactResponse, approval: PlanApprovalView) {
    response.plan_approval_status = Some(approval.status.to_string());
    response.plan_approved_artifact_id = approval.approved_artifact_id;
    response.plan_approved_version = approval.approved_version;
    response.plan_approved_at = approval.approved_at;
}

pub(super) fn plan_approval_view_sync(
    conn: &Connection,
    session_id: &str,
    artifact_id: &str,
    artifact_version: u32,
) -> Result<PlanApprovalView, AppError> {
    let row = conn
        .query_row(
            "SELECT artifact_id, artifact_version, approved_at
             FROM plan_artifact_approvals
             WHERE session_id = ?1 AND status = 'approved'",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;

    let Some((approved_artifact_id, approved_version, approved_at)) = row else {
        return Ok(PlanApprovalView::draft());
    };

    if approved_artifact_id == artifact_id && approved_version == i64::from(artifact_version) {
        Ok(PlanApprovalView::approved(
            approved_artifact_id,
            artifact_version,
            approved_at,
        ))
    } else {
        Ok(PlanApprovalView::draft())
    }
}

// ============================================================================
// EditError Types
// ============================================================================

/// Error type for apply_edits pure function.
#[derive(Debug)]
pub enum EditError {
    /// The old_text anchor was not found in the content.
    AnchorNotFound {
        edit_index: usize,
        old_text_preview: String,
    },
    /// The old_text anchor matches multiple locations (ambiguous).
    AmbiguousAnchor {
        edit_index: usize,
        old_text_preview: String,
    },
}

/// Apply sequential old_text→new_text edits to content.
///
/// Edits are applied SEQUENTIALLY — each edit sees the result of all previous edits,
/// not the original content. If any edit fails (anchor not found or ambiguous),
/// the entire operation returns an error and no changes are applied.
///
/// **Ambiguity check**: Verifies that each old_text appears exactly once in the
/// CURRENT content (after prior edits). The check starts searching AFTER the first
/// match ends (`pos + old_text.len()`) to avoid false positives from the match itself.
///
/// **Phantom match note**: If edit N's `new_text` introduces text matching edit N+1's
/// `old_text`, edit N+1 will operate on the introduced text (by design). Agents should
/// use unique 20+ char anchors to avoid ambiguity from sequential interactions.
#[allow(dead_code)]
pub fn apply_edits(content: &str, edits: &[PlanEdit]) -> Result<String, EditError> {
    let mut result = content.to_string();
    for (i, edit) in edits.iter().enumerate() {
        let pos = result
            .find(&edit.old_text)
            .ok_or_else(|| EditError::AnchorNotFound {
                edit_index: i,
                old_text_preview: edit.old_text.chars().take(80).collect(),
            })?;

        if result[pos + edit.old_text.len()..].contains(&edit.old_text) {
            return Err(EditError::AmbiguousAnchor {
                edit_index: i,
                old_text_preview: edit.old_text.chars().take(80).collect(),
            });
        }

        result = format!(
            "{}{}{}",
            &result[..pos],
            &edit.new_text,
            &result[pos + edit.old_text.len()..],
        );
    }
    Ok(result)
}

/// Map an AppError to an HttpError for handler responses.
pub(super) fn map_app_err(e: AppError) -> HttpError {
    match e {
        AppError::Validation(msg) => HttpError::validation(msg),
        AppError::NotFound(_) => StatusCode::NOT_FOUND.into(),
        AppError::Conflict(msg) => HttpError {
            status: StatusCode::CONFLICT,
            message: Some(msg),
        },
        AppError::FeatureDisabled(msg) => HttpError {
            status: StatusCode::CONFLICT,
            message: Some(msg),
        },
        _ => StatusCode::INTERNAL_SERVER_ERROR.into(),
    }
}

/// Async pre-transaction freeze check. Returns Err(AppError::Conflict) if a verification
/// agent is actively running on a child session of any owning session, UNLESS the caller
/// IS that verification child.
///
/// Runs BEFORE db.run_transaction() — registry methods are async and cannot be called
/// inside the synchronous spawn_blocking closure of db.run().
/// Accepts TOCTOU trade-off (single-user context, self-healing on process exit).
///
/// SIMPLIFICATION: ralphx-plan-verifier agents are autonomous (no stdin pipes) and do NOT
/// register in InteractiveProcessRegistry. Therefore is_generating = is_running.
/// This was verified during implementation: ralphx-plan-verifier agents spawn via
/// ChatService::send_message() which registers only in RunningAgentRegistry.
///
/// TRUST MODEL: caller identity is transport-owned when provided via
/// `x-ralphx-caller-session-id`; JSON `caller_session_id` remains a compatibility fallback.
/// :3847 is localhost-only (single-user desktop) — prevents accidental concurrent writes, not adversarial.
pub(super) fn resolve_caller_session_id(
    headers: &axum::http::HeaderMap,
    body_caller_session_id: Option<&str>,
) -> Option<String> {
    headers
        .get(CALLER_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| body_caller_session_id.map(ToOwned::to_owned))
}

#[derive(Debug, Clone)]
pub(super) struct ArtifactMutationAuthority {
    pub(super) agent_run_id: String,
    pub(super) conversation_id: String,
}

pub(super) fn resolve_artifact_mutation_authority(
    headers: &axum::http::HeaderMap,
) -> Option<ArtifactMutationAuthority> {
    let agent_run_id = headers.get("x-ralphx-agent-run-id")?.to_str().ok()?.trim();
    let conversation_id = headers
        .get("x-ralphx-conversation-id")?
        .to_str()
        .ok()?
        .trim();
    if agent_run_id.is_empty() || conversation_id.is_empty() {
        return None;
    }
    Some(ArtifactMutationAuthority {
        agent_run_id: agent_run_id.to_string(),
        conversation_id: conversation_id.to_string(),
    })
}

pub(super) async fn reconcile_plan_notifications(
    state: &HttpServerState,
    prior_artifact_id: Option<&str>,
    current_artifact: &Artifact,
    sessions: &[IdeationSession],
) {
    let resolver =
        crate::application::NotificationContextResolver::from_app_state(&state.app_state);
    for session in sessions
        .iter()
        .filter(|session| session.session_flow == IdeationSessionFlow::Planning)
    {
        if let Some(prior_artifact_id) = prior_artifact_id {
            state
                .app_state
                .notification_service()
                .resolve_workflow_notification(
                    &crate::application::interactive_notification_producer::plan_notification_key(
                        session.id.as_str(),
                        prior_artifact_id,
                    ),
                )
                .await;
        }
        let excluded = match resolver.session_is_automation_owned(session).await {
            Ok(true) => true,
            Ok(false) => match resolver.session_has_implementation_task(session).await {
                Ok(has_task) => has_task,
                Err(error) => {
                    tracing::warn!(error = %error, session_id = %session.id, "Failed to check implementation ownership for plan notification");
                    true
                }
            },
            Err(error) => {
                tracing::warn!(error = %error, session_id = %session.id, "Failed to check automation ownership for plan notification");
                true
            }
        };
        if excluded {
            continue;
        }
        match resolver.resolve_ideation_session_target(session).await {
            Ok(resolved) if resolved.target.kind != NotificationTargetKind::None => {
                state
                    .app_state
                    .notification_service()
                    .record(
                        crate::application::interactive_notification_producer::InteractiveNotificationProducer::plan_approval(
                            session.project_id.to_string(),
                            session.id.as_str(),
                            current_artifact.id.as_str(),
                            session.title.as_deref(),
                            resolved.target,
                        ),
                    )
                    .await;
            }
            Ok(_) => {
                tracing::warn!(session_id = %session.id, "Skipped plan notification without a navigable target")
            }
            Err(error) => {
                tracing::warn!(error = %error, session_id = %session.id, "Failed to resolve plan notification target")
            }
        }
    }
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

/// Shared core for both update_plan_artifact and edit_plan_artifact.
///
/// Takes the resolved artifact + new content, creates a new version,
/// batch-updates sessions/proposals, resets verification, and returns
/// data needed for event emission.
///
/// This helper handles only the transaction. Both update and edit handlers
/// leave any later automatic verification to the acceptance boundary. The transaction handles:
///   - Create new version (version + 1, previous_version_id = old.id)
///   - Batch-update sessions pointing to old → new
///   - Batch-update proposals (preserve plan_version_at_creation)
///   - Conditional verification reset (CAS: only if in_progress=0)
///
/// The caller is responsible for emitting events:
///   - plan_artifact:updated { previous_artifact_id: old.id, new_artifact_id: new.id, session_id }
///   - plan:proposals_may_need_update (only if linked proposals exist)
///
/// Returns a tuple containing:
///   - created artifact, old artifact id, owning sessions, linked proposal ids,
///     and legacy-reset result.
pub(super) fn finalize_plan_update(
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

    let owning_sessions = SessionRepo::get_by_plan_artifact_id_sync(conn, &old_id)?;
    let session_ids: Vec<String> = owning_sessions
        .iter()
        .map(|s| s.id.as_str().to_string())
        .collect();
    SessionRepo::batch_update_artifact_id_sync(conn, &session_ids, created.id.as_str())?;

    if let Some(authority) = authority {
        conn.query_row(
            "UPDATE agent_runs
             SET action_target_id = ?1
             WHERE id = ?2
               AND conversation_id = ?3
               AND status = 'running'
               AND action_kind = 'verify_plan'
               AND action_target_id = ?4
               AND action_context_id IN (
                 SELECT id FROM ideation_sessions WHERE plan_artifact_id = ?1
               )
             RETURNING action_context_id",
            rusqlite::params![
                created.id.as_str(),
                authority.agent_run_id,
                authority.conversation_id,
                old_id,
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    }

    let linked_proposals = ProposalRepo::get_by_plan_artifact_id_sync(conn, &old_id)?;
    let linked_proposal_ids: Vec<String> =
        linked_proposals.iter().map(|p| p.id.to_string()).collect();

    ProposalRepo::batch_update_artifact_id_sync(conn, &old_id, created.id.as_str())?;

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
