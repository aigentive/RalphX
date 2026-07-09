use super::*;
use crate::domain::entities::{build_child_session, ChildSessionDraftInput};

pub(crate) async fn create_verification_child_session(
    state: &HttpServerState,
    parent_session_id: &str,
    description: &str,
    title: &str,
    disabled_specialists: &[String],
) -> Result<bool, String> {
    let effective_description = if disabled_specialists.is_empty() {
        description.to_string()
    } else {
        format!(
            "{}\nDISABLED_SPECIALISTS: {}",
            description,
            disabled_specialists.join(", ")
        )
    };

    let parent_id = IdeationSessionId::from_string(parent_session_id.to_string());

    let parent = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .map_err(|e| format!("Failed to fetch parent session: {}", e))?
        .ok_or_else(|| format!("Parent session {} not found", parent_session_id))?;

    let child_session = build_child_session(
        parent_id.clone(),
        &parent,
        ChildSessionDraftInput {
            title: Some(title.to_string()),
            inherit_context: true,
            team_mode: None,
            team_config_json: None,
            source_task_id: None,
            source_context_type: None,
            source_context_id: None,
            spawn_reason: None,
            blocker_fingerprint: None,
            purpose: SessionPurpose::Verification,
            is_external_trigger: false,
        },
    );

    let child_id = child_session.id.clone();
    let child_session_str = child_id.as_str().to_string();
    let parent_session_str = parent_session_id.to_string();

    let created_session = state
        .app_state
        .ideation_session_repo
        .create(child_session)
        .await
        .map_err(|e| format!("Failed to create verification child session: {}", e))?;

    let link = SessionLink::new(
        parent_id.clone(),
        child_id.clone(),
        SessionRelationship::FollowOn,
    );
    state
        .app_state
        .session_link_repo
        .create(link)
        .await
        .map_err(|e| format!("Failed to create session link: {}", e))?;

    let chat_service = build_ideation_chat_service(state, &created_session);
    let orchestration_triggered = match chat_service
        .send_message(
            ChatContextType::Ideation,
            &child_session_str,
            effective_description.as_str(),
            Default::default(),
        )
        .await
    {
        Ok(send_result) => {
            if send_result.queued_as_pending {
                tracing::info!(
                    session_id = child_session_str,
                    "Verification child launch deferred because ideation capacity is full"
                );
                if let Err(persist_err) = state
                    .app_state
                    .ideation_session_repo
                    .set_pending_initial_prompt(
                        &child_session_str,
                        Some(effective_description.clone()),
                    )
                    .await
                {
                    error!(
                        "Failed to persist pending_initial_prompt for capacity-deferred verification child {}: {}",
                        child_session_str, persist_err
                    );
                }
                false
            } else {
                true
            }
        }
        Err(e) => {
            error!(
                "Failed to spawn ralphx-plan-verifier on verification child session {}: {}",
                child_session_str, e
            );
            // Archive the child row so it does not linger as an orphan
            if let Err(archive_err) = state
                .app_state
                .ideation_session_repo
                .update_status(&child_id, IdeationSessionStatus::Archived)
                .await
            {
                error!(
                    "Failed to archive verification child session {} after spawn failure: {}",
                    child_session_str, archive_err
                );
            }
            false
        }
    };

    let session_title = created_session
        .title
        .clone()
        .unwrap_or_else(|| title.to_string());
    crate::http_server::emit_http_event(
        state,
        "ideation:child_session_created",
        serde_json::json!({
            "sessionId": child_session_str,
            "parentSessionId": parent_session_str,
            "title": session_title,
            "purpose": "verification",
            "orchestrationTriggered": orchestration_triggered,
            "pendingInitialPrompt": if orchestration_triggered { serde_json::Value::Null } else { serde_json::json!(description) }
        }),
    );

    Ok(orchestration_triggered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::{AppState, TeamService, TeamStateTracker};
    use crate::commands::ExecutionState;
    use crate::domain::entities::{ArtifactId, IdeationSession, ProjectId, SessionOrigin};
    use crate::http_server::types::HttpServerState;
    use std::sync::Arc;

    async fn setup_sqlite_state() -> HttpServerState {
        let app_state = Arc::new(AppState::new_sqlite_test());
        let execution_state = Arc::new(ExecutionState::new());
        let tracker = TeamStateTracker::new();
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
        HttpServerState {
            app_state,
            execution_state,
            team_tracker: tracker,
            team_service,
            delegation_service: Default::default(),
        }
    }

    fn make_parent_session(plan_artifact_id: Option<ArtifactId>) -> IdeationSession {
        IdeationSession {
            id: IdeationSessionId::new(),
            project_id: ProjectId::from_string("proj-test".to_string()),
            title: Some("Verification Parent".to_string()),
            status: IdeationSessionStatus::Active,
            plan_artifact_id,
            inherited_plan_artifact_id: None,
            seed_task_id: None,
            parent_session_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            archived_at: None,
            converted_at: None,
            team_mode: None,
            team_config_json: None,
            title_source: None,
            verification_status: VerificationStatus::default(),
            verification_in_progress: false,
            verification_generation: 0,
            verification_current_round: None,
            verification_max_rounds: None,
            verification_gap_count: 0,
            verification_gap_score: None,
            verification_convergence_reason: None,
            source_project_id: Some("source-project".to_string()),
            source_session_id: Some("source-session".to_string()),
            source_task_id: None,
            source_context_type: None,
            source_context_id: None,
            spawn_reason: None,
            blocker_fingerprint: None,
            session_purpose: SessionPurpose::default(),
            session_flow: Default::default(),
            cross_project_checked: true,
            plan_version_last_read: None,
            origin: SessionOrigin::External,
            expected_proposal_count: None,
            auto_accept_status: None,
            auto_accept_started_at: None,
            api_key_id: None,
            idempotency_key: None,
            external_activity_phase: None,
            external_last_read_message_id: None,
            dependencies_acknowledged: false,
            proposal_generation_progress: Default::default(),
            pending_initial_prompt: None,
            acceptance_status: None,
            verification_confirmation_status: None,
            analysis: Default::default(),
            last_effective_model: None,
        }
    }

    fn saturate_ideation_capacity(state: &HttpServerState) {
        state.execution_state.set_global_max_concurrent(1);
        state.execution_state.increment_running();
    }

    #[tokio::test]
    async fn test_verification_child_deferred_capacity_returns_false_and_keeps_child_queued() {
        let state = setup_sqlite_state().await;
        saturate_ideation_capacity(&state);

        let parent = make_parent_session(Some(ArtifactId::new()));
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .expect("parent should be created");

        let orchestration_triggered = create_verification_child_session(
            &state,
            parent_id.as_str(),
            "Run verification round loop",
            "Auto-verification",
            &[],
        )
        .await
        .expect("verification child creation should still succeed");

        assert!(
            !orchestration_triggered,
            "capacity-deferred verification child must report orchestration_triggered=false"
        );

        let children = state
            .app_state
            .ideation_session_repo
            .get_children(&parent_id)
            .await
            .expect("children should load");
        assert_eq!(children.len(), 1, "one child row should have been created");
        let child = &children[0];
        assert_eq!(child.session_purpose, SessionPurpose::Verification);
        assert_eq!(child.status, IdeationSessionStatus::Active);
        assert_eq!(
            child.pending_initial_prompt.as_deref(),
            Some("Run verification round loop")
        );
        assert_eq!(child.origin, SessionOrigin::External);
        assert_eq!(child.source_project_id.as_deref(), Some("source-project"));
        assert_eq!(child.source_session_id.as_deref(), Some("source-session"));
    }
}
