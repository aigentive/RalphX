use super::*;
use rusqlite::OptionalExtension;

pub(super) const CALLER_SESSION_ID_HEADER: &str = "x-ralphx-caller-session-id";
pub(super) const PLAN_APPROVAL_DRAFT: &str = "draft";
pub(super) const PLAN_APPROVAL_APPROVED: &str = "approved";

pub(super) struct PlanApprovalView {
    pub status: &'static str,
    pub approved_artifact_id: Option<String>,
    pub approved_version: Option<u32>,
    pub approved_blueprint_artifact_id: Option<String>,
    pub approved_blueprint_version: Option<u32>,
    pub approved_at: Option<String>,
}

impl PlanApprovalView {
    pub fn draft() -> Self {
        Self {
            status: PLAN_APPROVAL_DRAFT,
            approved_artifact_id: None,
            approved_version: None,
            approved_blueprint_artifact_id: None,
            approved_blueprint_version: None,
            approved_at: None,
        }
    }

    pub fn approved(
        artifact_id: String,
        version: u32,
        blueprint_artifact_id: Option<String>,
        blueprint_version: Option<u32>,
        approved_at: String,
    ) -> Self {
        Self {
            status: PLAN_APPROVAL_APPROVED,
            approved_artifact_id: Some(artifact_id),
            approved_version: Some(version),
            approved_blueprint_artifact_id: blueprint_artifact_id,
            approved_blueprint_version: blueprint_version,
            approved_at: Some(approved_at),
        }
    }
}

pub(super) fn attach_plan_approval(response: &mut ArtifactResponse, approval: PlanApprovalView) {
    response.plan_approval_status = Some(approval.status.to_string());
    response.plan_approved_artifact_id = approval.approved_artifact_id;
    response.plan_approved_version = approval.approved_version;
    response.plan_approved_blueprint_artifact_id = approval.approved_blueprint_artifact_id;
    response.plan_approved_blueprint_version = approval.approved_blueprint_version;
    response.plan_approved_at = approval.approved_at;
}

pub(super) fn plan_approval_view_sync(
    conn: &Connection,
    session_id: &str,
    artifact_id: &str,
    artifact_version: u32,
    blueprint_artifact_id: Option<&str>,
    blueprint_artifact_version: Option<u32>,
) -> Result<PlanApprovalView, AppError> {
    let row = conn
        .query_row(
            "SELECT artifact_id, artifact_version, blueprint_artifact_id,
                    blueprint_artifact_version, approved_at
             FROM plan_artifact_approvals
             WHERE session_id = ?1 AND status = 'approved'",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;

    let Some((
        approved_artifact_id,
        approved_version,
        approved_blueprint_artifact_id,
        approved_blueprint_version,
        approved_at,
    )) = row
    else {
        return Ok(PlanApprovalView::draft());
    };

    if approved_artifact_id == artifact_id
        && approved_version == i64::from(artifact_version)
        && approved_blueprint_artifact_id.as_deref() == blueprint_artifact_id
        && approved_blueprint_version == blueprint_artifact_version.map(i64::from)
    {
        Ok(PlanApprovalView::approved(
            approved_artifact_id,
            artifact_version,
            approved_blueprint_artifact_id,
            approved_blueprint_version.map(|value| value as u32),
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
    current_artifact: &Artifact,
    sessions: &[IdeationSession],
    mutation_authority: Option<&ArtifactMutationAuthority>,
) {
    let publish_authority = mutation_authority.and_then(|value| value.plan_approval_authority());
    let approval_notification_deps = crate::application::plan_approval_notification_service::PlanApprovalNotificationDeps::from_app_state(&state.app_state);
    crate::application::plan_approval_notification_service::reconcile_plan_approval_on_publish_with_deps(
        &approval_notification_deps,
        None,
        current_artifact.id.as_str(),
        sessions,
        publish_authority.as_ref(),
    )
    .await;
}

pub(super) fn next_artifact_version_sync(
    conn: &Connection,
    artifact_id: Option<&ArtifactId>,
) -> Result<u32, AppError> {
    let Some(artifact_id) = artifact_id else {
        return Ok(1);
    };
    let artifact = ArtifactRepo::get_by_id_sync(conn, artifact_id.as_str())?.ok_or_else(|| {
        AppError::NotFound(format!("Artifact {} not found", artifact_id.as_str()))
    })?;
    Ok(artifact.metadata.version + 1)
}

pub(super) fn delete_current_bundle_relation_sync(
    conn: &Connection,
    session: &IdeationSession,
) -> Result<(), AppError> {
    let Some(bundle) = session.plan_artifact_bundle() else {
        return Ok(());
    };
    let Some(blueprint_id) = bundle.blueprint_id else {
        return Ok(());
    };
    conn.execute(
        "DELETE FROM artifact_relations
         WHERE relation_type = 'related_to'
           AND ((from_artifact_id = ?1 AND to_artifact_id = ?2)
             OR (from_artifact_id = ?2 AND to_artifact_id = ?1))",
        rusqlite::params![bundle.overview_id.as_str(), blueprint_id.as_str()],
    )?;
    Ok(())
}
