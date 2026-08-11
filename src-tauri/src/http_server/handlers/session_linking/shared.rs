use super::*;

pub(super) fn json_error(status: StatusCode, error: impl Into<String>) -> JsonError {
    (status, Json(serde_json::json!({ "error": error.into() })))
}

#[doc(hidden)]
pub fn synthesize_verification_prompt(
    purpose: &Option<String>,
    verification_generation: Option<i32>,
    max_rounds: u32,
    effective_description: &Option<String>,
    parent_session_id: &str,
) -> Option<String> {
    if purpose.as_deref() != Some("verification") || effective_description.is_some() {
        return None;
    }
    let generation = verification_generation.unwrap_or(1);
    Some(format!(
        "Begin plan verification.\n\nparent_session_id: {}, generation: {}, max_rounds: {}",
        parent_session_id, generation, max_rounds
    ))
}

pub(super) async fn load_parent_context(
    state: &HttpServerState,
    parent: &IdeationSession,
) -> ParentContextResponse {
    let plan_content = if let Some(plan_id) = &parent.plan_artifact_id {
        state
            .app_state
            .artifact_repo
            .get_by_id(plan_id)
            .await
            .ok()
            .flatten()
            .and_then(|artifact| {
                if let crate::domain::entities::ArtifactContent::Inline { text } = artifact.content
                {
                    Some(text)
                } else {
                    None
                }
            })
    } else {
        None
    };

    let proposals = state
        .app_state
        .task_proposal_repo
        .get_by_session(&parent.id)
        .await
        .unwrap_or_default();

    let proposal_summaries = proposals
        .iter()
        .map(|p| ParentProposalSummary {
            id: p.id.to_string(),
            title: p.title.clone(),
            category: p.category.to_string(),
            priority: p.suggested_priority.to_string(),
            status: p.status.to_string(),
            acceptance_criteria: p.acceptance_criteria.clone(),
        })
        .collect();

    ParentContextResponse {
        parent_session: ParentSessionSummary {
            id: parent.id.to_string(),
            title: parent
                .title
                .clone()
                .unwrap_or_else(|| "Untitled".to_string()),
            status: parent.status.to_string(),
        },
        plan_content,
        proposals: proposal_summaries,
    }
}

pub(crate) fn build_ideation_chat_service(
    state: &HttpServerState,
    _session: &IdeationSession,
) -> crate::application::AppChatService {
    let app = &state.app_state;
    app.build_chat_service_with_execution_state(Arc::clone(&state.execution_state))
}

pub(super) async fn rollback_verification_state(
    state: &HttpServerState,
    parent_id: &IdeationSessionId,
    current_generation: i32,
    failure_context: &'static str,
) {
    let parent_id_str = parent_id.as_str().to_string();
    let pid_for_reset = parent_id_str.clone();
    let db = state.app_state.db.clone();

    if let Err(re) = db
        .run(move |conn| SessionRepo::reset_auto_verify_sync(conn, &pid_for_reset))
        .await
    {
        error!(
            "Failed to rollback verification state after {}: {}",
            failure_context, re
        );
    } else {
        emit_verification_status_changed(
            state.app_state.events.as_ref(),
            &parent_id_str,
            VerificationStatus::Unverified,
            false,
            None,
            Some("spawn_failed"),
            Some(current_generation),
        );
    }
}
