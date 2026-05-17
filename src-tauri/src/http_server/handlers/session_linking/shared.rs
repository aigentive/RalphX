use super::*;

pub(super) fn json_error(status: StatusCode, error: impl Into<String>) -> JsonError {
    (status, Json(serde_json::json!({ "error": error.into() })))
}

fn team_config_input_to_constraints(input: &TeamConfigInput) -> TeamConstraints {
    TeamConstraints {
        max_teammates: input.max_teammates.map(|v| v as u8).unwrap_or(5),
        model_cap: input
            .model_ceiling
            .clone()
            .unwrap_or_else(|| "sonnet".to_string()),
        budget_limit: input.budget_limit,
        ..TeamConstraints::default()
    }
}

fn constraints_to_team_config_input(constraints: &TeamConstraints) -> TeamConfigInput {
    TeamConfigInput {
        max_teammates: Some(constraints.max_teammates as i32),
        model_ceiling: Some(constraints.model_cap.clone()),
        budget_limit: constraints.budget_limit,
        composition_mode: None,
    }
}

fn parse_team_config_json(json_str: Option<&String>) -> TeamConstraints {
    json_str
        .and_then(|s| serde_json::from_str::<TeamConfigInput>(s).ok())
        .map(|input| team_config_input_to_constraints(&input))
        .unwrap_or_default()
}

pub(super) fn validate_resolved_team_config(
    resolved_team_mode: Option<&String>,
    resolved_team_config_json: Option<&String>,
) -> (Option<String>, Option<String>) {
    let team_mode = match resolved_team_mode {
        Some(mode) => mode.clone(),
        None => return (None, None),
    };

    if team_mode == "solo" {
        return (Some(team_mode), None);
    }

    let resolved_constraints = parse_team_config_json(resolved_team_config_json);
    let config = team_constraints_config();
    let yaml_constraints = get_team_constraints(config, "ideation");
    let validated = validate_child_team_config(&resolved_constraints, &yaml_constraints);
    let validated_input = constraints_to_team_config_input(&validated);
    let validated_json = serde_json::to_string(&validated_input).ok();

    (Some(team_mode), validated_json)
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

pub fn session_is_team_mode(session: &IdeationSession) -> bool {
    session.team_mode.as_deref().is_some_and(|m| m != "solo")
}

pub(super) fn build_ideation_chat_service(
    state: &HttpServerState,
    session: &IdeationSession,
) -> crate::application::AppChatService {
    let app = &state.app_state;
    let mut chat_service =
        app.build_chat_service_with_execution_state(Arc::clone(&state.execution_state));
    if session_is_team_mode(session) {
        chat_service = chat_service.with_team_mode(true);
    }

    chat_service
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
    let app_handle = state.app_state.app_handle.clone();

    if let Err(re) = db
        .run(move |conn| SessionRepo::reset_auto_verify_sync(conn, &pid_for_reset))
        .await
    {
        error!(
            "Failed to rollback verification state after {}: {}",
            failure_context, re
        );
    } else if let Some(handle) = app_handle {
        emit_verification_status_changed(
            &handle,
            &parent_id_str,
            VerificationStatus::Unverified,
            false,
            None,
            Some("spawn_failed"),
            Some(current_generation),
        );
    }
}
